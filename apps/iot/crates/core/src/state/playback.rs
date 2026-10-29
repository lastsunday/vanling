use super::*;

use crate::diagnostics::PlaybackDiagnostics;
use crate::intent::PlaybackAction;

/// Playback phase the Speaker page drives. A sound is a one-shot: the driver
/// reports it finished, and only then is the next tap heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackPhase {
    /// Nothing sounding: the next tap starts the page's sound.
    #[default]
    Idle,
    /// A sound is sounding. A tap in this phase is counted, not heard.
    Playing,
}

/// Playback state behind the Speaker page.
///
/// Small enough to sit in the state and its diagnostics snapshot alike: the
/// sounds themselves are the driver's, and this only records which one the page
/// holds, whether the output is latched quiet, and what the taps did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackState {
    pub phase: PlaybackPhase,
    /// The sound a tap plays, and the page's `SRC` row. Advances on every
    /// accepted tap, so the page shows what the *next* tap will play rather
    /// than echoing the last one.
    pub sound: Sound,
    /// The output latch. Held here rather than in the driver so a codec that
    /// resets itself cannot un-mute the speaker behind the panel's back.
    pub muted: bool,
    /// Sounds actually started; the panel's `PLY` row.
    pub plays: u16,
    /// Taps that arrived while a sound was sounding and were therefore not
    /// heard; the panel's second `PLY` figure, so a busy finger is visible as a
    /// number rather than as a sound that did not happen.
    pub dropped: u16,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::boot()
    }
}

impl PlaybackState {
    /// The state a board boots into: idle, on the first sound, audible. Mute
    /// starts off because a boot that is silent would read as a dead speaker,
    /// and the codec's own reset already leaves the DAC unmuted.
    pub const BOOT: Self = Self {
        phase: PlaybackPhase::Idle,
        sound: Sound::Tap,
        muted: false,
        plays: 0,
        dropped: 0,
    };

    pub const fn boot() -> Self {
        Self::BOOT
    }
}

impl DeviceManager {
    pub(super) fn apply_playback(&mut self, action: PlaybackAction) {
        if !self.state.playback_enabled {
            return;
        }
        match action {
            PlaybackAction::Next => match self.state.playback.phase {
                PlaybackPhase::Playing => {
                    self.state.playback.dropped = self.state.playback.dropped.saturating_add(1);
                }
                PlaybackPhase::Idle => {
                    self.state.playback.sound = self.state.playback.sound.next();
                    self.state.playback.phase = PlaybackPhase::Playing;
                    self.state.playback.plays = self.state.playback.plays.saturating_add(1);
                }
            },
            PlaybackAction::SetMuted(muted) => self.state.playback.muted = muted,
            PlaybackAction::Finished => {
                if self.state.playback.phase == PlaybackPhase::Playing {
                    self.state.playback.phase = PlaybackPhase::Idle;
                }
            }
        }
    }
}

impl DeviceState {
    pub(crate) fn playback_diagnostics(&self) -> PlaybackDiagnostics {
        PlaybackDiagnostics {
            phase: self.playback.phase,
            sound: self.playback.sound,
            muted: self.playback.muted,
            plays: self.playback.plays,
            dropped: self.playback.dropped,
        }
    }
}

impl DeviceManager {
    pub(super) fn ring(&mut self, sound: Sound) {
        if !self.state.playback_enabled || self.state.playback.phase != PlaybackPhase::Idle {
            return;
        }
        self.state.playback.sound = sound;
        self.state.playback.phase = PlaybackPhase::Playing;
        self.state.playback.plays = self.state.playback.plays.saturating_add(1);
    }
}

#[cfg(test)]
mod speaker_page {
    use super::*;
    use crate::state::test_support::*;

    use crate::drivers::playback::Sound;

    fn on_speaker() -> DeviceManager {
        let mut manager = landed(
            DeviceManager::with_motion(true, MotionCapabilities::EMPTY)
                .with_audio(true)
                .with_playback(true),
        );
        for _ in 0..=3 {
            if manager.state().page == DisplayPage::Speaker {
                break;
            }
            apply_page(&mut manager, triple_click());
        }
        assert_eq!(manager.state().page, DisplayPage::Speaker);
        manager
    }

    fn play(manager: &mut DeviceManager) {
        let before = manager.playback_state();
        apply_page(manager, tap());
        if before.phase == PlaybackPhase::Playing {
            assert_eq!(
                manager.playback_state().sound,
                before.sound,
                "a dropped tap must not walk the catalogue"
            );
        } else {
            assert_ne!(
                manager.playback_state().sound,
                before.sound,
                "a heard tap advances to the next sound"
            );
        }
    }

