//! The colour path: a fill, the page's rows over it, and the frame that ships.
//!
//! PageId drawing belongs to the product: this module only decides whether a frame is worth
//! painting (`FrameOwner`, which also remembers what the CPU last painted) and hands the
//! buffer to the painter the app injected.

use core::any::Any;
use core::future::Future;
use core::pin::Pin;

use esp_hal::time::{Duration, Instant};
use iot_core::drivers::surface::{ScreenContext, ScreenPainter, ScreenSurface, ScreenTarget};
use iot_core::light::rgb565;
use iot_core::stats::windowed_percent;

use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::components::st7789::Wire;
use crate::stack::{high_water, main_stack_bytes, note_stack_high_water};

use super::*;

impl ScreenSurface for Screen {
    fn repaint_step(&self) -> u8 {
        REPAINT_STEP
    }

    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        self.pending.as_ref()?;
        Some(Box::pin(self.flush_pending()))
    }

    fn set_backlight(&mut self, level_pct: u8) {
        self.panel.set_backlight(level_pct);
    }

    fn set_backlight_ceiling(&mut self, level_pct: u8) {
        self.panel.set_ceiling(level_pct);
    }

    fn repaint(
        &mut self,
        painter: &mut dyn ScreenPainter,
        snapshot: &dyn Any,
        force: bool,
    ) -> bool {
        let Some(color) = self.owner.painted_color() else {
            return false;
        };
        if !force && !self.page_dirty {
            return false;
        }
        // A fill is the only thing between the last page and this one: when there was none,
        // the buffer still holds the picture the painter is about to scroll.
        let keep = !self.page_dirty && painter.keeps_frame(snapshot);
        self.page_dirty = false;
        if !keep {
            painter.reset();
        }
        note_stack_high_water();
        self.paint(color, painter, snapshot, keep);
        true
    }
}

impl Screen {
    pub(super) fn context(&self) -> ScreenContext {
        ScreenContext {
            instance: self.instance,
            fps: self.metrics.fps(),
            spl_offset_db: SPL_OFFSET_DECIBELS,
            stack_probe: Some(note_stack_high_water),
        }
    }

    pub(super) fn paint(
        &mut self,
        color: Rgb,
        painter: &mut dyn ScreenPainter,
        snapshot: &dyn Any,
        keep: bool,
    ) {
        let render_start = Instant::now();
        self.owner.painted(color);
        let fill = if keep {
            Duration::ZERO
        } else {
            let started = Instant::now();
            self.fill_pixels(color);
            started.elapsed()
        };

        let bytes = self.panel.frame_bytes();
        let context = self.context();
        painter.paint(
            ScreenTarget {
                frame: &mut self.frame[..bytes],
                width: usize::from(self.panel.width()),
                height: usize::from(self.panel.height()),
                ground: Some(color),
            },
            context,
            snapshot,
        );

        // The render and the transfer are timed apart rather than together: "the frame cost
        // 20 ms" does not say which of the two has to be made cheaper, so they are reported as
        // separate peaks. `worst_chunk` is the transfer's longest single hold, which is the
        // number the chunking moves; `worst write` stays the whole frame's wire time.
        self.pending = Some(Pending {
            camera: false,
            color: Some(color),
            render: render_start.elapsed(),
            fill,
            damage: painter.damage(),
        });
    }

    /// Writes one colour into the whole frame and records it as the colour the CPU holds.
    pub(super) fn fill(&mut self, color: Rgb) {
        self.owner.painted(color);
        self.fill_pixels(color);
    }

    fn fill_pixels(&mut self, color: Rgb) {
        let rgb565 = rgb565(color);
        let (hi, lo) = ((rgb565 >> 8) as u8, rgb565 as u8);
        // Two pixels a store: a byte at a time is 153 600 iterations of one-byte writes on a
        // path that runs twice per breathing frame.
        let pair = [hi, lo, hi, lo];
        let (blocks, rest) = self.frame.as_chunks_mut::<4>();
        for block in blocks {
            block.copy_from_slice(&pair);
        }
        for pixel in rest.as_chunks_mut::<2>().0 {
            pixel.copy_from_slice(&pair[..2]);
        }
    }

    /// Ships whatever the last paint left in the buffer, then accounts for the frame. Split
    /// from painting because the panel's transfer is a DMA operation: awaiting it lets the
    /// executor run between chunks instead of holding it for the whole frame.
    pub(super) async fn flush_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let started = Instant::now();
        let wire = match self.panel.write(self.frame, pending.damage).await {
            Ok(wire) => {
                self.panel.note_frame_shipped();
                wire
            }
            Err(error) => {
                if pending.camera {
                    log::error!("[DISPLAY] camera frame write failed: {error:?}");
                } else {
                    log::error!("[DISPLAY] frame write failed: {error:?}");
                    // The panel holds part of a frame and the buffer no longer matches it:
                    // refill the ground and let the next repaint draw the page whole.
                    if let Some(color) = pending.color {
                        self.fill(color);
                        self.page_dirty = true;
                    }
                }
                let spent = started.elapsed();
                Wire {
                    total: spent,
                    longest: spent,
                }
            }
        };
        self.account_frame(pending.render, pending.fill, wire, started.elapsed());
    }

    pub(super) fn account_frame(
        &mut self,
        render: Duration,
        fill: Duration,
        wire: Wire,
        wait: Duration,
    ) {
        if let Some(cost) = self.metrics.fold_frame(render, fill, wire, wait) {
            let percent = windowed_percent(cost.hold.as_micros(), cost.window.as_micros());
            let over_budget = percent > 25 || cost.worst_write.as_millis() > 20;
            log::log!(
                if over_budget {
                    log::Level::Warn
                } else {
                    log::Level::Debug
                },
                "[DISPLAY] {} repaints in {} ms, held the executor {} ms ({}%), worst render {} ms (fill {}), worst wait {} ms, worst write {} ms (longest chunk {} ms)",
                cost.frames,
                cost.window.as_millis(),
                cost.hold.as_millis(),
                percent,
                cost.worst_render.as_millis(),
                cost.worst_fill.as_millis(),
                cost.worst_wait.as_millis(),
                cost.worst_write.as_millis(),
                cost.worst_chunk.as_millis(),
            );
            self.report_stack_high_water();
        }
        self.metrics.fold_fps();
    }

    fn report_stack_high_water(&mut self) {
        let high = high_water();
        if high == 0 {
            return;
        }
        let total = main_stack_bytes();
        let grew = self.metrics.fold_stack_peak(high);
        log::log!(
            if grew {
                log::Level::Info
            } else {
                log::Level::Debug
            },
            "[DISPLAY] stack peak {high} B of {total} B",
        );
    }
}
