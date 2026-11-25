//! A short state or count, on its own paper.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Accent,
    Danger,
}

/// How the badge lays its paper down; the tone picks the colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// Solid tone paper with ink that reads on it.
    #[default]
    Filled,
    /// A tinted paper under tone-coloured ink.
    Light,
    /// No paper, a one-pixel tone outline.
    Outline,
}

#[derive(Clone, Copy, Debug)]
pub struct Badge<'a> {
    text: &'a [u8],
    tone: Tone,
    variant: Variant,
    theme: Option<&'a Theme>,
}

impl<'a> Badge<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self {
            text,
            tone: Tone::Neutral,
            variant: Variant::Filled,
            theme: None,
        }
    }

    pub const fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub const fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Widget for Badge<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let (paper, ink) = match self.tone {
            Tone::Neutral => (theme.muted, theme.ground),
            Tone::Accent => (theme.accent, theme.on_accent),
            Tone::Danger => (theme.danger, theme.on_accent),
        };
        // Light and outline keep the tone as ink, so the paper is the only thing that moves.
        let edge = match self.tone {
            Tone::Neutral => theme.muted,
            Tone::Accent => theme.accent,
            Tone::Danger => theme.danger,
        };
        let (style, ink) = match self.variant {
            Variant::Filled => (Style::new().background(ui.color(paper)), ink),
            Variant::Light => (Style::new().background(ui.color(theme.selection)), edge),
            Variant::Outline => (Style::new().bordered(), edge),
        };
        let ink = ui.color(ink);
        ui.text(self.text, style.radius(3).pad(2).nowrap().color(ink))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use iot_core::light::{Rgb, rgb565};
    use iot_core::ui::{BoxConstraints, Canvas, PX, Rect, Screen};

    const W: usize = 40;
    const H: usize = 24;

    fn render(variant: Variant, tone: Tone) -> (Vec<u8>, Rect) {
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        ui.column(Style::new().pad(4), |ui| {
            ui.add(Badge::new(b"ON").tone(tone).variant(variant));
        });
        ui.paint(
            BoxConstraints::tight(W as i32 * PX, H as i32 * PX),
            &mut |_, _, _| {},
        );
        let rect = ui.rects().last().expect("the badge is the last box");
        let frame = ui.into_canvas().expect("a canvas was handed in").frame();
        (frame.to_vec(), rect)
    }

    fn pixel(frame: &[u8], x: i32, y: i32) -> u16 {
        let at = (y as usize * W + x as usize) * 2;
        u16::from_be_bytes([frame[at], frame[at + 1]])
    }

    /// A pixel the run cannot reach: the bottom padding, left of the glyph's stem.
    fn padding(frame: &[u8], rect: Rect) -> u16 {
        pixel(frame, rect.x + 1, rect.bottom() - 2)
    }

    #[test]
    fn a_filled_badge_lays_its_tone_colour_and_rounds_its_corner() {
        let (frame, rect) = render(Variant::Filled, Tone::Accent);
        assert_eq!(padding(&frame, rect), rgb565(Rgb(0x4C, 0xB8, 0xFF)));
        assert_eq!(pixel(&frame, rect.x, rect.y), 0, "the corner is cut away");
    }

    #[test]
    fn an_outline_badge_keeps_the_paper_empty() {
        let (frame, rect) = render(Variant::Outline, Tone::Accent);
        let accent = rgb565(Rgb(0x4C, 0xB8, 0xFF));
        assert_eq!(padding(&frame, rect), 0, "no paper, the run only");
        assert_eq!(pixel(&frame, rect.x + 2, rect.y), accent, "the outline");
    }

    #[test]
    fn a_light_badge_tints_the_paper_under_the_tone_ink() {
        let (light, rect) = render(Variant::Light, Tone::Accent);
        let (filled, _) = render(Variant::Filled, Tone::Accent);
        assert_ne!(padding(&light, rect), padding(&filled, rect));
    }
}
