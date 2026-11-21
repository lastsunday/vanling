use super::*;
use crate::ui::font::Metrics;
use crate::ui::{Screen, Ui};

fn page(width: i16, height: i16) -> Ui<'static, 'static> {
    Ui::headless(width as usize, height as usize, Screen::default())
}

fn finish(ui: &mut Ui<'_, '_>, width: i16, height: i16) {
    ui.layout(BoxConstraints::tight(
        i32::from(width) * PX,
        i32::from(height) * PX,
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
fn a_wrapping_row_keeps_a_basis_child_its_natural_size() {
    let mut ui = page(100, 100);
    let mut ids = [NodeId::NONE; 2];
    ui.wrap(Style::new().row(), |ui| {
        ids[0] = ui.text(b"XXXXXX", Style::new().width(40).basis(0).nowrap());
        ids[1] = ui.text(b"XXXXXX", Style::new().width(40).nowrap());
    });
    finish(&mut ui, 100, 100);
    assert_eq!(
        ui.rect(ids[0]).w,
        40,
        "a wrapping flex still needs the natural size"
    );
    assert_eq!(ui.rect(ids[1]).x, 40, "both still fit on the first run");
}

#[test]
fn a_zero_width_scroll_measures_no_content() {
    let mut tree = Tree::new();
    let area = ScrollId::new(1);
    let root = tree.push(Style::new().stretch(), NodeKind::Box, NodeId::NONE);
    let scroll = tree.push(
        Style::new().scrolling().scroll_area(area),
        NodeKind::Box,
        root,
    );
    let run = b"AAAAA AAAAA AAAAA";
    tree.push(
        Style::new(),
        NodeKind::Text {
            start: 0,
            len: run.len() as u16,
        },
        scroll,
    );
    tree.layout(BoxConstraints::tight(0, 40 * PX), run);
    assert_eq!(tree.scroll_info(area).unwrap().content, 0);
    tree.layout(BoxConstraints::tight(30 * PX, 40 * PX), run);
    assert!(tree.scroll_info(area).unwrap().content > 0);
}

#[test]
fn a_text_node_re_measures_when_the_width_changes() {
    let mut tree = Tree::new();
    let root = tree.push(Style::new().stretch(), NodeKind::Box, NodeId::NONE);
    let run = b"AAAAA AAAAA";
    let text = tree.push(
        Style::new(),
        NodeKind::Text {
            start: 0,
            len: run.len() as u16,
        },
        root,
    );
    tree.layout(BoxConstraints::tight(30 * PX, 40 * PX), run);
    assert_eq!(tree.rect(text).h, 24, "at 30 the run takes two lines");
    tree.layout(BoxConstraints::tight(72 * PX, 40 * PX), run);
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
    ui.scroll(Style::new().height(20).scroll_offset(10), |ui| {
        child = ui.node(Style::new().height(100));
    });
    finish(&mut ui, 100, 20);
    assert_eq!(ui.rect(child).y, -10);
    assert!(ui.clipped(child), "a scroll box clips its content");
}

#[test]
fn a_named_scroll_box_reports_where_it_sits_and_how_far_it_goes() {
    let mut ui = page(100, 20);
    let mut child = NodeId::NONE;
    ui.scroll(
        Style::new()
            .height(20)
            .scroll_area(ScrollId::new(1))
            .scroll_offset(10),
        |ui| {
            child = ui.node(Style::new().height(100));
        },
    );
    finish(&mut ui, 100, 20);
    let info = ui.scroll_info(ScrollId::new(1)).expect("the area is named");
    assert_eq!(info.rect, crate::ui::Rect::new(0, 0, 100, 20));
    assert_eq!(info.offset, 10);
    assert_eq!(info.viewport, 20);
    assert_eq!(info.content, 100);
    assert_eq!(info.max(), 80, "the travel is what the content overflows");
    assert_eq!(ui.rect(child).y, -10);
}

#[test]
fn a_named_scroll_box_in_a_stretched_row_takes_the_rows_height() {
    let mut ui = page(100, 40);
    ui.row(Style::new().grow(1).basis(0).stretch().gap(2), |ui| {
        ui.scroll(Style::new().grow(1).scroll_area(ScrollId::new(2)), |ui| {
            ui.node(Style::new().height(100));
        });
        ui.stack(Style::new().width(2), |_ui| {});
    });
    finish(&mut ui, 100, 40);
    let info = ui.scroll_info(ScrollId::new(2)).expect("the area is named");
    assert_eq!(info.rect, crate::ui::Rect::new(0, 0, 96, 40));
    assert_eq!(info.viewport, 40);
    assert_eq!(info.max(), 60);
}

#[test]
fn the_declared_scroll_offset_is_not_clamped() {
    let mut ui = page(100, 20);
    let mut child = NodeId::NONE;
    ui.scroll(
        Style::new()
            .height(20)
            .scroll_area(ScrollId::new(1))
            .scroll_offset(90),
        |ui| {
            child = ui.node(Style::new().height(100));
        },
    );
    finish(&mut ui, 100, 20);
    assert_eq!(
        ui.rect(child).y,
        -90,
        "the page owns the clamp, because only it knows its bounce"
    );
    assert_eq!(ui.scroll_info(ScrollId::new(1)).unwrap().offset, 90);
}

#[test]
fn a_scroll_box_stacks_its_children_on_the_scrolling_axis() {
    let mut ui = page(100, 40);
    let mut ids = [NodeId::NONE; 2];
    ui.scroll(
        Style::new().height(40).gap(2).scroll_area(ScrollId::new(1)),
        |ui| {
            ids[0] = ui.node(Style::new().height(10));
            ids[1] = ui.node(Style::new().height(10));
        },
    );
    finish(&mut ui, 100, 40);
    assert_eq!(ui.rect(ids[0]).y, 0);
    assert_eq!(ui.rect(ids[1]).y, 12, "the gap separates the rows");
    assert_eq!(ui.scroll_info(ScrollId::new(1)).unwrap().content, 22);
}

#[test]
fn a_scrolled_box_shifts_every_child_by_the_same_offset() {
    let mut ui = page(100, 40);
    let mut ids = [NodeId::NONE; 2];
    ui.scroll(
        Style::new()
            .height(40)
            .gap(2)
            .scroll_offset(5)
            .scroll_area(ScrollId::new(1)),
        |ui| {
            ids[0] = ui.node(Style::new().height(10));
            ids[1] = ui.node(Style::new().height(10));
        },
    );
    finish(&mut ui, 100, 40);
    assert_eq!(ui.rect(ids[0]).y, -5);
    assert_eq!(ui.rect(ids[1]).y, 7, "the order survives the shift");
}

#[test]
fn a_nowrap_run_takes_the_width_it_is_given() {
    let mut ui = page(100, 20);
    let mut id = NodeId::NONE;
    ui.column(Style::new(), |ui| {
        id = ui.text(b"AAAAA", Style::new().nowrap().width(30));
    });
    finish(&mut ui, 100, 20);
    assert_eq!(ui.rect(id).w, 30);
    assert_eq!(ui.rect(id).h, 12);
}

#[test]
fn a_percentage_resolves_against_the_parent() {
    let mut ui = page(100, 20);
    let mut id = NodeId::NONE;
    ui.column(Style::new().stretch(), |ui| {
        ui.node(Style::new().width(0).height(10));
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

#[test]
fn a_box_re_measures_when_only_the_height_changes() {
    let mut tree = Tree::new();
    let root = tree.push(Style::new().stretch(), NodeKind::Box, NodeId::NONE);
    let child = tree.push(Style::new().grow(1), NodeKind::Box, root);
    tree.layout(BoxConstraints::tight(100 * PX, 20 * PX), b"");
    assert_eq!(tree.rect(child).h, 20);
    tree.layout(BoxConstraints::tight(100 * PX, 40 * PX), b"");
    assert_eq!(tree.rect(child).h, 40, "a height-only change re-measures");
}

#[test]
fn a_nowrap_run_stays_on_one_line() {
    let mut ui = page(30, 40);
    let mut id = NodeId::NONE;
    ui.column(Style::new(), |ui| {
        id = ui.text(b"ABCDEFGH", Style::new().width(30).nowrap());
    });
    finish(&mut ui, 30, 40);
    assert_eq!(ui.rect(id).h, 12);
}

#[test]
fn padding_insets_a_run_without_being_counted_as_ink() {
    let mut ui = page(100, 40);
    let mut id = NodeId::NONE;
    ui.column(Style::new(), |ui| {
        id = ui.text(b"AB", Style::new().padding(2, 3, 4, 5));
    });
    finish(&mut ui, 100, 40);
    assert_eq!((ui.rect(id).w, ui.rect(id).h), (18, 20));
}

#[test]
fn a_percentage_scales_the_parent_rather_than_a_hundredth_of_it() {
    let mut ui = page(150, 12);
    let mut id = NodeId::NONE;
    ui.column(Style::new(), |ui| {
        let mut style = Style::new();
        style.width = Length::Percent(50);
        style.height = Length::Px(12);
        id = ui.node(style);
    });
    finish(&mut ui, 150, 12);
    assert_eq!(ui.rect(id).w, 75);
}

#[test]
fn a_basis_starts_a_flexible_child_at_its_own_size() {
    let mut ui = page(100, 12);
    let mut ids = [NodeId::NONE; 2];
    ui.row(Style::new(), |ui| {
        ids[0] = ui.node(Style::new().basis(40).grow(1));
        ids[1] = ui.node(Style::new().grow(1));
    });
    finish(&mut ui, 100, 12);
    assert_eq!(ui.rect(ids[0]).w, 70);
    assert_eq!(ui.rect(ids[1]).w, 30);
}

#[test]
fn a_wrapping_row_stretches_each_run_to_its_widest_child() {
    let mut ui = page(100, 100);
    let mut ids = [NodeId::NONE; 2];
    ui.wrap(Style::new().row().stretch(), |ui| {
        ids[0] = ui.text(b"A", Style::new().width(20).nowrap());
        ids[1] = ui.column(Style::new().width(20), |ui| {
            ui.text(b"B", Style::new().nowrap());
            ui.text(b"C", Style::new().nowrap());
        });
    });
    finish(&mut ui, 100, 100);
    assert_eq!(ui.rect(ids[0]).h, 24);
    assert_eq!(ui.rect(ids[1]).h, 24);
}

#[test]
fn dp_is_five_quarters_of_a_pixel_and_rem_is_the_root_text_size() {
    let mut ui = page(100, 100);
    let mut ids = [NodeId::NONE; 2];
    ui.column(Style::new(), |ui| {
        let mut dp = Style::new();
        dp.width = Length::Dp(8);
        dp.height = Length::Dp(8);
        ids[0] = ui.node(dp);
        let mut rem = Style::new();
        rem.width = Length::Rem(100);
        rem.height = Length::Rem(100);
        ids[1] = ui.node(rem);
    });
    finish(&mut ui, 100, 100);
    assert_eq!((ui.rect(ids[0]).w, ui.rect(ids[0]).h), (10, 10));
    assert_eq!((ui.rect(ids[1]).w, ui.rect(ids[1]).h), (12, 12));
    assert_eq!(
        core::mem::size_of::<Length>(),
        4,
        "a unit is still one word"
    );
}

#[test]
fn the_three_colour_slots_cost_three_bytes() {
    let bytes = core::mem::size_of::<Style>();
    assert!(bytes <= 84, "a style is {bytes} B");
}

#[test]
fn a_font_size_sets_the_line_box_and_stretches_each_cell() {
    let mut ui = page(200, 100);
    let mut ids = [NodeId::NONE; 3];
    ui.column(Style::new(), |ui| {
        ids[0] = ui.text(b"A", Style::new().nowrap().font_size(Length::Px(12)));
        ids[1] = ui.text(b"A", Style::new().nowrap().font_size(Length::Px(24)));
        ids[2] = ui.text(b"A", Style::new().nowrap().font_size(Length::Rem(200)));
    });
    finish(&mut ui, 200, 100);
    assert_eq!(ui.rect(ids[0]).h, 12);
    assert_eq!((ui.rect(ids[1]).w, ui.rect(ids[1]).h), (12, 24));
    assert_eq!(
        (ui.rect(ids[2]).w, ui.rect(ids[2]).h),
        (12, 24),
        "2 rem is the 24 px a 200 percent root size means"
    );
}

#[test]
fn a_larger_font_size_breaks_a_run_into_more_lines() {
    let run = "中文中文".as_bytes();
    let small = typeset::wrap(run, 24, typeset::MAX_LINES, Metrics::default());
    let large = typeset::wrap(run, 24, typeset::MAX_LINES, Metrics::at(24));
    assert_eq!(small.count, 2);
    assert_eq!(large.count, 4, "each cell is twice as wide at 24 px");
}
