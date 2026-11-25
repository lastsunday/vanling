//! One choice out of three, drawn as a bordered strip.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::{CORNER, Theme};

#[derive(Clone, Copy, Debug)]
pub struct Segmented<'a> {
    labels: [&'a [u8]; 3],
    tags: [u16; 3],
    selected: u8,
    height: i16,
    theme: Option<&'a Theme>,
}

impl<'a> Segmented<'a> {
    pub const fn new(labels: [&'a [u8]; 3], tags: [u16; 3], selected: u8) -> Self {
        Self {
            labels,
            tags,
            selected,
            height: TOUCH_MIN as i16,
            theme: None,
        }
    }

    pub const fn height(mut self, height: i16) -> Self {
        self.height = height;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Widget for Segmented<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let edge = ui.color(theme.muted);
        let fg = ui.color(theme.foreground);
        let muted = ui.color(theme.muted);
        let paper = ui.color(theme.selection);
        ui.row(
            Style::new()
                .height(self.height)
                .stretch()
                .bordered()
                .color(edge)
                .radius(CORNER),
            |ui| {
                for (index, label) in self.labels.into_iter().enumerate() {
                    let on = index == usize::from(self.selected);
                    let style = Style::new()
                        .grow(1)
                        .basis(0)
                        .center()
                        .ellipsis()
                        .tag(self.tags[index])
                        .text_align(TextAlign::Center)
                        .background(if on { paper } else { ui.color(theme.ground) })
                        .color(if on { fg } else { muted });
                    ui.text(label, style);
                }
            },
        )
    }
}
