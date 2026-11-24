//! Three steps; the current one prints its own name, the others their number.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::text::format_u16;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Stepper<'a> {
    labels: [&'a [u8]; 3],
    current: u8,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> Stepper<'a> {
    pub const fn new(labels: [&'a [u8]; 3], current: u8) -> Self {
        Self {
            labels,
            current,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn tag(mut self, tag: u16) -> Self {
        self.tag = tag;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for Stepper<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let paper = ui.color(theme.accent);
        let on_paper = ui.color(theme.on_accent);
        let muted = ui.color(theme.muted);
        let current = usize::from(self.current).min(2);
        let mut digits = [0u8; 6];
        ui.row(
            Style::new().height(TOUCH_MIN as i16).gap(2).tag(self.tag),
            |ui| {
                for (index, label) in self.labels.into_iter().enumerate() {
                    let on = index == current;
                    let style = Style::new()
                        .grow(1)
                        .basis(0)
                        .center()
                        .nowrap()
                        .radius(3)
                        .text_align(TextAlign::Center)
                        .background(if on { paper } else { ui.color(theme.ground) })
                        .color(if on { on_paper } else { muted });
                    let step = u16::try_from(index + 1).unwrap_or(1);
                    ui.text(
                        if on {
                            label
                        } else {
                            format_u16(step, &mut digits)
                        },
                        style,
                    );
                }
            },
        )
    }
}
