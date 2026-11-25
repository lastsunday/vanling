//! The Speaker page: what a tap plays, the mute latch, and the readout the output reports.

#[cfg(test)]
mod tests;

use crate::shared::i18n::Msg;
use crate::shared::i18n::words::sound_label;
use crate::shared::intent::{BusinessIntent, InputIntent, PlaybackAction};
use crate::shared::model::{Command, DeviceState, Effects, PlaybackPhase};
use crate::shared::page;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::ui::text::format_u16;
use iot_core::ui::{Style, Ui};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeakerPage;

impl super::Service for SpeakerPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        if let BusinessIntent::Playback(action) = intent {
            state.playback.apply(*action, state.backed.playback);
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for SpeakerPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        match op.event {
            InputEvent::Button(ButtonEvent::Click)
            | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
                Some(BusinessIntent::Playback(PlaybackAction::Next))
            }
            InputEvent::Gesture(GestureEvent::LongPress { .. }) => Some(BusinessIntent::Playback(
                PlaybackAction::SetMuted(!state.playback.muted),
            )),
            _ => None,
        }
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        let lang = snapshot.lang;
        let playback = snapshot.playback;
        let fault = playback.health.fault();
        page::body(ui, |ui| {
            ui.row(Style::new().between(), |ui| {
                page::title(ui, Msg::SpeakerTitle.bytes(lang));
                if playback.muted {
                    ui.text(Msg::Mute.bytes(lang), Style::new().nowrap());
                }
            });
            page::reading(
                ui,
                b"ST",
                match (fault, playback.phase) {
                    (Some(_), _) => Msg::NoOut.bytes(lang),
                    (None, PlaybackPhase::Idle) => Msg::Idle.bytes(lang),
                    (None, PlaybackPhase::Playing) => Msg::Playing.bytes(lang),
                },
            );
            page::reading(ui, b"SRC", sound_label(playback.sound));
            page::reading(ui, b"PLY", format_u16(playback.plays, &mut [0u8; 6]));
            page::reading(ui, b"DRP", format_u16(playback.dropped, &mut [0u8; 6]));
            ui.text(b"", Style::new());
            match fault {
                Some(reason) => {
                    ui.text(Msg::TapRetry.bytes(lang), Style::new().nowrap());
                    ui.text(reason.as_bytes(), Style::new().nowrap());
                }
                None => {
                    ui.text(Msg::TapPlay.bytes(lang), Style::new().nowrap());
                    ui.text(Msg::HoldMute.bytes(lang), Style::new().nowrap());
                }
            }
        });
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view).with_repaints(super::Repaint::When(playback_moved))
    }
}

fn playback_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.playback != next.playback
}
