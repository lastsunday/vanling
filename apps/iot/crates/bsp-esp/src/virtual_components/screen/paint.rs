//! The colour path: a fill, the page's rows over it, and the frame that ships.
//!
//! Page drawing is `iot-ui`'s; this module decides whether a frame is worth painting
//! (`FrameOwner`, which also remembers what the CPU last painted) and owns the diagnostics
//! repaint gate, which is what keeps a live page from holding the shared executor in a
//! ~15 ms block every 20 ms snapshot.

use core::sync::atomic::{AtomicU32, Ordering};

use esp_hal::time::{Duration, Instant};
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::camera::FrameOwner;
use iot_core::render::{diag_repaint_due, windowed_percent};
use iot_core::stamp::rgb565;
use iot_ui::{Canvas, Surface};

use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::components::st7789::Wire;
use crate::stack::{high_water, main_stack_bytes, note_stack_high_water};

use super::*;

static DIAG_LAST_REPAINT_MS: AtomicU32 = AtomicU32::new(0);

impl DiagnosticsSink for Screen {
    fn consume(&mut self, diagnostics: &Diagnostics) {
        if self.diagnostics == *diagnostics {
            return;
        }
        let page_changed = self.diagnostics.page != diagnostics.page;
        // A page switch is drawn immediately; a diagnostics drift is only drawn
        // when the page redraws it and the rate gate has opened. The gate exists
        // because a repaint ships the whole 320×240 frame down one blocking SPI
        // transfer, and without it the live pages (Audio follows the capture,
        // Attitude the motion sample) hold the shared cooperative executor in a
        // ~15 ms block every 20 ms snapshot — starving the playback feed and the
        // capture poll, which then restart their DMAs (the `RST` counter climbs).
        let now_ms = Instant::now().duration_since_epoch().as_millis() as u32;
        let since_ms = now_ms.wrapping_sub(DIAG_LAST_REPAINT_MS.load(Ordering::Relaxed)) as u64;
        let repaint = page_changed
            || diag_repaint_due(
                diagnostics.page,
                &self.diagnostics,
                diagnostics,
                since_ms,
                DEBUG_DIAGNOSTICS,
            );
        // The previous snapshot is read through `self` right up to the decision
        // above rather than saved into a local: `Diagnostics` embeds a 1.6 KB
        // audio envelope, and one by-value copy of it is a large slice of a main
        // stack this path is already close to overflowing.
        self.diagnostics = *diagnostics;
        if !repaint {
            return;
        }
        if let Some(color) = self.owner.painted_color() {
            note_stack_high_water();
            self.paint(color);
            DIAG_LAST_REPAINT_MS.store(now_ms, Ordering::Relaxed);
        }
    }
}

impl Screen {
    pub(super) fn paint(&mut self, color: Rgb) {
        let render_start = Instant::now();
        self.owner.painted(color);

        let height = self.panel.height();
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

        let bytes = self.panel.frame_bytes();
        let ground = self.owner.painted_color();
        {
            let canvas = Canvas::new(&mut self.frame[..bytes], width, usize::from(height), ground);
            let mut surface = Surface::new(
                canvas,
                &self.diagnostics,
                self.instance,
                self.metrics.fps(),
                DEBUG_DIAGNOSTICS,
                SPL_OFFSET_DECIBELS,
            )
            .with_stack_probe(note_stack_high_water);
            surface.render(self.diagnostics.page);
        }

        // The render and the transfer are timed apart rather than together: "the frame cost
        // 20 ms" does not say which of the two has to be made cheaper, so they are reported as
        // separate peaks. `worst_chunk` is the transfer's longest single hold, which is the
        // number the chunking moves; `worst write` stays the whole frame's wire time.
        self.pending = Some(Pending {
            camera: false,
            render: render_start.elapsed(),
        });
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
                "[DISPLAY] {:?} {} repaints in {} ms, held the executor {} ms ({}%), worst render {} ms, worst wait {} ms, worst write {} ms (longest chunk {} ms); asked {}",
                self.diagnostics.page,
                cost.frames,
                cost.window.as_millis(),
                cost.hold.as_millis(),
                percent,
                cost.worst_render.as_millis(),
                cost.worst_wait.as_millis(),
                cost.worst_write.as_millis(),
                cost.worst_chunk.as_millis(),
                self.diagnostics.demand,
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
