//! When a frame drift earns a full-frame repaint.

use crate::services::descriptor;
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::demand::{DemandKey, Mode};

#[cfg(test)]
use crate::services::REPAINT_FLOOR_MS;

/// How many display steps a change-gated page waits before drift earns a repaint.
const ON_CHANGE_STRIDE: u64 = 4;

pub(super) fn repaint_due(
    page: PageId,
    prev: &FrameSnapshot,
    next: &FrameSnapshot,
    since_ms: u64,
    overlay: bool,
) -> bool {
    let step = descriptor(page).repaint_step_ms();
    match next.demand.claim(DemandKey::DISPLAY).mode {
        Mode::Off => return false,
        Mode::OnChange => return since_ms >= step * ON_CHANGE_STRIDE && prev != next,
        Mode::Continuous => {}
    }
    // Every page that prints a word has to be redrawn when the words change, including the
    // ones whose own snapshot did not move.
    if prev.lang != next.lang {
        return true;
    }
    if since_ms < step {
        // The frame that ends an animation is drawn even inside the floor: the glide's last
        // offset would otherwise never reach the glass.
        let settled = prev.page == next.page && prev.animating() && !next.animating();
        if !settled {
            return false;
        }
    }
    descriptor(page).repaints_on(prev, next, overlay)
}

