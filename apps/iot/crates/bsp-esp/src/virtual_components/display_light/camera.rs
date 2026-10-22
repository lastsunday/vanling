//! The Camera page: the sensor's own frame plus the readout over it.
use super::draw::{mode_word, stamp_text};
use super::stack::note_stack_high_water;
use super::*;

/// The overlay inputs of a camera frame that are not the frame itself: what the counters say,
/// which window the sensor is cropping to, where the diagnostics go, and what the page can
/// claim about itself.
struct CameraStamp<'a> {
    counters: CameraCounters,
    geometry: Option<WindowGeometry>,
    diagnostics: &'a Diagnostics,
    fps: u8,
    /// The page has just been entered and no frame has arrived yet, so the numbers would all
    /// read zero and `NOBUS` would claim there is no sensor. Says it is starting instead.
    starting: bool,
}

/// The Camera page's readout, stamped over the sensor's picture.
///
/// A free function of the buffer and its numbers, for the reason [`stamp_pixel`] is: a
/// camera readout's layout is otherwise only checkable by looking at a panel, and here it can
/// be stamped into a bare buffer and read back as pixels.
///
/// Two facts decide what is worth printing. The field of view is arithmetic — output width
/// over array width — so a decimation ratio that reads back wrong is a stretched picture of
/// the right shape, and the ratio and bins are what say so. And a pixel clock the sensor's PLL
/// will not lock to produces perfectly framed noise with every register reading back correct,
/// which the fingerprint is the only thing here that can see. Between them the geometry rows
/// answer "is this the picture I asked for" and the fingerprint answers "is there a picture".
///
/// The exposure line is the sensor's own and is AGC-driven, so it says whether a slow frame is
/// the part's doing rather than the transport's. It is read once at bring-up rather than
/// polled, because it is I²C and it does not change on its own.
fn stamp_camera(frame: &mut [u8], width: usize, height: usize, stamp: CameraStamp<'_>) {
    let CameraStamp {
        counters,
        geometry,
        diagnostics,
        fps,
        starting,
    } = stamp;
    stamp_text(
        frame,
        b"CAMERA",
        block_x(Block::First),
        overlay_row_top(0),
        width,
        height,
    );
    if starting {
        stamp_text(
            frame,
            b"WAIT",
            block_value_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
    }
    if let Some(geometry) = geometry {
        let ratio = &mut [0u8; 6];
        let ratio = write_ratio(ratio, geometry.subsample);
        stamp_text(
            frame,
            b"WIN",
            block_x(Block::First),
            overlay_row_top(1),
            width,
            height,
        );
        stamp_text(
            frame,
            ratio,
            block_value_x(Block::First),
            overlay_row_top(1),
            width,
            height,
        );
        let out = &mut [0u8; 12];
        let out = format_pair(Some((geometry.out_width, geometry.out_height)), out);
        stamp_text(
            frame,
            b"OUT",
            block_x(Block::First),
            overlay_row_top(2),
            width,
            height,
        );
        stamp_text(
            frame,
            out,
            block_value_x(Block::First),
            overlay_row_top(2),
            width,
            height,
        );
        let read = &mut [0u8; 12];
        let read = format_pair(Some((geometry.win_width, geometry.win_height)), read);
        stamp_text(
            frame,
            b"RD",
            block_x(Block::First),
            overlay_row_top(3),
            width,
            height,
        );
        stamp_text(
            frame,
            read,
            block_value_x(Block::First),
            overlay_row_top(3),
            width,
            height,
        );
    } else {
        stamp_text(
            frame,
            if starting { b"----" } else { b"NOBUS" },
            block_value_x(Block::First),
            overlay_row_top(1),
            width,
            height,
        );
    }

    // Frames, and the re-arms that cost: on a single-buffer ring every whole frame costs one
    // rebuild, so the two counters move together and a gap between them is what a stall looks
    // like.
    let frames = &mut [0u8; 12];
    let frames = format_u32(counters.frames, frames);
    stamp_text(
        frame,
        b"FRM",
        block_x(Block::Second),
        overlay_row_top(0),
        width,
        height,
    );
    stamp_text(
        frame,
        frames,
        block_value_x(Block::Second),
        overlay_row_top(0),
        width,
        height,
    );
    let restarts = &mut [0u8; 12];
    let restarts = format_u32(counters.restarts, restarts);
    stamp_text(
        frame,
        b"RM",
        block_x(Block::Second),
        overlay_row_top(1),
        width,
        height,
    );
    stamp_text(
        frame,
        restarts,
        block_value_x(Block::Second),
        overlay_row_top(1),
        width,
        height,
    );
    // The light's own row, because the Camera page does not draw the fill that row would
    // otherwise sit on — the state still moved, so the readout should still say where it is.
    let live = &diagnostics.lights[0];
    // Three glyphs, like every other label: the reading column sits a block label slot from
    // the block's left edge, and a four-glyph label fills that slot exactly, which leaves the
    // value butted against it. The value is the mode word either way, so the label says as
    // much as it needs to and no more.
    let mode = mode_word(live.mode);
    stamp_text(
        frame,
        b"MOD",
        block_x(Block::Second),
        overlay_row_top(2),
        width,
        height,
    );
    stamp_text(
        frame,
        mode,
        block_value_x(Block::Second),
        overlay_row_top(2),
        width,
        height,
    );

    // A fold over the frame's own bytes, over a prime stride so the walk lands on a spread of
    // rows rather than tracking one. Sparse on purpose: this runs on every painted frame.
    // Folded before any glyph goes down, so the number describes the sensor's picture and not
    // the readout sitting on it — otherwise the readout itself moves it every repaint.
    // Skipped while starting: the buffer holds the fill, not a sensor picture, so folding it
    // would report a fingerprint for a frame that does not exist.
    if !starting {
        let sampled = &mut [0u8; 12];
        let sampled = format_u32(frame_fingerprint(frame), sampled);
        stamp_text(
            frame,
            b"FP",
            block_x(Block::Third),
            overlay_row_top(0),
            width,
            height,
        );
        stamp_text(
            frame,
            sampled,
            block_value_x(Block::Third),
            overlay_row_top(0),
            width,
            height,
        );
    }

    if DEBUG_DIAGNOSTICS {
        DisplayLight::stamp_fps(frame, fps, width, height);
    }
}

impl DisplayLight {
    /// Advances the camera and, if a whole frame arrived, stamps the readout over it and
    /// ships it. Returns whether this pass painted.
    ///
    /// The frame is not copied: the buffer the camera just filled is the one the panel reads,
    /// which is why they are the same bytes. The ordering is the load-bearing part — `advance`
    /// reports a frame and does not re-arm until the next call, so nothing writes the buffer
    /// between it being reported and the panel being given it, and the readout goes on top of
    /// a picture that is not being rewritten underneath.
    pub(crate) fn paint_camera(&mut self, now_ms: u64) -> bool {
        // Each field is borrowed where it is needed rather than copied out, and the borrows
        // are of disjoint fields so they hold at once. The snapshot is the reason: it embeds a
        // 1.6 KB audio envelope, and copying it here would put a kilobyte and a half on a stack
        // this path is already close to the floor of — see the note on `paint`.
        let bytes = self.frame_bytes();
        let diagnostics = &self.diagnostics;
        let fps = self.fps;
        let width = usize::from(self.panel.width());
        let height = usize::from(self.panel.height());
        let Some(camera) = &mut self.camera else {
            return false;
        };
        let frame = &mut self.frame[..bytes];

        // Parked by the last page change, so this is where it comes back — with the chain it
        // handed back, over a buffer whose contents were a colour fill until now.
        if !self.owner.is_camera() {
            camera.resume(frame);
            self.owner.taken();
        }
        let sample = camera.advance(frame, now_ms);
        if sample != FrameAdvance::Fresh {
            return false;
        }
        // `advance` still runs (it is what repairs the chain), so a throttled camera leaves
        // the picture fresh without shipping it.
        if self.diagnostics.demand.plane(PlaneKey::CAMERA).period_ms >= CAMERA_THUMBNAIL_PERIOD_MS {
            let due = self
                .thumbnail_last_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= THUMBNAIL_INTERVAL_MS);
            if !due {
                return false;
            }
            self.thumbnail_last_ms = Some(now_ms);
        }
        let counters = camera.counters();
        let geometry = camera.geometry();

        let render_start = Instant::now();
        // A free function rather than a method: the glyph writes need the buffer while the
        // snapshot and the panel are reached through `self`, and a method taking `&mut self`
        // could not hold those at once. Disjoint fields, so it can be handed all three. The
        // rate is read out first because it is a `self` field and `frame` borrows `self`.
        stamp_camera(
            frame,
            width,
            height,
            CameraStamp {
                counters,
                geometry,
                diagnostics,
                fps,
                starting: false,
            },
        );
        let render = render_start.elapsed();

        let write_start = Instant::now();
        if let Err(error) = self.panel.write_frame(&self.frame[..bytes]) {
            log::error!("[DISPLAY] camera frame write failed: {error:?}");
        } else {
            self.note_frame_shipped();
        }
        self.sample_frame_cost(render, write_start.elapsed());
        self.sample_fps();
        // Sampled here because the camera page reaches the panel through this path and nowhere
        // else: without it the reading is `0 B` on this page for the whole run, which says
        // nothing about the stack. `consume` covers the light pages and `stamp_sweep` the audio
        // page's own working set.
        note_stack_high_water();
        true
    }

    /// Draws the Camera page's own frame, for the tick where the page has just been entered
    /// and no sensor frame has arrived yet.
    ///
    /// The camera page has no fill — the sensor's picture is the page — but leaving the previous
    /// page's colour on the panel until a frame lands reads as a stall: at this sensor's rate
    /// that is about a fifth of a second of a frozen picture. This says `WAIT` instead, and
    /// leaves no fingerprint, because the bytes it would fold are the fill rather than a sensor
    /// picture.
    pub(super) fn stamp_camera_page(&mut self, width: usize, height: usize) {
        stamp_camera(
            self.frame,
            width,
            height,
            CameraStamp {
                counters: CameraCounters::ZERO,
                geometry: None,
                diagnostics: &self.diagnostics,
                fps: self.fps,
                starting: true,
            },
        );
    }
}
