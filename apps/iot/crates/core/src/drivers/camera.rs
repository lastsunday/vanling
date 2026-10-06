//! What the render layer needs from a camera, and nothing about how one is wired.
//!
//! The panel is the only thing that can show a frame, and it holds exactly one frame buffer,
//! so a camera that shares it has to be handed that buffer rather than own a second copy —
//! one frame is 150 KB and a board cannot spare two. The buffer therefore travels as an
//! argument on every call instead of being stored in the trait: one `&mut` owner, and no way
//! to write those bytes twice at once.
//!
//! Everything here is a shape, not a mechanism. The trait carries the cadence, the counters
//! and the sensor's read-back geometry, so a panel can drive a camera and a diagnostics row
//! can print what one is doing without either naming a peripheral. Which pixels arrive and
//! how quickly is the driver's business; `@/records/iot/camera.md`.

use alloc::boxed::Box;

use crate::drivers::light::{Fill, Rgb};

/// What one call to [`FrameSource::advance`] found.
///
/// Reported per call rather than as a predicate because the caller needs all three: a fresh
/// frame is one to draw, a repeat means the chain stalled and the picture is going stale, and
/// a stall means the same thing sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAdvance {
    /// A frame newer than the one previously reported is in `frame`.
    Fresh,
    /// Nothing new. The chain is alive but has not finished another frame.
    Same,
    /// Nothing new for long enough that the chain was rebuilt.
    Stalled,
}

/// Running totals, so a readout can say whether the picture is advancing and whether it is
/// arriving by the chain rebuilding itself.
///
/// Every counter zero rather than `Default`, so a board can declare one in a `const` the way
/// the other capability shapes are declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraCounters {
    /// Frames handed out since boot.
    pub frames: u32,
    /// Polls that found no frame newer than the last one.
    pub repeated: u32,
    /// Chains rebuilt since boot — the stall repairs, which is also what every whole frame
    /// costs on a single-buffer ring.
    pub restarts: u32,
    /// Descriptors the DMA had finished at the last poll, against [`FrameSource`]'s own
    /// count of them.
    pub finished: u32,
}

impl CameraCounters {
    pub const ZERO: Self = Self {
        frames: 0,
        repeated: 0,
        restarts: 0,
        finished: 0,
    };
}

impl Default for CameraCounters {
    fn default() -> Self {
        Self::ZERO
    }
}

/// The window the sensor is actually reading out, as the part reports it.
///
/// Read-back, not the values written: an output size that reads back correct while the
/// decimation ratio is wrong is a picture of the right shape and the wrong content. The
/// ratio and the bins are what make that distinguishable, so both are here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowGeometry {
    /// Output window, in output pixels.
    pub out_width: u16,
    pub out_height: u16,
    /// Readout window, in sensor pixels, with the margins the part counts into its own row
    /// timing.
    pub win_width: u16,
    pub win_height: u16,
    pub row_start: u16,
    pub col_start: u16,
    /// The decimation ratio as programmed, row nibble in the high half.
    pub subsample: u8,
    /// Scalar mode: `0` is off, and this driver leaves it off because it narrows the view.
    pub scalar: u8,
    /// Whether the sub-sampling bins are written.
    pub sub_bins: [u8; 8],
    pub crop_enabled: bool,
}

/// A surface a [`FrameSource`] can be bound to.
///
/// The other half of the seam from [`FrameSource`]: the source fills a buffer the surface
/// owns, and the surface knows how to stamp over it and ship it. Declared here rather than in
/// the app because the surface lives in the bsp crate and the app is a layer above it — the
/// trait is what lets the renderer hold a panel without naming either the panel or a sensor.
///
/// Separate from [`crate::drivers::light::RgbLight`] because a camera is not a colour, and
/// because only some surfaces have a frame buffer: a WS2812 strip implements [`RgbLight`]
/// and nothing more, so a board with no panel needs no camera either.
pub trait CameraTarget {
    /// Advance the bound camera and ship a frame if a whole one arrived. Returns whether it
    /// painted, so a caller can tell an idle tick from a frame that went out.
    fn paint_camera(&mut self, now_ms: u64) -> bool;

    /// Binds the camera. Takes it by value because the surface is its only consumer.
    fn attach_camera(&mut self, camera: Box<dyn FrameSource>);
}

/// Who owns the frame buffer a surface and a camera share.
///
/// The camera writes sensor frames into it while its page is up and the CPU writes colour
/// fills everywhere else, and one buffer cannot be written twice at once — so ownership is
/// one value rather than something assembled from parts, which is what lets a state that
/// cannot be left be written down as one state at all.
///
/// A surface returns to [`FrameOwner::NeedsPaint`] rather than to the colour it held before
/// the camera took the buffer: the light state is meant to keep moving while the camera page
/// is up, so restoring the earlier colour would undo it. See `@/records/iot/camera.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOwner {
    /// No owner: the surface has booted without painting, or the camera has just given the
    /// buffer back, so the bytes under the next fill are whatever the camera left.
    NeedsPaint,
    /// The CPU owns the buffer and last filled it with this.
    Painted { fill: Fill, color: Rgb },
    /// The camera owns the buffer and is writing into it.
    Camera,
}

