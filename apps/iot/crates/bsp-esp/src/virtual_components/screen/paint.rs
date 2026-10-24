//! The colour path: a fill, the page's rows over it, and the frame that ships.
//!
//! Page drawing is `iot-ui`'s; this module decides whether a frame is worth painting
//! (`FrameOwner`, which also remembers what the CPU last painted) and owns the diagnostics
//! repaint gate, which is what keeps a live page from holding the shared executor in a
//! ~15 ms block every 20 ms snapshot.

use core::sync::atomic::{AtomicU32, Ordering};

use esp_hal::time::{Duration, Instant};
use iot_core::demand::PlaneKey;
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::camera::FrameOwner;
use iot_core::drivers::light::{scale_brightness, vertical_brightness};
use iot_core::render::{diag_repaint_due, windowed_percent};
use iot_core::stamp::rgb565;
use iot_ui::{Canvas, Surface};

use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::stack::{high_water, main_stack_bytes, note_stack_high_water};

use super::*;

/// Last full-frame repaint taken by the panel, in milliseconds since boot.
/// Shared across the diagnostics pushes so the repaint rate bound adds no width
/// to the renderer state the main-task async machine holds across awaits.
static DIAG_LAST_REPAINT_MS: AtomicU32 = AtomicU32::new(0);

impl DiagnosticsSink for Screen {
    /// The panel is the surface the product's pages are drawn on: it is the one with the frame
    /// buffer, and `consume` below is what stamps the page's rows into it.
    fn draws_pages(&self) -> bool {
        true
    }

    fn consume(&mut self, diagnostics: &Diagnostics) {
        if self.diagnostics == *diagnostics {
            return;
        }
        let page_changed = self.diagnostics.page != diagnostics.page;
        // Leaving the camera's page is what hands the buffer back, and it is done here rather
        // than in `paint` for two reasons. A chain left running would overwrite every fill
        // after it a row at a time; and waiting for `paint` to release it deadlocked the
        // panel, because taking the buffer is what clears the colour `paint` reads to know
        // what to draw — so the call that had to release it was gated on there being a colour,
        // and the page that needed releasing was the one that had cleared it.
        // Gated on the demand rather than on the page: a demand granting no mode is
        // something an application other than a page switch could ask for.
        if !diagnostics
            .demand
            .plane(PlaneKey::CAMERA)
            .mode
            .is_collecting()
            && self.owner.is_camera()
        {
            self.camera.pause();
            self.owner.released();
        }
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
        self.diagnostics = diagnostics.clone();
        if !repaint {
            return;
        }
        if let Some((fill, color)) = self.owner.painted_color() {
            note_stack_high_water();
            self.paint(fill, color);
            DIAG_LAST_REPAINT_MS.store(now_ms, Ordering::Relaxed);
        }
    }
}

impl Screen {
    /// Paints `fill`/`color`, stamps the page's rows over it, and ships the frame.
    /// Always paints; callers guard for repaint skipping.
    pub(super) fn paint(&mut self, fill: Fill, color: Rgb) {
        let render_start = Instant::now();
        self.owner.painted(fill, color);

        let height = self.panel.height();
        let colors = |row: u16| match fill {
            Fill::Uniform => color,
            Fill::VerticalGradient => scale_brightness(color, vertical_brightness(row, height)),
        };
        let width = usize::from(self.panel.width());
        for (row, px) in self.frame.chunks_exact_mut(width * 2).enumerate() {
            let rgb565 = rgb565(colors(row as u16));
            let hi = (rgb565 >> 8) as u8;
            let lo = rgb565 as u8;
            for pixel in px.chunks_exact_mut(2) {
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

        // The render and the transfer are timed apart rather than together: this
        // whole path runs on the one cooperative executor the playback feed and
        // the capture poll share, and "the frame cost 20 ms" does not say whether
        // the frame can be made cheaper or whether the SPI transfer has to yield
        // the bus. Those need different fixes, so the log has to tell them apart.
        let render = render_start.elapsed();
        let write_start = Instant::now();
        if let Err(e) = self.panel.write(self.frame) {
            log::error!("[DISPLAY] frame write failed: {e:?}");
            // Stay dark and let the next fill retry: waking the panel now would
            // show the frame this write failed to replace.
            self.owner = FrameOwner::NeedsPaint;
        } else {
            self.panel.note_frame_shipped();
        }
        self.account_frame(render, write_start.elapsed());
    }

    /// Folds one frame's costs and rate into their windows, reporting a closed cost window and
    /// the stack peak on that same window.
    pub(super) fn account_frame(&mut self, render: Duration, write: Duration) {
        if let Some(cost) = self.metrics.fold_frame(render, write) {
            let percent = windowed_percent(cost.hold.as_micros(), cost.window.as_micros());
            // A window is telemetry; one this expensive is the fault the report exists for,
            // and the one shape worth waking a reader with.
            let over_budget = percent > 25 || cost.worst_write.as_millis() > 20;
            log::log!(
                if over_budget {
                    log::Level::Warn
                } else {
                    log::Level::Debug
                },
                "[DISPLAY] {:?} {} repaints in {} ms, held the executor {} ms ({}%), worst render {} ms, worst write {} ms; asked {}",
                self.diagnostics.page,
                cost.frames,
                cost.window.as_millis(),
                cost.hold.as_millis(),
                percent,
                cost.worst_render.as_millis(),
                cost.worst_write.as_millis(),
                self.diagnostics.demand,
            );
            // On the window the cost line closes rather than on the repaint rate: the peak
            // only moves when a page draws deeper, and the rate window repeated the same
            // number twice a second.
            self.report_stack_high_water();
        }
        self.metrics.fold_fps();
    }

    /// Reports the deepest main-stack use seen so far, when it is news.
    ///
    /// Sampled here rather than at bring-up because the peak belongs to whichever page drew
    /// deepest, and the Audio page's sweep is the frame that decides whether the stack still
    /// fits. A window repeats while nothing new is drawn deeper, so the line follows the peak
    /// rather than the window: once at every kilobyte gained, and on `debug` for the unchanged
    /// reading a record may need to quote.
    fn report_stack_high_water(&mut self) {
        let high = high_water();
        // Zero is the sampler before the first frame has used the stack, not a depth.
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
