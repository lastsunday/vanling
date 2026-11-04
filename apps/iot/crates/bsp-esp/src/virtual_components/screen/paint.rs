//! The colour path: a fill, the page's rows over it, and the frame that ships.
//!
//! Page drawing belongs to the product: this module only decides whether a frame is worth
//! painting (`FrameOwner`, which also remembers what the CPU last painted) and hands the
//! buffer to the painter the app injected.

use core::any::Any;
use core::future::Future;
use core::pin::Pin;

use esp_hal::time::{Duration, Instant};
use iot_core::drivers::camera::FrameOwner;
use iot_core::drivers::surface::{PageSurface, PanelContext, PanelPainter, PanelTarget};
use iot_core::stamp::rgb565;
use iot_core::stats::windowed_percent;

use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::components::st7789::Wire;
use crate::stack::{high_water, main_stack_bytes, note_stack_high_water};

use super::*;

impl PageSurface for Screen {
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

    fn repaint(&mut self, painter: &dyn PanelPainter, snapshot: &dyn Any, force: bool) -> bool {
        let Some(color) = self.owner.painted_color() else {
            return false;
        };
        if !force && !self.page_dirty {
            return false;
        }
        self.page_dirty = false;
        note_stack_high_water();
        self.paint(color, painter, snapshot);
        true
    }
}

impl Screen {
    pub(super) fn context(&self) -> PanelContext {
        PanelContext {
            instance: self.instance,
            fps: self.metrics.fps(),
            spl_offset_db: SPL_OFFSET_DECIBELS,
            stack_probe: Some(note_stack_high_water),
        }
    }

    pub(super) fn paint(&mut self, color: Rgb, painter: &dyn PanelPainter, snapshot: &dyn Any) {
        let render_start = Instant::now();
        self.fill(color);

        let bytes = self.panel.frame_bytes();
        let context = self.context();
        painter.paint(
            PanelTarget {
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
            render: render_start.elapsed(),
        });
    }

    /// Writes one colour into the whole frame and records it as the colour the CPU holds.
    pub(super) fn fill(&mut self, color: Rgb) {
        self.owner.painted(color);

        let width = usize::from(self.panel.width());
        let rgb565 = rgb565(color);
        let hi = (rgb565 >> 8) as u8;
        let lo = rgb565 as u8;
        for px in self.frame.chunks_exact_mut(width * 2) {
            for pixel in px.as_chunks_mut::<2>().0 {
                pixel[0] = hi;
                pixel[1] = lo;
            }
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
        let wire = match self.panel.write(self.frame).await {
            Ok(wire) => {
                self.panel.note_frame_shipped();
                wire
            }
            Err(error) => {
                if pending.camera {
                    log::error!("[DISPLAY] camera frame write failed: {error:?}");
                } else {
                    log::error!("[DISPLAY] frame write failed: {error:?}");
                    self.owner = FrameOwner::NeedsPaint;
                }
                let spent = started.elapsed();
                Wire {
                    total: spent,
                    longest: spent,
                }
            }
        };
        self.account_frame(pending.render, wire, started.elapsed());
    }

    pub(super) fn account_frame(&mut self, render: Duration, wire: Wire, wait: Duration) {
        if let Some(cost) = self.metrics.fold_frame(render, wire, wait) {
            let percent = windowed_percent(cost.hold.as_micros(), cost.window.as_micros());
            let over_budget = percent > 25 || cost.worst_write.as_millis() > 20;
            log::log!(
                if over_budget {
                    log::Level::Warn
                } else {
                    log::Level::Debug
                },
                "[DISPLAY] {} repaints in {} ms, held the executor {} ms ({}%), worst render {} ms, worst wait {} ms, worst write {} ms (longest chunk {} ms)",
                cost.frames,
                cost.window.as_millis(),
                cost.hold.as_millis(),
                percent,
                cost.worst_render.as_millis(),
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
