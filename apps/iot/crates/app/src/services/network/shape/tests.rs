use super::*;
use crate::shared::intent::CHARS;
use crate::shared::intent::Confirm;
use crate::shared::model::{Entry, SetupWait};
use alloc::vec;
use alloc::vec::Vec;
use iot_core::drivers::network::{AccessPoint, LinkStatus, NetworkMode, SetupHost, SetupPass};

const LAYERS: [Layer; 3] = [Layer::Abc, Layer::Digits, Layer::Symbols];

fn asking() -> NetworkView {
    NetworkView {
        confirm: Some(Confirm::Forget(AccessPoint::default())),
        ..NetworkView::default()
    }
}

fn attempting() -> LinkStatus {
    LinkStatus {
        phase: LinkPhase::Connecting,
        ..LinkStatus::default()
    }
}

fn typing() -> NetworkView {
    NetworkView {
        keyboard: true,
        entry: Some(Entry::default()),
        ..NetworkView::default()
    }
}

fn bands(draft: NetworkView, link: LinkStatus) -> Vec<Band> {
    let mut asked = Vec::new();
    each_band(draft, link, |band| asked.push(band));
    asked
}

#[test]
fn the_order_is_the_layering() {
    assert_eq!(
        Band::IN_ORDER,
        [Band::Header, Band::Content, Band::Controls, Band::Overlay]
    );
}

#[test]
fn a_page_that_is_only_showing_the_page_has_no_box_over_it() {
    let (draft, link) = (NetworkView::default(), LinkStatus::default());
    assert!(Band::Header.showing(draft, link));
    assert!(
        Band::Content.showing(draft, link),
        "the list is the content"
    );
    assert!(
        Band::Controls.showing(draft, link),
        "and the strip is offered"
    );
    assert!(
        Band::Overlay.hidden(draft, link),
        "there is nothing over the list to draw"
    );
}

#[test]
fn a_question_hides_the_content_and_the_strip_and_nothing_else() {
    let draft = asking();
    let link = LinkStatus::default();
    assert!(
        Band::Content.hidden(draft, link),
        "the list is under the box"
    );
    assert!(
        Band::Controls.hidden(draft, link),
        "and the strip is not offered"
    );
    assert!(!Band::Header.hidden(draft, link), "the readings stay");
    assert!(
        Band::Overlay.showing(draft, link),
        "and the box is what is drawn"
    );
}

#[test]
fn a_join_in_flight_is_a_box_over_the_list_with_the_strip_still_offered() {
    let draft = NetworkView::default();
    let link = attempting();
    assert!(
        Band::Content.hidden(draft, link),
        "the box covers the list it is about"
    );
    assert!(
        Band::Controls.showing(draft, link),
        "the strip is not the box's to take"
    );
    assert!(Band::Overlay.showing(draft, link));
}

#[test]
fn the_setup_box_covers_the_list_and_leaves_the_strip_offered() {
    let draft = NetworkView::default();
    let link = LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable: false,
        },
        ..LinkStatus::default()
    };
    assert!(Band::Content.hidden(draft, link));
    assert!(
        Band::Controls.showing(draft, link),
        "the strip is how the panel gets back"
    );
    assert!(
        !Band::Header.hidden(draft, link),
        "the mode's name stays on the header"
    );
    assert!(Band::Overlay.showing(draft, link));
}

#[test]
fn a_phone_less_front_has_no_box_over_the_list() {
    let draft = NetworkView::default();
    let link = LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::Panel,
            cancellable: false,
        },
        ..LinkStatus::default()
    };
    assert!(!link.shows_setup_box());
    assert!(Band::Content.showing(draft, link));
    assert!(Band::Controls.showing(draft, link));
    assert!(Band::Overlay.hidden(draft, link));
}

