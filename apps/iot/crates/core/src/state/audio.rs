use super::*;

use crate::diagnostics::AudioDiagnostics;
use crate::intent::CaptureAction;

/// Capture phase the Audio page drives. The capture driver never stops — it
/// keeps folding samples into its envelope — so a phase only decides which
/// envelope the panel draws and whether the sweep follows the live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioPhase {
    /// Nothing captured yet: the panel draws no sweep, only the tap hint.
    #[default]
    Idle,
    /// Capturing: the panel follows the live envelope.
    Recording,
    /// A capture that ran and stopped: the panel shows the latched envelope.
    Stopped,
}

/// Capture state behind the Audio page. The envelopes are summaries from the
/// driver, never samples, so this stays small enough to sit in the state and
/// its derived diagnostics snapshot alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioState {
    pub phase: AudioPhase,
    /// The most recent capture poll, always current even while stopped — the
    /// driver keeps running underneath, so resuming picks the sweep up wherever
    /// it has moved to rather than where the previous capture left it.
    pub live: AudioSample,
    /// The poll latched when a capture stopped, so a stopped capture keeps
    /// showing what it caught instead of following the still-running scope.
    pub captured: AudioSample,
}

impl Default for AudioState {
    fn default() -> Self {
        Self::boot()
    }
}

impl AudioState {
    /// The state a board boots into: idle, with a blank envelope on both sides
    /// so the Audio page draws a flat line until a capture runs.
    pub const BOOT: Self = Self {
        phase: AudioPhase::Idle,
        live: AudioSample {
            envelope: AudioEnvelope::ZERO,
            elapsed_ms: 0,
            restarts: 0,
            dba_lsb: 0,
        },
        captured: AudioSample {
            envelope: AudioEnvelope::ZERO,
            elapsed_ms: 0,
            restarts: 0,
            dba_lsb: 0,
        },
    };

    pub const fn boot() -> Self {
        Self::BOOT
    }
}

impl DeviceManager {
    pub(super) fn apply_capture_intent(&mut self, action: CaptureAction) {
        if !self.state.audio_enabled {
            return;
        }
        match (action, self.state.audio.phase) {
            (CaptureAction::Start, AudioPhase::Idle | AudioPhase::Stopped) => {
                self.state.audio.phase = AudioPhase::Recording;
            }
            (CaptureAction::Stop, AudioPhase::Recording) => {
                self.state.audio.captured = self.state.audio.live;
                self.state.audio.phase = AudioPhase::Stopped;
            }
            _ => {}
        }
    }
}

impl DeviceState {
    pub(crate) fn audio_diagnostics(&self) -> AudioDiagnostics {
        let shown = match self.audio.phase {
            AudioPhase::Stopped => &self.audio.captured,
            AudioPhase::Recording | AudioPhase::Idle => &self.audio.live,
        };
        AudioDiagnostics {
            phase: self.audio.phase,
            envelope: shown.envelope,
            elapsed_ms: shown.elapsed_ms,
            restarts: shown.restarts,
            dba_lsb: shown.dba_lsb,
        }
    }
}

impl DeviceManager {
    pub fn apply_capture(&mut self, sample: AudioSample) {
        if self.state.audio_enabled {
            self.state.audio.live = sample;
        }
    }
}

#[cfg(test)]
mod audio_page {
    use super::*;
    use crate::state::test_support::*;

