//! Three grey bars standing in for content that has not arrived.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::theme::Theme;

/// How much of the box each bar takes, in percent.
const SHARE: [i16; 3] = [100, 72, 48];

#[derive(Clone, Copy, Debug)]
pub struct Skeleton {
    width: i16,
    theme: &'static Theme,
}

impl Skeleton {
    pub const fn new(width: i16) -> Self {
        Self {
            width,
            theme: &Theme::PANEL,
        }
    }
}

impl Widget for Skeleton {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let paper = ui.color(self.theme.muted);
        let width = self.width;
        ui.column(Style::new().gap(3), |ui| {
            for share in SHARE {
                ui.node(
                    Style::new()
                        .height(6)
                        .width(width * share / 100)
                        .radius(3)
                        .background(paper),
                );
            }
        })
    }
}