#[test]
fn a_question_is_what_the_box_holds_when_a_link_is_joining_under_it() {
    let draft = asking();
    let link = attempting();
    assert!(Band::Content.hidden(draft, link));
    assert!(
        Band::Controls.hidden(draft, link),
        "the question holds the strip, whatever the radio is doing"
    );
    assert!(Band::Overlay.showing(draft, link));
}

#[test]
fn the_page_under_a_keyboard_has_no_strip_and_no_box() {
    let draft = typing();
    let link = LinkStatus::default();
    assert!(
        Band::Content.showing(draft, link),
        "the keyboard is the content"
    );
    assert!(
        Band::Controls.hidden(draft, link),
        "the strip is not a control while typing"
    );
    assert!(
        Band::Overlay.hidden(draft, link),
        "there is nothing for the box to say"
    );
}

#[test]
fn a_draft_is_the_pages_own_subject_over_an_attempt_too() {
    let draft = typing();
    let link = attempting();
    assert!(Band::Content.showing(draft, link));
    assert!(Band::Controls.hidden(draft, link));
    assert!(Band::Overlay.hidden(draft, link));
}

#[test]
fn the_strip_is_hidden_under_a_keyboard_because_the_two_are_the_same_band() {
    let strip = toolbar_box();
    let last = key_box(COLUMNS * (ROWS - 1)).expect("the keyboard's last row is a row of keys");
    assert!(
        strip.y < last.y + last.h,
        "the strip ({strip:?}) starts below the keys ({last:?}), so nothing is drawn over"
    );
    let label_top = strip.y + (strip.h - LINE) / 2;
    assert!(
        label_top < last.y + last.h,
        "and the strip's own word lands inside the last key row"
    );
}

#[test]
fn the_bands_hand_out_the_page_and_leave_the_box_out_while_there_is_nothing_to_say() {
    assert_eq!(
        bands(NetworkView::default(), LinkStatus::default()),
        vec![Band::Header, Band::Content, Band::Controls]
    );
}

#[test]
fn the_bands_never_ask_for_what_a_question_is_hiding() {
    assert_eq!(
        bands(asking(), LinkStatus::default()),
        vec![Band::Header, Band::Overlay]
    );
}

#[test]
fn the_bands_leave_the_strip_out_and_keep_the_box_in_while_a_finger_types() {
    assert_eq!(
        bands(typing(), LinkStatus::default()),
        vec![Band::Header, Band::Content]
    );
}

#[test]
fn the_bands_put_the_join_box_over_the_list_and_leave_the_strip_under_it() {
    assert_eq!(
        bands(NetworkView::default(), attempting()),
        vec![Band::Header, Band::Controls, Band::Overlay]
    );
}

#[test]
fn a_join_the_page_handed_over_is_a_box_before_the_radio_reports() {
    let draft = NetworkView {
        join_wait: true,
        ..NetworkView::default()
    };
    assert_eq!(
        bands(draft, LinkStatus::default()),
        vec![Band::Header, Band::Controls, Band::Overlay],
        "the box comes up on the tap's own frame, not on the radio's answer"
    );
}

#[test]
fn an_access_point_the_page_is_waiting_for_puts_its_box_up_first() {
    let waiting = NetworkView {
        setup_wait: Some(SetupWait::Host(SetupHost::AccessPoint)),
        ..NetworkView::default()
    };
    assert_eq!(
        bands(waiting, LinkStatus::default()),
        vec![Band::Header, Band::Controls, Band::Overlay],
        "the setup box is on the glass while the radio is still being reconfigured"
    );
}

#[test]
fn a_passphrase_change_alone_keeps_the_box_it_was_answered_in() {
    let draft = NetworkView {
        setup_wait: Some(SetupWait::Open(true)),
        ..NetworkView::default()
    };
    let link = LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable: false,
        },
        setup_password: Some(SetupPass::from_array(*b"k3mq9t7w2x")),
        ..LinkStatus::default()
    };
    assert_eq!(
        bands(draft, link),
        vec![Band::Header, Band::Controls, Band::Overlay],
        "the box is already the access point's, and the strip is how the panel gets back"
    );
}

