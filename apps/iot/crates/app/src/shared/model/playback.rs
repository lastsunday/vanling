use crate::shared::intent::PlaybackAction;
use iot_core::drivers::playback::{PlaybackPhase, Sound};

use super::Health;

/// Playback state behind the Speaker page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackState {
    pub phase: PlaybackPhase,
    /// The sound a tap plays, and the page's `SRC` row.
    pub sound: Sound,
    /// The output latch.
    pub muted: bool,
    /// Whether the output is answering. A tap while it is down is still sent: that attempt
    /// is the recovery.
    pub health: Health,
    /// What the page counts: the sounds that played and the taps that lost.
    pub view: PlaybackView,
}

/// The tallies the Speaker page draws. They are the page's own numbers: nothing in the
/// playback logic reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackView {
    /// Sounds actually started; the panel's `PLY` row.
    pub plays: u16,
    /// Taps that arrived while a sound was sounding and were therefore not heard; the
    /// panel's second `PLY` figure, so a busy finger is visible as a number rather than as
    /// a sound that did not happen.
    pub dropped: u16,
}

/// What the playback task must do to a speaker to follow the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommand {
    Play(Sound),
    SetMuted(bool),
}

impl PlaybackState {
    /// The mute edge, if this state crossed it.
    pub fn mute_command(&self, previous: &Self) -> Option<PlaybackCommand> {
        (self.muted != previous.muted).then_some(PlaybackCommand::SetMuted(self.muted))
    }

    /// The start edge, if this state crossed it. A sound already playing is never sent
    /// again: a tap over it is a drop, which never leaves `Playing`.
    pub fn play_command(&self, previous: &Self) -> Option<PlaybackCommand> {
        (self.phase == PlaybackPhase::Playing && previous.phase != PlaybackPhase::Playing)
            .then_some(PlaybackCommand::Play(self.sound))
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::boot()
    }
}

impl PlaybackState {
    /// The state a board boots into: idle, on the first sound, audible.
    pub const BOOT: Self = Self {
        phase: PlaybackPhase::Idle,
        sound: Sound::Tap,
        muted: false,
        health: Health::Ready,
        view: PlaybackView {
            plays: 0,
            dropped: 0,
        },
    };

    pub const fn boot() -> Self {
        Self::BOOT
    }

    /// What the Speaker page asks, refused when the board wired no output part. A tap while
    /// the part is down still leaves the state as `Playing`, so the worker gets the request
    /// and can retry the part: `Started` confirms it, `Refused` takes the page back to idle
    /// and records the reason.
    pub(crate) fn apply(&mut self, action: PlaybackAction, wired: bool) {
        if !wired {
            return;
        }
        match action {
            PlaybackAction::Next => match self.phase {
                PlaybackPhase::Playing => {
                    self.view.dropped = self.view.dropped.saturating_add(1);
                }
                PlaybackPhase::Idle => {
                    self.sound = self.sound.next();
                    self.phase = PlaybackPhase::Playing;
                }
            },
            PlaybackAction::SetMuted(muted) => self.muted = muted,
            PlaybackAction::Finished => {
                if self.phase == PlaybackPhase::Playing {
                    self.phase = PlaybackPhase::Idle;
                }
            }
            PlaybackAction::Started => {
                self.health = Health::Ready;
                self.view.plays = self.view.plays.saturating_add(1);
            }
            PlaybackAction::Ready => self.health = Health::Ready,
            PlaybackAction::Refused(reason) => {
                self.health = Health::Down(reason);
                if self.phase == PlaybackPhase::Playing {
                    self.phase = PlaybackPhase::Idle;
                }
            }
        }
    }
}

#[cfg(test)]
mod speaker_page {
    use super::*;
    use crate::shared::intent::*;
    use crate::state::test_support::*;
    use iot_core::drivers::motion::MotionCapabilities;

    use iot_core::drivers::playback::Sound;

