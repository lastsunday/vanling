//! When a frame drift earns a full-frame repaint.

use crate::services::descriptor;
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::demand::{DemandKey, Mode};

#[cfg(test)]
use crate::services::REPAINT_FLOOR_MS;

pub fn frame_repaint_due(
    page: PageId,
    prev: &FrameSnapshot,
    next: &FrameSnapshot,
    since_ms: u64,
    overlay: bool,
) -> bool {
    let step = descriptor(page).repaint_step_ms();
    match next.demand.claim(DemandKey::DISPLAY).mode {
        Mode::Off => return false,
        Mode::OnChange => return since_ms >= step * 4 && prev != next,
        Mode::Continuous => {}
    }
    if since_ms < step {
        return false;
    }
    descriptor(page).repaints_on(prev, next, overlay)
}

/// Repaint rate (frames per second) over one window, `0` for an empty or degenerate window.
pub fn windowed_rate(frames: u32, elapsed_ms: u64) -> u8 {
    if elapsed_ms == 0 {
        return 0;
    }
    u64::from(frames)
        .saturating_mul(1_000)
        .checked_div(elapsed_ms)
        .and_then(|rate| u8::try_from(rate).ok())
        .unwrap_or(u8::MAX)
}

/// A share of a window as a whole percent, or `0` for a window nothing was measured in.
pub fn windowed_percent(part_us: u64, window_us: u64) -> u32 {
    part_us
        .saturating_mul(100)
        .checked_div(window_us)
        .and_then(|pct| u32::try_from(pct).ok())
        .unwrap_or(100)
        .min(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AudioPhase, PlaybackPhase};
    use iot_core::drivers::audio::AudioEnvelope;
    use iot_core::drivers::network::{AccessPoint, ApList, Ipv4, LinkPhase, Ssid};
    use iot_core::drivers::playback::Sound;

    #[test]
    fn a_share_is_reported_whole_and_never_over_a_hundred() {
        assert_eq!(windowed_percent(500_000, 2_000_000), 25);
        assert_eq!(windowed_percent(2_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(1, 1_000_000), 0);
        assert_eq!(windowed_percent(4_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(7_000, 0), 100);
    }

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
    fn diag_repaint_waits_out_the_step() {
        let base = FrameSnapshot::default();
        let mut moved = base;
        moved.audio.phase = AudioPhase::Recording;
        assert!(!frame_repaint_due(
            PageId::Audio,
            &base,
            &moved,
            audio_step() - 1,
            false
        ));
        assert!(frame_repaint_due(
            PageId::Audio,
            &base,
            &moved,
            audio_step(),
            false
        ));
    }

    #[test]
    fn diag_repaint_ignores_the_wall_time_counter() {
        let base = FrameSnapshot::default();
        let mut ticked = base;
        ticked.audio.elapsed_ms = 1_234;
        assert!(!frame_repaint_due(
            PageId::Audio,
            &base,
            &ticked,
            audio_step(),
            false
        ));
    }

    #[test]
    fn diag_repaint_audio_tracks_everything_else_it_draws() {
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
                frame_repaint_due(PageId::Audio, &base, &next, audio_step(), false),
                "an Audio drift the page draws must repaint"
            );
        }
    }

    #[test]
    fn diag_repaint_attitude_follows_every_snapshot() {
        let base = FrameSnapshot::default();
        let mut ticked = base;
        ticked.audio.elapsed_ms = 10;
        assert!(!frame_repaint_due(
            PageId::Attitude,
            &base,
            &ticked,
            REPAINT_FLOOR_MS - 1,
            false
        ));
        assert!(frame_repaint_due(
            PageId::Attitude,
            &base,
            &ticked,
            REPAINT_FLOOR_MS,
            false
        ));
    }

    #[test]
    fn diag_repaint_ambient_only_for_the_overlay() {
        let base = FrameSnapshot::default();
        let mut ticked = base;
        ticked.audio.phase = AudioPhase::Recording;
        assert!(!frame_repaint_due(
            PageId::Ambient,
            &base,
            &ticked,
            REPAINT_FLOOR_MS,
            false
        ));
        assert!(frame_repaint_due(
            PageId::Ambient,
            &base,
            &ticked,
            REPAINT_FLOOR_MS,
            true
        ));
    }

    #[test]
    fn diag_repaint_speaker_tracks_the_row_it_draws() {
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
                frame_repaint_due(PageId::Speaker, &base, &next, REPAINT_FLOOR_MS, false),
                "a playback drift the Speaker page draws must repaint even with the overlay off"
            );
        }
    }

    #[test]
    fn diag_repaint_speaker_ignores_a_drift_it_does_not_draw() {
        let base = FrameSnapshot::default();
        let mut drifting = base;
        drifting.audio.envelope = live_envelope();
        for overlay in [false, true] {
            assert!(
                !frame_repaint_due(PageId::Speaker, &base, &drifting, REPAINT_FLOOR_MS, overlay),
                "the Speaker page draws no envelope, so the overlay flag must not re-open it"
            );
        }
    }

    #[test]
    fn diag_repaint_network_tracks_the_link_and_the_list() {
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
                frame_repaint_due(PageId::Network, &base, &next, REPAINT_FLOOR_MS, false),
                "a link or list drift the Network page draws must repaint even with the \
                 overlay off"
            );
        }
    }

    #[test]
    fn diag_repaint_network_takes_the_board_address_the_first_time_it_arrives() {
        let base = FrameSnapshot::default();
        let mut addressed = base;
        addressed.mac = Some([0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef]);
        assert!(frame_repaint_due(
            PageId::Network,
            &base,
            &addressed,
            REPAINT_FLOOR_MS,
            false
        ));
    }

    #[test]
    fn diag_repaint_network_ignores_a_drift_it_does_not_draw() {
        let base = FrameSnapshot::default();
        let mut drifting = base;
        drifting.audio.envelope = live_envelope();
        drifting.touch.taps = 12;
        for overlay in [false, true] {
            assert!(
                !frame_repaint_due(PageId::Network, &base, &drifting, REPAINT_FLOOR_MS, overlay),
                "the Network page draws no envelope and no counter"
            );
        }
    }

    #[test]
    fn rate_scales_frames_over_the_window() {
        assert_eq!(windowed_rate(13, 250), 52);
        assert_eq!(windowed_rate(0, 250), 0);
        assert_eq!(windowed_rate(5, 0), 0);
    }

    #[test]
    fn overheated_rate_saturates_at_max() {
        assert_eq!(windowed_rate(1_000, 1_000), u8::MAX);
        assert_eq!(windowed_rate(u32::MAX, 1), u8::MAX);
    }

    #[test]
    fn an_unlit_panel_never_repaints_and_a_dim_one_only_on_a_change() {
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
            !frame_repaint_due(PageId::Audio, &unlit, &wobble, 10 * REPAINT_FLOOR_MS, true),
            "a panel nobody is looking at does not get frames no matter what drifts"
        );

        let mut dim = base;
        dim.demand.set(
            DemandKey::DISPLAY,
            iot_core::demand::Claim::new(Mode::OnChange, 0, iot_core::demand::Priority::Primary),
        );
        assert!(
            !frame_repaint_due(PageId::Audio, &dim, &dim, 10 * REPAINT_FLOOR_MS, false),
            "nothing changed, so there is nothing to redraw"
        );

        let mut dim_moved = dim;
        dim_moved.audio.phase = AudioPhase::Recording;
        assert!(
            !frame_repaint_due(PageId::Audio, &dim, &dim_moved, audio_step(), false),
            "a dim panel waits out the base cadence before it spends a frame"
        );
        assert!(frame_repaint_due(
            PageId::Audio,
            &dim,
            &dim_moved,
            audio_step() * 4,
            false
        ));
    }
}

#[test]
fn only_the_audio_page_raises_the_step() {
    for page in PageId::ALL {
        assert!(
            descriptor(page).repaint_step_ms() >= REPAINT_FLOOR_MS,
            "{page:?} cannot repaint faster than the floor"
        );
        assert_eq!(
            descriptor(page).repaint_step_ms() > REPAINT_FLOOR_MS,
            page == PageId::Audio,
            "{page:?} step"
        );
    }
}