#[test]
fn the_action_strip_is_one_target_under_the_list_and_the_page_ends_where_it_does() {
    let strip = toolbar_box();
    assert_eq!(strip.y, TOOLBAR_TOP);
    assert_eq!(strip.y + strip.h, CONTENT_BOTTOM);
    assert_eq!(strip.w, PANEL_COLUMNS);
    for y in TOOLBAR_TOP..CONTENT_BOTTOM {
        assert!(toolbar_at(y as u16), "row {y} is the strip's");
    }
    assert!(!toolbar_at(TOOLBAR_TOP as u16 - 1));
    assert!(!toolbar_at(CONTENT_BOTTOM as u16));
}

#[test]
fn a_split_strip_is_two_cells_of_one_width_that_meet_at_the_middle() {
    let left = toolbar_cell(ToolbarCell::Left);
    let right = toolbar_cell(ToolbarCell::Right);
    assert_eq!(left.w, right.w, "two cells of one strip are one width");
    assert_eq!(left.y, right.y);
    assert_eq!(left.y, TOOLBAR_TOP);
    assert_eq!(left.h, TOOLBAR_H);
    assert_eq!(left.x, TOOLBAR_GAP / 2, "the seam is outside the left cell");
    assert!(right.x + right.w + TOOLBAR_GAP / 2 == PANEL_COLUMNS);
    assert!(
        left.x + left.w <= right.x,
        "the cells do not overlap, whatever the seam does"
    );
    assert!(left.w >= TOUCH_MIN, "each cell is still a finger's own");

    for (x, cell) in [
        (0, ToolbarCell::Left),
        (PANEL_COLUMNS / 2 - 1, ToolbarCell::Left),
        (PANEL_COLUMNS / 2, ToolbarCell::Right),
        (PANEL_COLUMNS - 1, ToolbarCell::Right),
    ] {
        assert_eq!(
            toolbar_cell_at(x as u16, (TOOLBAR_TOP + TOOLBAR_H / 2) as u16),
            Some(cell),
            "column {x}"
        );
    }
    assert_eq!(toolbar_cell_at(0, TOOLBAR_TOP as u16 - 1), None);
    assert_eq!(toolbar_cell_at(0, CONTENT_BOTTOM as u16), None);
}

#[test]
fn a_band_is_a_row_from_the_top_of_the_list_to_the_next_one() {
    for band in 0..LIST_ROWS {
        for offset in 0..LIST_ROW_H {
            assert_eq!(
                list_band_at((LIST_TOP + band * LIST_ROW_H + offset) as u16),
                Some(band),
                "band {band} at offset {offset}"
            );
        }
    }
    assert_eq!(
        list_band_at((TOOLBAR_TOP - 1) as u16),
        Some(LIST_ROWS - 1),
        "the list's own last row is the last band"
    );
    assert_eq!(list_band_at(LIST_TOP as u16 - 1), None);
    assert_eq!(
        list_band_at(TOOLBAR_TOP as u16),
        None,
        "the action strip is not a band"
    );
    assert_eq!(list_band_at(CONTENT_BOTTOM as u16), None);
}

#[test]
fn a_band_holds_a_name_and_the_address_below_it_without_reaching_the_next_band() {
    for band in 0..LIST_ROWS {
        let top = band_lines_top(band);
        let band_top = LIST_TOP + band * LIST_ROW_H;
        assert!(top >= band_top, "band {band}'s name is above its own row");
        assert!(
            top + ROW_PITCH + LINE <= band_top + LIST_ROW_H,
            "band {band}'s address runs into the row under it"
        );
        let leftover = LIST_ROW_H - (ROW_PITCH + LINE);
        assert_eq!(top - band_top, leftover / 2, "band {band}");
        assert_eq!(
            band_top + LIST_ROW_H - (top + ROW_PITCH + LINE),
            leftover - leftover / 2,
            "band {band}"
        );
    }
}