    fn on_speaker() -> DeviceManager {
        let mut manager = landed(DeviceManager::boot(Boot {
            backed: Backed {
                motion: Some(MotionCapabilities::EMPTY),
                audio: true,
                playback: true,
                ..Backed::NONE
            },
            ..Default::default()
        }));
        for _ in 0..=3 {
            if manager.state().page() == Page::Speaker {
                break;
            }
            apply_page(&mut manager, triple_click());
        }
        assert_eq!(manager.state().page(), Page::Speaker);
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
            manager.apply_action(ActionIntent::Playback(PlaybackAction::Started));
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
            Page::Ambient.next_page(Backed {
                motion: Some(MotionCapabilities::EMPTY),
                playback: true,
                ..Backed::NONE
            }),
            Page::Attitude
        );
        assert_eq!(
            Page::Audio.next_page(Backed {
                motion: Some(MotionCapabilities::EMPTY),
                audio: true,
                playback: true,
                ..Backed::NONE
            }),
            Page::Speaker
        );
        assert_eq!(
            Page::Audio.next_page(Backed {
                motion: Some(MotionCapabilities::EMPTY),
                audio: true,
                ..Backed::NONE
            }),
            Page::Ambient,
            "a board without a speaker skips the page"
        );
        assert_eq!(
            Page::Ambient.next_page(Backed {
                playback: true,
                ..Backed::NONE
            }),
            Page::Speaker
        );
        assert_eq!(
            Page::Speaker.next_page(Backed {
                motion: Some(MotionCapabilities::EMPTY),
                audio: true,
                ..Backed::NONE
            }),
            Page::Ambient
        );
    }

    #[test]
    fn a_full_lap_walks_every_page_and_returns_home() {
        let mut manager = on_speaker();
        let pages = [Page::Ambient, Page::Attitude, Page::Audio];
        for expected in pages {
            manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
            assert_eq!(manager.state().page(), expected);
        }
        manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
        assert_eq!(manager.state().page(), Page::Speaker, "and back round");
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
        assert_eq!(played.view.plays, 1);
        assert_eq!(played.view.dropped, 0);

        play(&mut manager);
        let dropped = manager.playback_state();
        assert_eq!(
            dropped.phase,
            PlaybackPhase::Playing,
            "still the same sound"
        );
        assert_eq!(dropped.sound, second, "and not the next one");
        assert_eq!(dropped.view.plays, 1, "so no second sound started");
        assert_eq!(dropped.view.dropped, 1);

        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Idle);
        play(&mut manager);
        let second_play = manager.playback_state();
        assert_eq!(
            second_play.sound,
            second.next(),
            "the catalogue kept walking",
        );
        assert_eq!(second_play.view.plays, 2);
        assert_eq!(
            second_play.view.dropped, 1,
            "the dropped tally is kept across plays"
        );
    }

    #[test]
    fn a_finish_arriving_with_nothing_playing_is_ignored() {
        let mut manager = on_speaker();
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Idle);
        play(&mut manager);
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
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
    fn a_refused_play_leaves_the_page_idle_and_names_the_reason() {
        let mut manager = on_speaker();
        apply_page(&mut manager, tap());
        let asked = manager.playback_state();
        assert_eq!(
            asked.phase,
            PlaybackPhase::Playing,
            "the tap still reaches the worker, which is the recovery attempt"
        );
        assert_eq!(asked.view.plays, 0, "nothing is counted before it started");

        manager.apply_action(ActionIntent::Playback(PlaybackAction::Refused(
            "the codec did not answer on I2C",
        )));
        let refused = manager.playback_state();
        assert_eq!(refused.phase, PlaybackPhase::Idle);
        assert_eq!(
            refused.health,
            Health::Down("the codec did not answer on I2C")
        );
        assert_eq!(refused.view.plays, 0);

        apply_page(&mut manager, tap());
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Started));
        let recovered = manager.playback_state();
        assert_eq!(recovered.health, Health::Ready);
        assert_eq!(recovered.view.plays, 1, "the retry is what gets counted");
    }

    #[test]
    fn the_boot_report_reaches_the_page_and_a_ready_clears_the_reason() {
        let mut manager = on_speaker();
        assert_eq!(
            manager.state().playback_diagnostics().health,
            Health::Ready,
            "the page starts assuming nothing is wrong"
        );
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Refused("no codec")));
        assert_eq!(
            manager.state().playback_diagnostics().health,
            Health::Down("no codec"),
            "the worker's boot report is what the page shows"
        );
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Ready));
        assert_eq!(
            manager.state().playback_diagnostics().health,
            Health::Ready,
            "a later report clears NO OUT"
        );
    }

    #[test]
    fn a_command_is_sent_only_on_the_edge_that_crossed_it() {
        let idle = PlaybackState::boot();
        let mut playing = idle;
        playing.phase = PlaybackPhase::Playing;
        playing.sound = Sound::Warn;
        let mut muted = idle;
        muted.muted = true;

        assert_eq!(
            playing.play_command(&idle),
            Some(PlaybackCommand::Play(Sound::Warn)),
            "entering Playing starts the sound the state names"
        );
        assert_eq!(
            playing.play_command(&playing),
            None,
            "a pass that stays in Playing must not replay the sound"
        );
        assert_eq!(
            idle.play_command(&playing),
            None,
            "finishing a sound sends nothing to the driver"
        );
        assert_eq!(
            muted.mute_command(&idle),
            Some(PlaybackCommand::SetMuted(true))
        );
        assert_eq!(
            muted.mute_command(&muted),
            None,
            "the latch is written on the edge, not every pass"
        );
    }

    #[test]
    fn a_board_without_a_speaker_plays_nothing() {
        let mut manager = landed(DeviceManager::new());
        manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
        assert_eq!(manager.state().page(), Page::Ambient, "no page either");
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Next));
        assert_eq!(manager.playback_state(), PlaybackState::boot());
        manager.apply_action(ActionIntent::Playback(PlaybackAction::SetMuted(true)));
        assert!(
            !manager.playback_state().muted,
            "an unwired board's latch is nobody's business"
        );
    }

    #[test]
    fn the_tallies_saturate_rather_than_wrap() {
        let mut manager = on_speaker();
        manager.state.playback.view.plays = u16::MAX;
        manager.state.playback.view.dropped = u16::MAX;
        play(&mut manager);
        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
        let saturated = manager.playback_state();
        assert_eq!(saturated.view.plays, u16::MAX);
        assert_eq!(saturated.view.dropped, u16::MAX);
    }
}