/// The same question with the floor taken out: a frame this answers `true` for is one the page
/// wants drawn, so a screen holding it back must keep the loop awake. The diff offers each frame
/// once, so waiting for the next one is waiting for nothing.
pub(super) fn repaint_wanted(
    page: PageId,
    prev: &FrameSnapshot,
    next: &FrameSnapshot,
    overlay: bool,
) -> bool {
    match next.demand.claim(DemandKey::DISPLAY).mode {
        Mode::Off => false,
        Mode::OnChange => prev != next,
        Mode::Continuous => {
            prev.lang != next.lang || descriptor(page).repaints_on(prev, next, overlay)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AudioPhase, PlaybackPhase};
    use iot_core::drivers::audio::AudioEnvelope;
    use iot_core::drivers::network::{AccessPoint, ApList, Ipv4, LinkPhase, Ssid};
    use iot_core::drivers::playback::Sound;

    fn live_envelope() -> AudioEnvelope {
        let mut envelope = AudioEnvelope::default();
        let mut samples = [0_i16; iot_core::drivers::audio::SAMPLES_PER_COLUMN as usize];
        samples[0] = 0x7F;
        envelope.push(&samples);
        envelope
    }

    /// The Audio page sets its own step, so the tests follow the table rather than the
    /// default — a page that changed its cadence without changing this would fail here.
    fn audio_step() -> u64 {
        descriptor(PageId::Audio).repaint_step_ms()
    }

    #[test]
    fn a_language_switch_repaints_a_page_that_only_prints_words() {
        let base = FrameSnapshot::default();
        let mut moved = base;
        moved.lang = crate::shared::kit::i18n::Lang::ZhHans;
        assert!(repaint_due(PageId::Env, &base, &moved, 0, true));
        assert!(repaint_due(PageId::Speaker, &base, &moved, 0, true));
    }

    #[test]
    fn frame_repaint_waits_out_the_step() {
        let base = FrameSnapshot::default();
        let mut moved = base;
        moved.audio.phase = AudioPhase::Recording;
        assert!(!repaint_due(
            PageId::Audio,
            &base,
            &moved,
            audio_step() - 1,
            false
        ));
        assert!(repaint_due(
            PageId::Audio,
            &base,
            &moved,
            audio_step(),
            false
        ));
    }

    #[test]
    fn frame_repaint_ignores_the_wall_time_counter() {
        let base = FrameSnapshot::default();
        let mut ticked = base;
        ticked.audio.elapsed_ms = 1_234;
        assert!(!repaint_due(
            PageId::Audio,
            &base,
            &ticked,
            audio_step(),
            false
        ));
    }

    #[test]
    fn frame_repaint_audio_tracks_everything_else_it_draws() {
        let base = FrameSnapshot::default();
        let mut phase = base;
        phase.audio.phase = AudioPhase::Recording;
        let mut envelope = base;
        envelope.audio.envelope = live_envelope();
        let mut restarts = base;
        restarts.audio.restarts = 3;
        let mut level = base;
        level.audio.dba_lsb = 0x8000;
        for next in [phase, envelope, restarts, level] {
            assert!(
                repaint_due(PageId::Audio, &base, &next, audio_step(), false),
                "an Audio drift the page draws must repaint"
            );
        }
    }

    #[test]
    fn frame_repaint_attitude_follows_every_snapshot() {
        let base = FrameSnapshot::default();
        let mut ticked = base;
        ticked.audio.elapsed_ms = 10;
        assert!(!repaint_due(
            PageId::Attitude,
            &base,
            &ticked,
            REPAINT_FLOOR_MS - 1,
            false
        ));
        assert!(repaint_due(
            PageId::Attitude,
            &base,
            &ticked,
            REPAINT_FLOOR_MS,
            false
        ));
    }

    #[test]
    fn frame_repaint_input_only_for_the_overlay() {
        let base = FrameSnapshot::default();
        let mut ticked = base;
        ticked.audio.phase = AudioPhase::Recording;
        assert!(!repaint_due(
            PageId::Input,
            &base,
            &ticked,
            REPAINT_FLOOR_MS,
            false
        ));
        assert!(repaint_due(
            PageId::Input,
            &base,
            &ticked,
            REPAINT_FLOOR_MS,
            true
        ));
    }

    #[test]
    fn frame_repaint_speaker_tracks_the_row_it_draws() {
        let base = FrameSnapshot::default();
        let mut playing = base;
        playing.playback.phase = PlaybackPhase::Playing;
        let mut source = base;
        source.playback.sound = Sound::Rise;
        let mut muted = base;
        muted.playback.muted = true;
        let mut played = base;
        played.playback.plays = 7;
        let mut dropped = base;
        dropped.playback.dropped = 2;
        for next in [playing, source, muted, played, dropped] {
            assert!(
                repaint_due(PageId::Speaker, &base, &next, REPAINT_FLOOR_MS, false),
                "a playback drift the Speaker page draws must repaint even with the overlay off"
            );
        }
    }

    #[test]
    fn frame_repaint_speaker_ignores_a_drift_it_does_not_draw() {
        let base = FrameSnapshot::default();
        let mut drifting = base;
        drifting.audio.envelope = live_envelope();
        for overlay in [false, true] {
            assert!(
                !repaint_due(PageId::Speaker, &base, &drifting, REPAINT_FLOOR_MS, overlay),
                "the Speaker page draws no envelope, so the overlay flag must not re-open it"
            );
        }
    }

    #[test]
    fn frame_repaint_network_tracks_the_link_and_the_list() {
        let base = FrameSnapshot::default();
        let mut associated = base;
        associated.link.phase = LinkPhase::Connecting;
        associated.link.ap = Some(AccessPoint {
            ssid: Ssid::from_octets(b"neighbour"),
            bssid: [1, 2, 3, 4, 5, 6],
            channel: 11,
            rssi: -55,
            secured: true,
        });
        let mut addressed = base;
        addressed.link.address = Some(Ipv4::from_octets([192, 168, 1, 136]));
        let mut scanned = base;
        scanned.aps = Some(ApList::from_ranked(&[AccessPoint::default()]));
        for next in [associated, addressed, scanned] {
            assert!(
                repaint_due(PageId::Network, &base, &next, REPAINT_FLOOR_MS, false),
                "a link or list drift the Network page draws must repaint even with the \
                 overlay off"
            );
        }
    }

    #[test]
    fn frame_repaint_network_takes_the_board_address_the_first_time_it_arrives() {
        let base = FrameSnapshot::default();
        let mut addressed = base;
        addressed.mac = Some([0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef]);
        assert!(repaint_due(
            PageId::Network,
            &base,
            &addressed,
            REPAINT_FLOOR_MS,
            false
        ));
    }

    #[test]
    fn frame_repaint_network_ignores_a_drift_it_does_not_draw() {
        let base = FrameSnapshot::default();
        let mut drifting = base;
        drifting.audio.envelope = live_envelope();
        drifting.touch.taps = 12;
        for overlay in [false, true] {
            assert!(
                !repaint_due(PageId::Network, &base, &drifting, REPAINT_FLOOR_MS, overlay),
                "the Network page draws no envelope and no counter"
            );
        }
    }

    #[test]
    fn an_unlit_screen_never_repaints_and_a_dim_one_only_on_a_change() {
        let base = FrameSnapshot::default();
        assert_eq!(base.demand.claim(DemandKey::DISPLAY).mode, Mode::Continuous);

        let mut unlit = base;
        unlit.demand.set(
            DemandKey::DISPLAY,
            iot_core::demand::Claim::new(Mode::Off, 0, iot_core::demand::Priority::Primary),
        );
        let mut wobble = unlit;
        wobble.audio.phase = AudioPhase::Recording;

        assert!(
            !repaint_due(PageId::Audio, &unlit, &wobble, 10 * REPAINT_FLOOR_MS, true),
            "a screen nobody is looking at does not get frames no matter what drifts"
        );

        let mut dim = base;
        dim.demand.set(
            DemandKey::DISPLAY,
            iot_core::demand::Claim::new(Mode::OnChange, 0, iot_core::demand::Priority::Primary),
        );
        assert!(
            !repaint_due(PageId::Audio, &dim, &dim, 10 * REPAINT_FLOOR_MS, false),
            "nothing changed, so there is nothing to redraw"
        );

        let mut dim_moved = dim;
        dim_moved.audio.phase = AudioPhase::Recording;
        assert!(
            !repaint_due(PageId::Audio, &dim, &dim_moved, audio_step(), false),
            "a dim screen waits out the base cadence before it spends a frame"
        );
        assert!(repaint_due(
            PageId::Audio,
            &dim,
            &dim_moved,
            audio_step() * 4,
            false
        ));
    }

    #[test]
    fn only_audio_raises_the_step_and_only_text_lowers_it() {
        for page in PageId::ALL {
            let step = descriptor(page).repaint_step_ms();
            match page {
                PageId::Audio => assert!(step > REPAINT_FLOOR_MS, "{page:?} step"),
                PageId::Text | PageId::Menu | PageId::Widgets => assert!(
                    step < REPAINT_FLOOR_MS,
                    "{page:?} must keep up with a drag, not the idle floor"
                ),
                _ => assert_eq!(step, REPAINT_FLOOR_MS, "{page:?} step"),
            }
        }
    }
}
