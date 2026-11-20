use crate::shared::intent::AudioIntent;
use iot_core::drivers::audio::{AudioPhase, AudioSample};

use super::Health;

/// Capture state behind the Audio page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioState {
    /// What the page asked for: the capture that runs, and the one that was stopped.
    pub phase: AudioPhase,
    /// Whether the capture part is answering. The page stays; the poll retries the part.
    pub health: Health,
    /// What the page draws: the samples behind the phase.
    pub view: AudioView,
}

/// The samples the Audio page draws. Nothing outside the page's own drawing reads them, so
/// they are kept apart from the phase the capture logic decides on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioView {
    /// The most recent capture poll, always current even while stopped — the driver keeps
    /// running underneath, so resuming picks the sweep up wherever it has moved to rather
    /// than where the previous capture left it.
    pub live: AudioSample,
    /// The poll latched when a capture stopped, so a stopped capture keeps showing what it
    /// caught instead of following the still-running scope.
    pub captured: AudioSample,
}

impl Default for AudioState {
    fn default() -> Self {
        Self::boot()
    }
}

impl AudioState {
    /// The state a board boots into: idle, with a blank envelope on both sides so the Audio
    /// page draws a flat line until a capture runs.
    pub const BOOT: Self = Self {
        phase: AudioPhase::Idle,
        health: Health::Ready,
        view: AudioView {
            live: AudioSample::ZERO,
            captured: AudioSample::ZERO,
        },
    };

    pub const fn boot() -> Self {
        Self::BOOT
    }

    /// The Audio page's one transition, refused when the board wired no capture part.
    pub(crate) fn apply(&mut self, intent: AudioIntent, wired: bool) {
        if !wired {
            return;
        }
        match (intent, self.phase) {
            (AudioIntent::Start, AudioPhase::Idle | AudioPhase::Stopped) => {
                self.phase = AudioPhase::Recording;
            }
            (AudioIntent::Stop, AudioPhase::Recording) => {
                self.view.captured = self.view.live;
                self.phase = AudioPhase::Stopped;
            }
            _ => {}
        }
    }

    /// A capture poll: the part's health and the live envelope, refused when nothing is wired.
    pub(crate) fn observe(&mut self, sample: AudioSample, wired: bool) {
        if !wired {
            return;
        }
        self.health = match sample.fault {
            Some(reason) => Health::Down(reason),
            None => Health::Ready,
        };
        self.view.live = sample;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::intent::*;
    use crate::state::test_support::*;
    use iot_core::drivers::audio::{AudioEnvelope, SAMPLES_PER_COLUMN};
    use iot_core::drivers::motion::MotionCapabilities;

    const ALL_CAPS: MotionCapabilities = MotionCapabilities::TAP;

    fn loud(amplitude: i16) -> AudioSample {
        let mut samples = [0_i16; SAMPLES_PER_COLUMN as usize];
        samples[SAMPLES_PER_COLUMN as usize / 2] = amplitude;
        let mut envelope = AudioEnvelope::default();
        envelope.push(&samples);
        AudioSample {
            envelope,
            elapsed_ms: 7,
            restarts: 0,
            dba_lsb: 0,
            fault: None,
        }
    }

    fn apply(manager: &mut DeviceManager, operation: InputIntent) {
        manager.apply_intent(Intent::Input(operation));
    }

    fn on_audio_page() -> DeviceManager {
        let manager = landed(DeviceManager::boot(Boot {
            backed: Backed {
                motion: Some(ALL_CAPS),
                audio: true,
                ..Backed::NONE
            },
            boot_page: PageId::Audio,
            ..Default::default()
        }));
        assert_eq!(manager.state().page(), PageId::Audio);
        manager
    }

    fn tap_in_place(manager: &mut DeviceManager) {
        let page = manager.state().page();
        apply(manager, tap());
        assert_eq!(
            manager.state().page(),
            page,
            "a tap on audio records instead of paging"
        );
    }

    #[test]
    fn a_capture_that_lost_its_codec_reports_it_and_clears_it_when_it_returns() {
        let mut manager = on_audio_page();
        let mut lost = loud(1_000);
        lost.fault = Some("the codec did not answer on I2C");
        manager.apply_capture(lost);
        assert_eq!(
            manager.state().audio_snapshot().health,
            Health::Down("the codec did not answer on I2C"),
            "a fault reaches the page instead of dropping the part"
        );
        manager.apply_capture(loud(1_000));
        assert_eq!(
            manager.state().audio_snapshot().health,
            Health::Ready,
            "the next clean sample clears it, so the page can leave NO IN"
        );
    }

    #[test]
    fn a_capture_poll_is_kept_only_when_audio_is_enabled() {
        let mut disabled = DeviceManager::new();
        disabled.apply_capture(loud(8_000));
        assert_eq!(disabled.state().audio.view.live.envelope.committed(), 0);

        let mut enabled = DeviceManager::boot(Boot {
            backed: Backed {
                audio: true,
                ..Backed::NONE
            },
            ..Default::default()
        });
        enabled.apply_capture(loud(8_000));
        assert_eq!(enabled.state().audio.view.live.envelope.committed(), 1);
        assert_eq!(
            enabled.state().audio.phase,
            AudioPhase::Idle,
            "not captured yet"
        );
    }

    #[test]
    fn a_tap_starts_and_stops_a_capture() {
        let mut manager = on_audio_page();
        tap_in_place(&mut manager);
        assert_eq!(manager.state().audio.phase, AudioPhase::Recording);

        manager.apply_capture(loud(8_000));
        tap_in_place(&mut manager);
        assert_eq!(manager.state().audio.phase, AudioPhase::Stopped);
        assert_eq!(
            manager.state().audio.view.captured.envelope.committed(),
            1,
            "the poll on record is latched for the panel"
        );
    }

    #[test]
    fn a_stopped_capture_keeps_what_it_caught() {
        let mut manager = on_audio_page();
        tap_in_place(&mut manager);
        manager.apply_capture(loud(8_000));
        tap_in_place(&mut manager);
        let stopped = manager.state().audio.view.captured;

        let polled_on = loud(20_000);
        manager.apply_capture(polled_on);
        assert_eq!(
            manager.state().audio.view.captured,
            stopped,
            "the driver keeps polling but a stopped capture is frozen"
        );
        assert_eq!(
            manager.state().audio.view.live,
            polled_on,
            "live is the driver's latest poll, so resuming picks the scope up"
        );
    }

    #[test]
    fn a_tap_records_without_leaving_the_audio_page() {
        let mut manager = on_audio_page();
        apply(&mut manager, tap());
        assert_eq!(manager.state().page(), PageId::Audio, "the page holds");
        assert_eq!(manager.state().audio.phase, AudioPhase::Recording);
    }

    #[test]
    fn a_tap_off_the_audio_page_is_a_color_move_and_never_records() {
        let mut manager = on_audio_page();
        let before = manager.state().lights[0];

        for _ in 0..3 {
            manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
        }
        assert_eq!(manager.state().page(), PageId::Ambient, "walked off audio");

        apply(&mut manager, tap());
        assert_eq!(
            manager.state().audio.phase,
            AudioPhase::Idle,
            "never records"
        );
        assert_ne!(
            manager.state().lights[0],
            before,
            "the tap moved the color instead"
        );
    }
}