    use crate::drivers::audio::SAMPLES_PER_COLUMN;
    use crate::drivers::motion::MotionCapabilities;

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
        }
    }

    fn apply(manager: &mut DeviceManager, operation: OperationIntent) {
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    fn on_audio_page() -> DeviceManager {
        let mut manager = landed(DeviceManager::with_motion(true, ALL_CAPS).with_audio(true));
        apply(&mut manager, triple_click());
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Audio);
        manager
    }

    fn tap_in_place(manager: &mut DeviceManager) {
        let page = manager.state().page;
        apply(manager, tap());
        assert_eq!(
            manager.state().page,
            page,
            "a tap on audio records instead of paging"
        );
    }

    #[test]
    fn the_audio_page_only_appears_on_a_board_that_has_one() {
        assert_eq!(
            DisplayPage::Ambient.next_page(true, false, false, false, false),
            DisplayPage::Attitude
        );
        assert_eq!(
            DisplayPage::Attitude.next_page(true, false, false, false, false),
            DisplayPage::Ambient,
            "a board without audio skips the page"
        );
        assert_eq!(
            DisplayPage::Ambient.next_page(false, true, false, false, false),
            DisplayPage::Audio
        );
        assert_eq!(
            DisplayPage::Attitude.next_page(true, true, false, false, false),
            DisplayPage::Audio
        );
    }

    #[test]
    fn the_page_switch_reaches_the_audio_page_and_back() {
        let mut manager = landed(DeviceManager::new().with_audio(true));
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Audio);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn the_camera_page_only_appears_on_a_board_that_has_one() {
        assert_eq!(
            DisplayPage::Ambient.next_page(false, false, false, true, false),
            DisplayPage::Camera
        );
        assert_eq!(
            DisplayPage::Ambient.next_page(false, false, false, false, false),
            DisplayPage::Ambient,
            "a board without a camera never reaches the page"
        );
        assert_eq!(
            DisplayPage::Audio.next_page(false, false, false, true, false),
            DisplayPage::Camera,
            "the camera follows the pages that need a wired source"
        );
        assert_eq!(
            DisplayPage::Audio.next_page(false, false, false, false, false),
            DisplayPage::Ambient,
            "and is skipped when there is none"
        );
    }

    #[test]
    fn leaving_the_last_page_wraps_to_the_start_of_the_cycle() {
        assert_eq!(
            DisplayPage::Network.next_page(true, false, false, false, true),
            DisplayPage::Ambient,
            "a board that lost the link mid-cycle wraps rather than jumping ahead"
        );
        assert_eq!(
            DisplayPage::Network.next_page(false, false, false, false, true),
            DisplayPage::Ambient,
            "and a link is the only capability, so back to Ambient"
        );
        assert_eq!(
            DisplayPage::Camera.next_page(false, false, false, true, true),
            DisplayPage::Network
        );
        assert_eq!(
            DisplayPage::Camera.next_page(true, false, false, false, false),
            DisplayPage::Ambient,
            "a board that lost the camera mid-cycle wraps rather than jumping ahead"
        );
        assert_eq!(
            DisplayPage::Camera.next_page(false, true, false, false, false),
            DisplayPage::Ambient
        );
        assert_eq!(
            DisplayPage::Camera.next_page(false, false, false, false, false),
            DisplayPage::Ambient,
            "and a camera is the only capability, so back to Ambient"
        );
    }

    #[test]
    fn every_page_the_board_can_back_is_shown_on_the_way_round() {
        for (motion, audio, playback, camera, network) in [
            (false, false, false, false, false),
            (true, false, false, false, false),
            (false, true, false, false, false),
            (false, false, true, false, false),
            (false, false, false, true, false),
            (false, false, false, false, true),
            (true, true, true, true, true),
            (false, true, true, true, true),
            (true, false, false, true, true),
            (true, true, true, true, false),
        ] {
            let backed: Vec<DisplayPage> = CYCLE
                .iter()
                .copied()
                .filter(|page| page.is_backed(motion, audio, playback, camera, network))
                .collect();
            assert!(
                !backed.is_empty(),
                "Ambient needs no capability, so there is always somewhere to go"
            );
            for start in CYCLE {
                let start_is_backed = backed.contains(&start);
                let mut seen = Vec::new();
                let mut page = start;
                for _ in 0..backed.len() {
                    page = page.next_page(motion, audio, playback, camera, network);
                    seen.push(page);
                }
                let mut expected = backed.clone();
                let mut walked = seen.clone();
                expected.sort_by_key(|page| page.index());
                walked.sort_by_key(|page| page.index());
                assert_eq!(
                    walked, expected,
                    "a full turn from {start:?} did not visit each backed page once \
                     (motion={motion} audio={audio} playback={playback} camera={camera} \
                     network={network})"
                );
                if start_is_backed {
                    assert_eq!(
                        page, start,
                        "and a full turn from a page the board backs comes back to it, so \
                         the toggle is a cycle rather than a drift"
                    );
                } else {
                    assert!(
                        backed.contains(&seen[0]),
                        "one tap off {start:?} enters the cycle rather than staying put"
                    );
                }
            }
            if backed.len() == 1 {
                assert_eq!(
                    backed[0].next_page(motion, audio, playback, camera, network),
                    backed[0],
                    "a board that backs one page has nowhere to go and stays"
                );
            }
        }
    }

    #[test]
    fn the_boot_page_joins_the_cycle_it_was_asked_for() {
        let mut manager = landed(
            DeviceManager::with_motion(true, ALL_CAPS)
                .with_audio(true)
                .with_playback(true)
                .with_camera(true)
                .with_boot_page(DisplayPage::Camera),
        );
        assert_eq!(manager.state().page, DisplayPage::Camera);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Ambient);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Attitude);
    }

    #[test]
    fn the_boot_page_falls_back_for_every_capability_not_wired() {
        let bare = landed(DeviceManager::new());
        let default_page = bare.state().page;
        for page in CYCLE {
            let boot_page = landed(DeviceManager::new().with_boot_page(page))
                .state()
                .page;
            assert_eq!(
                boot_page, default_page,
                "{page:?} needs no capability, so a board with nothing wired still lands \
                 on the same page either way"
            );
        }
        let with_motion = landed(DeviceManager::with_motion(true, ALL_CAPS));
        assert_eq!(
            landed(
                DeviceManager::with_motion(true, ALL_CAPS).with_boot_page(DisplayPage::Attitude)
            )
            .state()
            .page,
            DisplayPage::Attitude,
            "the one capability it does have"
        );
        assert_eq!(
            with_motion.with_boot_page(DisplayPage::Camera).state().page,
            DisplayPage::Ambient,
            "and the one it does not"
        );
    }

    #[test]
    fn the_camera_page_cycles_from_the_board_and_back() {
        assert!(
            !DeviceManager::new().state().camera_enabled,
            "not declared until a board declares it"
        );
        let mut manager = landed(DeviceManager::new().with_camera(true));
        assert!(manager.state().camera_enabled);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Camera);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn a_board_without_a_camera_never_lands_on_its_page() {
        let mut manager = landed(DeviceManager::new().with_audio(true).with_playback(true));
        for _ in 0..8 {
            apply(&mut manager, triple_click());
            assert_ne!(
                manager.state().page,
                DisplayPage::Camera,
                "the camera page is gated on the wiring, not on how long you press"
            );
        }
    }

    #[test]
    fn a_capture_poll_is_kept_only_when_audio_is_enabled() {
        let mut disabled = DeviceManager::new();
        disabled.apply_capture(loud(8_000));
        assert_eq!(disabled.state().audio.live.envelope.committed(), 0);

        let mut enabled = DeviceManager::new().with_audio(true);
        enabled.apply_capture(loud(8_000));
        assert_eq!(enabled.state().audio.live.envelope.committed(), 1);
        assert_eq!(
            enabled.state().audio.phase,
            AudioPhase::Idle,
            "not captured yet"
        );
    }

    #[test]
    fn a_scan_list_is_kept_only_when_a_link_is_wired() {
        let list = ApList::from_ranked(&[AccessPoint {
            ssid: Ssid::from_octets(b"neighbour"),
            bssid: [1, 2, 3, 4, 5, 6],
            channel: 11,
            rssi: -55,
            secured: true,
        }]);

        let mut disabled = DeviceManager::new();
        disabled.apply_scan(list);
        assert_eq!(
            disabled.state().aps,
            None,
            "a build with no link has no page to draw a list on"
        );

        let mut enabled = DeviceManager::new().with_network(true);
        enabled.apply_scan(list);
        assert_eq!(enabled.state().aps, Some(list));
    }

    #[test]
    fn an_empty_scan_is_a_reading_not_an_absence() {
        let mut manager = DeviceManager::new().with_network(true);
        assert_eq!(manager.state().aps, None, "nothing has scanned yet");

        manager.apply_scan(ApList::empty());
        assert_eq!(
            manager.state().aps,
            Some(ApList::empty()),
            "a scan that found nothing is kept as an answer"
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
            manager.state().audio.captured.envelope.committed(),
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
        let stopped = manager.state().audio.captured;

        let polled_on = loud(20_000);
        manager.apply_capture(polled_on);
        assert_eq!(
            manager.state().audio.captured,
            stopped,
            "the driver keeps polling but a stopped capture is frozen"
        );
        assert_eq!(
            manager.state().audio.live,
            polled_on,
            "live is the driver's latest poll, so resuming picks the scope up"
        );
    }

    #[test]
    fn a_tap_records_without_leaving_the_audio_page() {
        let mut manager = on_audio_page();
        apply(&mut manager, tap());
        assert_eq!(manager.state().page, DisplayPage::Audio, "the page holds");
        assert_eq!(manager.state().audio.phase, AudioPhase::Recording);
    }

    #[test]
    fn a_tap_off_the_audio_page_is_a_color_move_and_never_records() {
        let mut manager = on_audio_page();
        let before = manager.state().lights[0];

        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(
            manager.state().page,
            DisplayPage::Ambient,
            "walked off audio"
        );

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
