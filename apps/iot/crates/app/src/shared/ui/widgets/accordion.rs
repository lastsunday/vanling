//! A header row that opens one line of body copy beneath it.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{NodeId, Overflow, Style, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Accordion<'a> {
    title: &'a [u8],
    body: &'a [u8],
    open: bool,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> Accordion<'a> {
    pub const fn new(title: &'a [u8], body: &'a [u8]) -> Self {
        Self {
            title,
            body,
            open: false,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
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

impl Widget for Accordion<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let ink = ui.color(theme.ink);
        let muted = ui.color(theme.muted);
        let paper = ui.color(theme.selection);
        ui.column(
            Style::new().pad(3).gap(1).background(paper).tag(self.tag),
            |ui| {
                ui.row(Style::new().height(TOUCH_MIN as i16 - 6).middle(), |ui| {
                    ui.text(self.title, Style::new().nowrap().color(ink));
                    ui.node(Style::new().grow(1));
                    ui.text(
                        if self.open { b"-" } else { b"+" },
                        Style::new().nowrap().color(ink),
                    );
                });
                if self.open {
                    ui.text(
                        self.body,
                        Style::new()
                            .nowrap()
                            .overflow(Overflow::Ellipsis)
                            .color(muted),
                    );
                }
            },
        )
    }
}
