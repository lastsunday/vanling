//! A short state or count, on its own paper.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::kit::theme::Theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Accent,
    Danger,
}

#[derive(Clone, Copy, Debug)]
pub struct Badge<'a> {
    text: &'a [u8],
    tone: Tone,
    theme: &'a Theme,
}

impl<'a> Badge<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self {
            text,
            tone: Tone::Neutral,
            theme: &Theme::PANEL,
        }
    }

    pub const fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for Badge<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let (paper, ink) = match self.tone {
            Tone::Neutral => (self.theme.muted, self.theme.ground),
            Tone::Accent => (self.theme.accent, self.theme.on_accent),
            Tone::Danger => (self.theme.danger, self.theme.on_accent),
        };
        let paper = ui.color(paper);
        let ink = ui.color(ink);
        ui.column(Style::new().pad(2).center().background(paper), |ui| {
            ui.text(self.text, Style::new().nowrap().color(ink));
        })
    }
}
