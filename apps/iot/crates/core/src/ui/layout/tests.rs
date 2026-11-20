use super::*;
use crate::ui::{Screen, Ui};

fn page(width: i16, height: i16) -> Ui<'static, 'static> {
    Ui::measure(width as usize, height as usize, Screen::default())
}

fn finish(ui: &mut Ui<'_, '_>, width: i16, height: i16) {
    ui.layout(BoxConstraints::tight(
        Sub::from(width) * UNIT,
        Sub::from(height) * UNIT,
    ));
}

#[test]
fn a_growing_child_takes_the_space_the_other_leaves() {
    let mut ui = page(100, 20);
    let mut right = NodeId::NONE;
    ui.row(Style::new(), |ui| {
        ui.text(b"A", Style::new().width(10).nowrap());
        right = ui.text(b"B", Style::new().grow(1));
    });
    finish(&mut ui, 100, 20);
    assert_eq!(ui.rect(right), crate::ui::Rect::new(10, 0, 90, 12));
}

#[test]
fn a_wrapping_row_breaks_into_runs() {
    let mut ui = page(100, 100);
    let mut ids = [NodeId::NONE; 3];
    ui.wrap(Style::new().row().stretch(), |ui| {
        for id in &mut ids {
            *id = ui.text(b"XXXXXX", Style::new().width(40).nowrap());
        }
    });
    finish(&mut ui, 100, 100);
    assert_eq!(ui.rect(ids[0]).x, 0);
    assert_eq!(ui.rect(ids[1]).x, 40, "two fit on the first run");
    assert_eq!(ui.rect(ids[2]).y, 12, "the third starts a second run");
}

#[test]
fn a_text_node_re_measures_when_the_width_changes() {
    let mut tree = Tree::new();
    let root = tree.push(Style::new().stretch(), Kind::Box, NodeId::NONE);
    let run = b"AAAAA AAAAA";
    let text = tree.push(
        Style::new(),
        Kind::Text {
            start: 0,
            len: run.len() as u16,
        },
        root,
    );
    tree.finish(BoxConstraints::tight(30 * UNIT, 40 * UNIT), run);
    assert_eq!(tree.rect(text).h, 24, "at 30 the run takes two lines");
    tree.finish(BoxConstraints::tight(72 * UNIT, 40 * UNIT), run);
    assert_eq!(tree.rect(text).h, 12, "at 72 it takes one");
}

#[test]
fn an_absolute_child_is_placed_by_its_insets() {
    let mut ui = page(100, 100);
    let mut id = NodeId::NONE;
    ui.stack(Style::new(), |ui| {
        id = ui.node(Style::new().absolute().left(10).top(5).size(20, 10));
    });
    finish(&mut ui, 100, 100);
    assert_eq!(ui.rect(id), crate::ui::Rect::new(10, 5, 20, 10));
}

#[test]
fn a_stretched_stack_reaches_its_parents_edge() {
    let mut ui = page(100, 40);
    let mut id = NodeId::NONE;
    ui.column(Style::new().stretch().grow(1), |ui| {
        ui.stack(Style::new().grow(1), |ui| {
            id = ui.node(Style::new().absolute().right(0).bottom(0).size(20, 10));
        });
    });
    finish(&mut ui, 100, 40);
    assert_eq!(ui.rect(id), crate::ui::Rect::new(80, 30, 20, 10));
}

#[test]
fn an_inset_of_zero_means_the_edge_not_an_unset_side() {
    let mut ui = page(100, 100);
    let mut corner = NodeId::NONE;
    let mut offset = NodeId::NONE;
    ui.stack(Style::new(), |ui| {
        corner = ui.node(Style::new().absolute().left(0).top(0).size(10, 10));
        offset = ui.node(Style::new().absolute().left(5).top(5).size(10, 10));
    });
    finish(&mut ui, 100, 100);
    assert_eq!(ui.rect(corner), crate::ui::Rect::new(0, 0, 10, 10));
    assert_eq!(ui.rect(offset), crate::ui::Rect::new(5, 5, 10, 10));
}