    fn mute(manager: &mut DeviceManager) {
        let before = manager.playback_state().muted;
        apply_page(manager, long_press());
        assert_ne!(
            manager.playback_state().muted,
            before,
            "the latch flips whichever way it stood"
        );
    }

    #[test]
    fn the_speaker_page_only_appears_on_a_board_that_has_one() {
        assert_eq!(
            DisplayPage::Ambient.next_page(true, false, true, false, false),
            DisplayPage::Attitude
        );
        assert_eq!(
            DisplayPage::Audio.next_page(true, true, true, false, false),
            DisplayPage::Speaker
        );
        assert_eq!(
            DisplayPage::Audio.next_page(true, true, false, false, false),
            DisplayPage::Ambient,
            "a board without a speaker skips the page"
        );
        assert_eq!(
            DisplayPage::Ambient.next_page(false, false, true, false, false),
            DisplayPage::Speaker
        );
        assert_eq!(
            DisplayPage::Speaker.next_page(true, true, false, false, false),
            DisplayPage::Ambient
        );
    }

    #[test]
    fn a_full_lap_walks_every_page_and_returns_home() {
        let mut manager = on_speaker();
        let pages = [
            DisplayPage::Ambient,
            DisplayPage::Attitude,
            DisplayPage::Audio,
        ];
        for expected in pages {
            manager.apply_business(BusinessIntent::NextPage);
            assert_eq!(manager.state().page, expected);
        }
        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(manager.state().page, DisplayPage::Speaker, "and back round");
    }

    #[test]
    fn a_tap_plays_the_next_sound_and_a_replay_is_counted_not_heard() {
        let mut manager = on_speaker();
        let boot = manager.playback_state();
        assert_eq!(boot.phase, PlaybackPhase::Idle);
        assert_eq!(boot.sound, Sound::Tap);
        assert!(!boot.muted, "a boot that is silent reads as dead");

        let second = Sound::Tap.next();
        play(&mut manager);
        let played = manager.playback_state();
        assert_eq!(played.phase, PlaybackPhase::Playing);
        assert_eq!(played.sound, second, "the tap advanced the catalogue");
        assert_eq!(played.plays, 1);
        assert_eq!(played.dropped, 0);

        play(&mut manager);
        let dropped = manager.playback_state();
        assert_eq!(
            dropped.phase,
            PlaybackPhase::Playing,
            "still the same sound"
        );
        assert_eq!(dropped.sound, second, "and not the next one");
        assert_eq!(dropped.plays, 1, "so no second sound started");
        assert_eq!(dropped.dropped, 1);

        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Idle);
        play(&mut manager);
        let second_play = manager.playback_state();
        assert_eq!(
            second_play.sound,
            second.next(),
            "the catalogue kept walking",
        );
        assert_eq!(second_play.plays, 2);
        assert_eq!(
            second_play.dropped, 1,
            "the dropped tally is kept across plays"
        );
    }

    #[test]
    fn a_finish_arriving_with_nothing_playing_is_ignored() {
        let mut manager = on_speaker();
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Idle);
        play(&mut manager);
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        assert_eq!(
            manager.playback_state().phase,
            PlaybackPhase::Idle,
            "a second finish has nothing left to finish"
        );
    }

    #[test]
    fn the_mute_latch_stands_until_it_is_flipped_back() {
        let mut manager = on_speaker();
        mute(&mut manager);
        assert!(manager.playback_state().muted);
        play(&mut manager);
        let muted = manager.playback_state();
        assert!(muted.muted, "still latched while playing");
        assert_eq!(muted.phase, PlaybackPhase::Playing, "a play still starts");
        mute(&mut manager);
        assert!(
            !manager.playback_state().muted,
            "and one press brings it back"
        );
    }

    #[test]
    fn muting_twice_does_not_cancel_itself_out() {
        let mut manager = on_speaker();
        mute(&mut manager);
        mute(&mut manager);
        assert!(!manager.playback_state().muted);
    }

    #[test]
    fn a_board_without_a_speaker_plays_nothing() {
        let mut manager = landed(DeviceManager::new());
        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(manager.state().page, DisplayPage::Ambient, "no page either");
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Next));
        assert_eq!(manager.playback_state(), PlaybackState::boot());
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::SetMuted(true)));
        assert!(
            !manager.playback_state().muted,
            "an unwired board's latch is nobody's business"
        );
    }

    #[test]
    fn the_tallies_saturate_rather_than_wrap() {
        let mut manager = on_speaker();
        manager.state.playback.plays = u16::MAX;
        manager.state.playback.dropped = u16::MAX;
        play(&mut manager);
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        let saturated = manager.playback_state();
        assert_eq!(saturated.plays, u16::MAX);
        assert_eq!(saturated.dropped, u16::MAX);
    }
}