#[test]
fn every_cell_lands_on_the_key_whose_box_holds_it() {
    for layer in LAYERS {
        for cell in 0..KEYS {
            let rect = key_box(cell).expect("a cell of the grid");
            for (x, y) in [
                (rect.x, rect.y),
                (rect.x + rect.w - 1, rect.y),
                (rect.x, rect.y + rect.h - 1),
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                (rect.x + rect.w / 2, rect.y + rect.h / 2),
            ] {
                assert_eq!(
                    key_at(layer, x as u16, y as u16),
                    key_of(layer, cell),
                    "{layer:?} cell {cell} at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn the_keys_a_tap_cannot_reach_are_the_ones_outside_the_keyboard() {
    assert_eq!(key_at(Layer::Abc, 5, KEYBOARD_TOP as u16 - 1), None);
    assert_eq!(key_at(Layer::Abc, 5, CONTENT_BOTTOM as u16), None);
    assert_eq!(
        key_at(Layer::Abc, PANEL_COLUMNS as u16, KEYBOARD_TOP as u16),
        None
    );
    assert_eq!(key_box(KEYS), None);
    assert_eq!(key_of(Layer::Abc, KEYS), None);
}

#[test]
fn the_digits_sit_in_the_block_a_phone_taught_the_thumb() {
    for digit in 0..9 {
        let cell = (digit / 3) * COLUMNS + digit % 3;
        assert_eq!(
            CELLS[cell],
            Cell::Digit(digit),
            "the key printed {} is not where the block puts it",
            digit + 1
        );
        assert_eq!(
            key_of(Layer::Abc, cell),
            Some(Key::Char(b'1' + digit as u8))
        );
    }
    let connect = cell_of(Key::Connect).expect("a control's own cell");
    assert_eq!(
        CELLS[connect - 1],
        Cell::Digit(9),
        "the zero beside connect"
    );
    assert_eq!(key_of(Layer::Abc, connect - 1), Some(Key::Char(b'0')));
}

#[test]
fn the_controls_are_the_same_cells_on_every_layer() {
    let controls = [
        Key::Layer,
        Key::Shift,
        Key::Backspace,
        Key::Connect,
        Key::Cancel,
        Key::Reveal,
    ];
    let mut cells = Vec::new();
    for key in controls {
        let cell = cell_of(key).expect("a control's own cell");
        for layer in LAYERS {
            assert_eq!(key_of(layer, cell), Some(key), "{key:?} on {layer:?}");
        }
        assert!(!cells.contains(&cell), "{key:?} shares a cell");
        cells.push(cell);
        assert_eq!(
            key_box(cell).expect("a cell of the grid").w,
            PANEL_COLUMNS / COLUMNS
        );
    }
    assert_eq!(
        cells.len(),
        KEYS - CHARS,
        "a cell the controls do not fill would be a key the grid shows but cannot name"
    );
}

#[test]
fn a_confirmation_answers_from_two_boxes_that_do_not_touch() {
    let keep = dialog_answer_box(DialogChoice::Keep);
    let act = dialog_answer_box(DialogChoice::Act);
    assert_eq!(keep.y, act.y);
    assert_eq!(
        act.x,
        keep.x + keep.w + DIALOG_GAP,
        "the two answers are one gap apart"
    );
    for choice in [DialogChoice::Keep, DialogChoice::Act] {
        let rect = dialog_answer_box(choice);
        for x in rect.x..rect.x + rect.w {
            for y in rect.y..rect.y + rect.h {
                assert_eq!(dialog_choice_at(x as u16, y as u16), Some(choice));
            }
        }
    }
    let seam = (keep.x + keep.w) as u16;
    assert_eq!(dialog_choice_at(seam, keep.y as u16), None);
    assert_eq!(dialog_choice_at(keep.x as u16, DIALOG_TOP as u16), None);
    assert_eq!(dialog_choice_at(0, 0), None);
}