#[test]
fn a_scroll_box_clips_and_shifts_its_content() {
    let mut ui = page(100, 20);
    let mut child = NodeId::NONE;
    ui.scroll(Style::new().height(20).offset(10), |ui| {
        child = ui.node(Style::new().height(100));
    });
    finish(&mut ui, 100, 20);
    assert_eq!(ui.rect(child).y, -10);
    assert!(
        ui.tree().info(child).clipped,
        "a scroll box clips its content"
    );
}

#[test]
fn a_percentage_resolves_against_the_parent() {
    let mut ui = page(100, 20);
    let mut id = NodeId::NONE;
    ui.column(Style::new().stretch(), |ui| {
        id = ui.node(Style::new().width(0).height(10));
        // The percentage is set through the style, not the builder's pixel width.
        let mut style = Style::new();
        style.width = Length::Percent(50);
        style.height = Length::Px(10);
        id = ui.node(style);
    });
    finish(&mut ui, 100, 20);
    assert_eq!(ui.rect(id).w, 50);
}

#[test]
fn a_fractional_grow_still_lands_on_whole_pixels_without_gaps() {
    let mut ui = page(10, 12);
    let mut ids = [NodeId::NONE; 3];
    ui.row(Style::new(), |ui| {
        for id in &mut ids {
            *id = ui.text(b"X", Style::new().grow(1));
        }
    });
    finish(&mut ui, 10, 12);
    let rects: Vec<_> = ids.iter().map(|id| ui.rect(*id)).collect();
    assert_eq!(rects[0].x, 0);
    assert_eq!(rects[1].x, rects[0].right(), "shared edges land together");
    assert_eq!(rects[2].x, rects[1].right());
    assert_eq!(rects[2].right(), 10);
}

#[test]
fn a_stretched_text_wraps_inside_its_box() {
    let mut ui = page(60, 100);
    let mut id = NodeId::NONE;
    ui.column(Style::new().stretch(), |ui| {
        id = ui.text(b"AAAA BBBB CCCC", Style::new());
    });
    finish(&mut ui, 60, 100);
    let rect = ui.rect(id);
    assert_eq!(rect.w, 60);
    assert!(rect.h >= 24, "the run wrapped onto at least two lines");
}

#[test]
fn the_same_declaration_lays_out_the_same_way_twice() {
    fn build(ui: &mut Ui<'_, '_>) -> [NodeId; 2] {
        let mut ids = [NodeId::NONE; 2];
        ui.row(Style::new().stretch(), |ui| {
            ids[0] = ui.text(b"AB", Style::new().width(20));
            ids[1] = ui.text(b"CD", Style::new().grow(1));
        });
        ids
    }
    let mut first = page(100, 20);
    let ids = build(&mut first);
    finish(&mut first, 100, 20);
    let mut second = page(100, 20);
    build(&mut second);
    finish(&mut second, 100, 20);
    for id in ids {
        assert_eq!(first.rect(id), second.rect(id));
    }
}

#[test]
fn a_tag_finds_the_box_a_finger_landed_on() {
    let mut ui = page(100, 100);
    let mut id = NodeId::NONE;
    ui.stack(Style::new(), |ui| {
        id = ui.node(Style::new().absolute().left(10).top(20).size(30, 40).tag(7));
    });
    finish(&mut ui, 100, 100);
    assert_eq!(ui.tag_at(20, 30), Some(7));
    assert_eq!(ui.tag_at(5, 5), None);
    let _ = id;
}

#[test]
fn justify_spreads_the_leftover_between_the_children() {
    let mut ui = page(100, 12);
    let mut ids = [NodeId::NONE; 2];
    ui.row(Style::new().between(), |ui| {
        ids[0] = ui.text(b"A", Style::new().width(10).nowrap());
        ids[1] = ui.text(b"B", Style::new().width(10).nowrap());
    });
    finish(&mut ui, 100, 12);
    assert_eq!(ui.rect(ids[1]).x, 90);
}

#[test]
fn align_center_puts_a_child_in_the_middle_of_the_cross_axis() {
    let mut ui = page(100, 100);
    let mut id = NodeId::NONE;
    ui.column(Style::new().middle(), |ui| {
        id = ui.node(Style::new().size(20, 10));
    });
    finish(&mut ui, 100, 100);
    assert_eq!(ui.rect(id).x, 40);
}
