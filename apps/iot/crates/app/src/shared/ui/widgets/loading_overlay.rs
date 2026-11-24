//! A dimmed box with one word in the middle: the page is busy and cannot be used.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct LoadingOverlay<'a> {
    text: &'a [u8],
    on: bool,
    width: i16,
    height: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> LoadingOverlay<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self {
            text,
            on: false,
            width: 0,
            height: 0,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Marks the busy box so a page can find the point that takes the overlay down.
    pub const fn tag(mut self, tag: u16) -> Self {
        self.tag = tag;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for LoadingOverlay<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let width = if self.width > 0 {
            self.width
        } else {
            ui.width() as i16
        };
        let height = if self.height > 0 {
            self.height
        } else {
            ui.height() as i16
        };
        let dim = ui.color(theme.overlay);
        let ink = ui.color(theme.ink);
        let ground = ui.color(theme.ground);
        ui.stack(Style::new().width(width).height(height), |ui| {
            ui.node(
                Style::new()
                    .absolute()
                    .width(width)
                    .height(height)
                    .background(if self.on { dim } else { ground }),
            );
            if self.on {
                ui.text(
                    self.text,
                    Style::new()
                        .absolute()
                        .width(width)
                        .height(height)
                        .center()
                        .nowrap()
                        .text_align(TextAlign::Center)
                        .tag(self.tag)
                        .color(ink),
                );
            }
        })
    }
}
