//! What the render layer needs from a camera, and nothing about how one is wired.

use alloc::boxed::Box;

use crate::paint::Rgb;

/// What one call to [`FrameSource::advance`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAdvance {
    Fresh,
    Same,
    Stalled,
}

/// Running totals, so a readout can say whether the picture is advancing and whether it is
/// arriving by the chain rebuilding itself.
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
pub trait CameraTarget {
    fn paint_camera(&mut self, now_ms: u64) -> bool;

    fn attach_camera(&mut self, camera: Box<dyn FrameSource>);
}

/// Who owns the frame buffer a surface and a camera share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOwner {
    NeedsPaint,
    Painted { color: Rgb },
    Camera,
}

impl FrameOwner {
    /// Whether painting this colour has any work to do.
    pub fn accepts_color(&self, color: Rgb) -> bool {
        match *self {
            FrameOwner::Camera => false,
            FrameOwner::Painted { color: c } => c != color,
            FrameOwner::NeedsPaint => true,
        }
    }

    /// Whether the camera owns the buffer, and so whether the surface must park it before
    /// it writes anything of its own.
    pub fn is_camera(&self) -> bool {
        matches!(self, FrameOwner::Camera)
    }

    /// Records that the CPU has just painted.
    pub fn painted(&mut self, color: Rgb) {
        *self = FrameOwner::Painted { color };
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
    pub fn painted_color(&self) -> Option<Rgb> {
        match *self {
            FrameOwner::Painted { color } => Some(color),
            FrameOwner::NeedsPaint | FrameOwner::Camera => None,
        }
    }
}

/// A camera the render layer can draw from.
pub trait FrameSource {
    fn advance(&mut self, frame: &mut [u8], now_ms: u64) -> FrameAdvance;

    fn pause(&mut self);

    fn resume(&mut self, frame: &mut [u8]);

    fn counters(&self) -> CameraCounters;

    fn geometry(&mut self) -> Option<WindowGeometry>;
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut owner = FrameOwner::NeedsPaint;
        owner.taken();
        assert!(!owner.accepts_color(Rgb(1, 2, 3)));
        owner.released();
        assert!(
            owner.accepts_color(Rgb(1, 2, 3)),
            "a fill after the camera released the buffer has to repaint, whatever it is"
        );
    }

    #[test]
    fn a_painted_colour_is_only_repeated_at_its_own_value() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Rgb(10, 20, 30));
        assert!(!owner.accepts_color(Rgb(10, 20, 30)));
        assert!(owner.accepts_color(Rgb(10, 20, 31)));
    }

    #[test]
    fn a_camera_refuses_fills_however_far_the_colour_moved() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Rgb(1, 1, 1));
        owner.taken();
        for level in [0, 127, 255] {
            assert!(
                !owner.accepts_color(Rgb(level, level, level)),
                "the light state moves on the camera's page; the buffer does not"
            );
        }
    }

    #[test]
    fn the_colour_a_camera_displaced_is_not_remembered() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Rgb(7, 8, 9));
        owner.taken();
        owner.released();
        assert_eq!(owner.painted_color(), None);
    }

    #[test]
    fn a_repaint_of_an_unchanged_page_reuses_the_colour_it_held() {
        let mut owner = FrameOwner::NeedsPaint;
        owner.painted(Rgb(4, 5, 6));
        assert_eq!(owner.painted_color(), Some(Rgb(4, 5, 6)));
        owner.taken();
        assert_eq!(owner.painted_color(), None);
    }
}