impl FrameOwner {
    /// Whether a fill of this pair has any work to do.
    ///
    /// [`FrameOwner::Camera`] refuses rather than paints, because the chain would overwrite the
    /// fill a row at a time. The light state still moves on that page — this refuses to paint,
    /// not to change colour.
    pub fn accepts_fill(&self, fill: Fill, color: Rgb) -> bool {
        match *self {
            FrameOwner::Camera => false,
            FrameOwner::Painted { fill: f, color: c } => f != fill || c != color,
            FrameOwner::NeedsPaint => true,
        }
    }

    /// Whether the camera owns the buffer, and so whether the surface must park it before it
    /// writes anything of its own.
    pub fn is_camera(&self) -> bool {
        matches!(self, FrameOwner::Camera)
    }

    /// Records that the CPU has just painted.
    pub fn painted(&mut self, fill: Fill, color: Rgb) {
        *self = FrameOwner::Painted { fill, color };
    }

    /// Records that the camera has taken the buffer.
    pub fn taken(&mut self) {
        *self = FrameOwner::Camera;
    }

    /// Records that the camera has given the buffer back, leaving the next fill to repaint.
    pub fn released(&mut self) {
        *self = FrameOwner::NeedsPaint;
    }

    /// The colour the CPU last painted, for a repaint that redraws the same page.
    pub fn painted_color(&self) -> Option<(Fill, Rgb)> {
        match *self {
            FrameOwner::Painted { fill, color } => Some((fill, color)),
            FrameOwner::NeedsPaint | FrameOwner::Camera => None,
        }
    }
}

/// A camera the render layer can draw from.
///
/// `advance` borrows the buffer rather than the trait returning a slice of one it owns, so the
/// surface keeps sole ownership of the bytes and the camera keeps only the transfer. That is
/// what lets the panel read the DMA's bytes, stamp its readout into the same frame, and push
/// the result without a frame-sized copy anywhere.
pub trait FrameSource {
    /// Advances the capture and reports what it found.
    ///
    /// `frame` is the panel's buffer, which the implementation must invalidate before reading
    /// it (the DMA wrote it through memory the CPU may have cached) and must not write to at
    /// any other time. Returns [`FrameAdvance::Fresh`] only when the whole frame is finished:
    /// a partial one is a picture with rows shifted.
    fn advance(&mut self, frame: &mut [u8], now_ms: u64) -> FrameAdvance;

    /// Stops feeding `frame` and leaves it to the CPU, keeping the camera and its chain.
    ///
    /// The surface calls this before painting a frame of its own into the same buffer: a chain
    /// still running would overwrite what it drew. Not a fault, so not counted in
    /// [`CameraCounters::restarts`].
    fn pause(&mut self);

    /// Gives `frame` back to the camera, replacing any chain already in flight.
    ///
    /// A replacement rather than a resumption, because nothing survives a pause that is worth
    /// keeping — see the implementation.
    fn resume(&mut self, frame: &mut [u8]);

    fn counters(&self) -> CameraCounters;

    /// The sensor's readout geometry, or `None` if it is not currently reachable.
    ///
    /// Over I²C, so the implementation caches it: a caller prints it, it does not poll it.
    fn geometry(&mut self) -> Option<WindowGeometry>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source with no peripheral behind it, so the shapes can be exercised on the host.
    struct Stub {
        counters: CameraCounters,
        geometry: Option<WindowGeometry>,
        feeds: bool,
        advances: u32,
        last_len: usize,
    }

    impl Stub {
        const fn new() -> Self {
            Self {
                counters: CameraCounters::ZERO,
                geometry: None,
                feeds: false,
                advances: 0,
                last_len: 0,
            }
        }
    }

    impl FrameSource for Stub {
        fn advance(&mut self, frame: &mut [u8], _now_ms: u64) -> FrameAdvance {
            self.advances = self.advances.saturating_add(1);
            self.last_len = frame.len();
            if !self.feeds {
                return FrameAdvance::Same;
            }
            self.counters.frames = self.counters.frames.saturating_add(1);
            self.counters.finished = 40;
            FrameAdvance::Fresh
        }

        fn pause(&mut self) {
            self.feeds = false;
        }

        fn resume(&mut self, _frame: &mut [u8]) {
            self.feeds = true;
        }

        fn counters(&self) -> CameraCounters {
            self.counters
        }

        fn geometry(&mut self) -> Option<WindowGeometry> {
            self.geometry
        }
    }

