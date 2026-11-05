//! Where the bench's finger lands: the cells and rows the panel draws, as coordinates.

use crate::controls;
use crate::controls::{DialogChoice, Key};

use super::BAND_X;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Touch {
    At { x: u16, y: u16, at_ms: u32 },
    Hold { x: u16, y: u16 },
    NextPage,
}

pub(super) fn strip_point() -> Touch {
    Touch::At {
        x: BAND_X,
        y: (controls::TOOLBAR_TOP + controls::TOOLBAR_H / 2) as u16,
        at_ms: 0,
    }
}

pub(super) fn answer_point(choice: DialogChoice) -> Touch {
    let rect = controls::dialog_answer_box(choice);
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

pub(super) fn row_point(band: usize) -> Touch {
    Touch::At {
        x: BAND_X,
        y: band_center(band),
        at_ms: 0,
    }
}

pub(super) fn hold_point(band: usize) -> Touch {
    Touch::Hold {
        x: BAND_X,
        y: band_center(band),
    }
}

pub(super) fn cancel_point() -> Touch {
    let cell = controls::cell_of(Key::Cancel).expect("the grid holds cancel");
    let rect = controls::key_box(cell).expect("a cell of the grid");
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

pub(super) fn band_center(band: usize) -> u16 {
    (controls::LIST_TOP + band * controls::LIST_ROW_H + controls::LIST_ROW_H / 2) as u16
}
