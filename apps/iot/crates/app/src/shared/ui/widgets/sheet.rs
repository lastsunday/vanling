//! A panel that rises from the bottom edge, or slides in from the left.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::theme::{CORNER, Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edge {
    #[default]
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug)]
pub struct Sheet<'a> {
    title: &'a [u8],
    rows: [&'a [u8]; 2],
    tags: [u16; 2],
    selected: u8,
    edge: Edge,
    width: i16,
    theme: &'a Theme,
}

impl<'a> Sheet<'a> {
    pub const fn new(title: &'a [u8], rows: [&'a [u8]; 2], tags: [u16; 2], selected: u8) -> Self {
        Self {
            title,
            rows,
            tags,
            selected,
            edge: Edge::Bottom,
            width: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn edge(mut self, edge: Edge) -> Self {
        self.edge = edge;
        self
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for Sheet<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let edge = ui.color(theme.muted);
        let ink = ui.color(theme.ink);
        let paper = ui.color(theme.selection);
        let ground = ui.color(theme.ground);
        let mut style = Style::new()
            .bordered()
            .color(edge)
            .radius(CORNER)
            .pad(3)
            .gap(1);
        if self.width > 0 {
            style = style.width(self.width);
        }
        ui.column(style, |ui| {
            if self.edge == Edge::Bottom {
                ui.node(Style::new().width(28).height(3).radius(2).background(edge));
            }
            ui.text(self.title, Style::new().nowrap().color(ink));
            for (index, row) in self.rows.into_iter().enumerate() {
                let on = index == usize::from(self.selected);
                ui.text(
                    row,
                    Style::new()
                        .nowrap()
                        .pad(2)
                        .tag(self.tags[index])
                        .background(if on { paper } else { ground })
                        .color(ink),
                );
            }
        })
    }
}
