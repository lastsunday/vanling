//! The Audio page: capture control and the meter the capture's envelope is drawn as.

mod sweep;

#[cfg(test)]
mod tests;

use iot_core::drivers::audio::{dbfs, spl};
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::ui::{Canvas, Rect, Screen, Style, Ui};

use crate::shared::intent::{AudioIntent, BusinessIntent, InputIntent, LightAction};
use crate::shared::kit::chrome;
use crate::shared::kit::i18n::Msg;
use crate::shared::kit::text::{format_dba, format_i32, format_u16};
use crate::shared::model::{AudioPhase, Command, DeviceState, Effects};
use crate::shared::snapshot::FrameSnapshot;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioPage;

impl super::Service for AudioPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        if let BusinessIntent::Audio(intent) = intent {
            state.audio.apply(*intent, state.backed.audio);
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for AudioPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        match op.event {
            InputEvent::Button(ButtonEvent::Click)
            | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
                let intent = if state.audio.phase == AudioPhase::Recording {
                    AudioIntent::Stop
                } else {
                    AudioIntent::Start
                };
                Some(BusinessIntent::Audio(intent))
            }
            InputEvent::Button(ButtonEvent::DoubleClick) => {
                state.light_intent(op, LightAction::Step)
            }
            InputEvent::Button(ButtonEvent::LongPress)
            | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
                state.light_intent(op, LightAction::Cycle)
            }
            InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
                state.light_swipe(op, direction)
            }
            _ => None,
        }
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        let lang = snapshot.lang;
        let audio = &snapshot.audio;
        let fault = audio.health.fault();
        let offset = ui.screen().spl_offset_db;
        chrome::page(ui, |ui| {
            ui.row(Style::new().between(), |ui| {
                chrome::title(ui, Msg::AudioTitle.bytes(lang));
                ui.text(
                    format_dba(spl(dbfs(audio.dba_lsb), offset), &mut [0u8; 12]),
                    Style::new().nowrap(),
                );
            });
            level(ui, b"PK", dbfs(audio.envelope.loudest()), offset);
            level(ui, b"RMS", dbfs(audio.envelope.loudest_rms()), offset);
            ui.row(Style::new().between(), |ui| {
                chrome::reading(
                    ui,
                    b"ST",
                    match (fault, audio.phase) {
                        (Some(_), _) => Msg::NoIn.bytes(lang),
                        (None, AudioPhase::Idle) => Msg::Idle.bytes(lang),
                        (None, AudioPhase::Recording) => Msg::Recording.bytes(lang),
                        (None, AudioPhase::Stopped) => Msg::Stopped.bytes(lang),
                    },
                );
                chrome::reading(
                    ui,
                    b"MS",
                    format_i32(
                        i32::try_from(audio.elapsed_ms).unwrap_or(i32::MAX),
                        &mut [0u8; 12],
                    ),
                );
            });
            ui.row(Style::new().gap(12), |ui| {
                chrome::reading(
                    ui,
                    b"COL",
                    format_u16(u16::from(audio.envelope.committed()), &mut [0u8; 6]),
                );
                chrome::reading(ui, b"RST", format_u16(audio.restarts, &mut [0u8; 6]));
                if audio.envelope.clipped() {
                    ui.text(Msg::Clip.bytes(lang), Style::new().nowrap());
                }
            });
            if let Some(reason) = fault {
                ui.text(reason.as_bytes(), Style::new());
                ui.text(Msg::TapRetry.bytes(lang), Style::new().nowrap());
                return;
            }
            if audio.phase == AudioPhase::Idle {
                ui.text(Msg::TapToRec.bytes(lang), Style::new().nowrap());
                return;
            }
            ui.custom(Style::new().grow(1).stretch(), 0);
        });
    }

    fn descriptor() -> super::Descriptor {
        // A capture poll closes two envelope columns every 20 ms; at the default step the
        // sweep would hold the executor 40% of the wall time on the S3.
        super::Descriptor::new(Self::view)
            .with_repaint_ms(super::REPAINT_FLOOR_MS * 2)
            .with_custom(custom)
            .with_repaints(super::Repaint::When(audio_moved))
    }
}

/// The sweep is the page's only custom leaf: slot 0.
fn custom(
    snapshot: &FrameSnapshot,
    screen: Screen,
    _slot: u8,
    canvas: &mut Canvas<'_>,
    rect: Rect,
) {
    let envelope = &snapshot.audio.envelope;
    let peaks = envelope.weighted_released_peaks();
    let levels = envelope.weighted_rms_columns();
    sweep::draw(
        canvas,
        rect,
        &peaks,
        &levels,
        envelope.clipped_age(),
        screen.probe,
    );
}

fn audio_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.audio.phase != next.audio.phase
        || prev.audio.envelope != next.audio.envelope
        || prev.audio.restarts != next.audio.restarts
        || prev.audio.dba_lsb != next.audio.dba_lsb
}

/// One level row: the decibels and the sound pressure they mean.
fn level(ui: &mut Ui<'_, '_>, label: &[u8], decibels: i16, offset: i16) {
    let mut buf = [0u8; 48];
    let take = label.len().min(chrome::LABEL_BYTES);
    buf[..take].copy_from_slice(&label[..take]);
    let mut end = take;
    while end < chrome::LABEL_BYTES {
        buf[end] = b' ';
        end += 1;
    }
    end = append(&mut buf, end, b" ");
    end = append(
        &mut buf,
        end,
        format_i32(i32::from(decibels), &mut [0u8; 12]),
    );
    end = append(&mut buf, end, b" DBFS ");
    end = append(
        &mut buf,
        end,
        format_i32(i32::from(spl(decibels, offset)), &mut [0u8; 12]),
    );
    end = append(&mut buf, end, b" SPL");
    ui.text(&buf[..end], Style::new().nowrap());
}

fn append(buf: &mut [u8], at: usize, run: &[u8]) -> usize {
    let end = (at + run.len()).min(buf.len());
    let take = end - at;
    buf[at..end].copy_from_slice(&run[..take]);
    end
}