    #[test]
    fn the_buffer_travels_with_the_call_rather_than_being_owned() {
        // The surface owns the bytes: whatever it passes is what gets written, and its length
        // is the frame's, so a caller cannot be handed a stale copy of a previous one.
        let mut source = Stub::new();
        let mut frame = vec![0u8; 153_600];
        assert_eq!(source.advance(&mut frame, 0), FrameAdvance::Same);
        assert_eq!(source.last_len, 153_600);
        assert_eq!(frame.len(), 153_600, "advance must not resize the buffer");
    }

    #[test]
    fn a_source_that_is_not_feeding_reports_repeats_rather_than_a_stall() {
        let mut source = Stub::new();
        let mut frame = vec![0u8; 64];
        assert_eq!(source.advance(&mut frame, 0), FrameAdvance::Same);
        assert_eq!(source.advance(&mut frame, 20), FrameAdvance::Same);
        assert_eq!(
            source.counters().repeated,
            0,
            "the source decides what to count"
        );
        assert_eq!(source.counters().frames, 0);
    }

    #[test]
    fn pausing_stops_frames_and_resuming_brings_them_back() {
        let mut source = Stub::new();
        let mut frame = vec![0u8; 64];
        source.resume(&mut frame);
        assert_eq!(source.advance(&mut frame, 0), FrameAdvance::Fresh);
        assert_eq!(source.counters().frames, 1);

        source.pause();
        assert_eq!(source.advance(&mut frame, 20), FrameAdvance::Same);
        assert_eq!(
            source.counters().frames,
            1,
            "a paused camera hands out nothing"
        );

        source.resume(&mut frame);
        assert_eq!(source.advance(&mut frame, 40), FrameAdvance::Fresh);
        assert_eq!(source.counters().frames, 2);
    }

    #[test]
    fn a_pause_is_not_counted_as_a_restart() {
        // A page change is deliberate; the counter is there to show whether the chain is
        // needing repairs it should not need.
        let mut source = Stub::new();
        let mut frame = vec![0u8; 64];
        source.pause();
        source.pause();
        source.resume(&mut frame);
        assert_eq!(source.counters().restarts, 0);
    }

    #[test]
    fn geometry_is_absent_until_a_sensor_reports_it() {
        let mut source = Stub::new();
        assert_eq!(source.geometry(), None);
        source.geometry = Some(WindowGeometry {
            out_width: 320,
            out_height: 240,
            win_width: 1616,
            win_height: 1208,
            subsample: 0x55,
            ..WindowGeometry::default()
        });
        let geometry = source.geometry().expect("present once reported");
        assert_eq!(geometry.out_width, 320);
        assert_eq!(geometry.win_width, 1616);
        assert_eq!(
            geometry.subsample, 0x55,
            "the ratio is what distinguishes a stretched frame"
        );
    }

    #[test]
    fn a_released_camera_lets_the_next_fill_through() {
        // The sequence the panel could not leave: the camera owned the buffer, the page
        // changed, and the one call that gives the buffer back was reached only when there
        // was a colour to paint — which there never was, because taking the buffer is what
        // cleared it. Every fill afterwards was refused and the panel kept the last frame.
        let mut owner = FrameOwner::NeedsPaint;
        owner.taken();
        assert!(!owner.accepts_fill(Fill::Uniform, Rgb(1, 2, 3)));
        owner.released();
        assert!(
            owner.accepts_fill(Fill::Uniform, Rgb(1, 2, 3)),
            "a fill after the camera released the buffer has to repaint, whatever it is"
        );
    }

    #[test]
    fn a_painted_colour_is_only_repeated_at_its_own_value() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Fill::Uniform, Rgb(10, 20, 30));
        assert!(!owner.accepts_fill(Fill::Uniform, Rgb(10, 20, 30)));
        assert!(owner.accepts_fill(Fill::Uniform, Rgb(10, 20, 31)));
        assert!(
            owner.accepts_fill(Fill::VerticalGradient, Rgb(10, 20, 30)),
            "the fill is part of what was painted, so a different one is work"
        );
    }

    #[test]
    fn a_camera_refuses_fills_however_far_the_colour_moved() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Fill::Uniform, Rgb(1, 1, 1));
        owner.taken();
        for level in [0, 127, 255] {
            assert!(
                !owner.accepts_fill(Fill::Uniform, Rgb(level, level, level)),
                "the light state moves on the camera's page; the buffer does not"
            );
        }
    }

    #[test]
    fn the_colour_a_camera_displaced_is_not_remembered() {
        // The light is meant to keep moving while the camera page is up, so handing the
        // buffer back must not resurrect the colour from before it.
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Fill::Uniform, Rgb(7, 8, 9));
        owner.taken();
        owner.released();
        assert_eq!(owner.painted_color(), None);
    }

    #[test]
    fn a_repaint_of_an_unchanged_page_reuses_the_colour_it_held() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Fill::VerticalGradient, Rgb(4, 5, 6));
        assert_eq!(
            owner.painted_color(),
            Some((Fill::VerticalGradient, Rgb(4, 5, 6)))
        );
        owner.taken();
        assert_eq!(owner.painted_color(), None);
    }
}
