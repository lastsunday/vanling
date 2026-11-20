//! The ink: what each laid-out node puts on the canvas.

use super::canvas::Canvas;
use super::geometry::Rect;
use super::layout::{NodeInfo, NodeKind, TextAlign};
use super::typeset;

/// Draws one laid-out node: its run, its paint leaf, or its own paper.
pub(crate) fn node(
    canvas: &mut Canvas<'_>,
    info: NodeInfo,
    text: &[u8],
    paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect),
) {
    if info.clipped {
        canvas.push_clip(info.clip);
    }
    let padding = info.style.padding;
    let content = info.rect.inset(
        i32::from(padding.left),
        i32::from(padding.top),
        i32::from(padding.right),
        i32::from(padding.bottom),
    );
    match info.kind {
        NodeKind::Text { start, len } => {
            let run = &text[usize::from(start)..usize::from(start + len)];
            draw_text(
                canvas,
                run,
                content,
                info.style.text_align,
                info.style.nowrap,
                info.style.max_lines,
            );
        }
        NodeKind::Paint { slot } => paint(slot, canvas, content),
        NodeKind::Box => {
            if info.style.fill {
                canvas.fill_rect(info.rect);
            }
            if info.style.border {
                canvas.stroke_rect(info.rect);
            }
        }
    }
    if info.clipped {
        canvas.pop_clip();
    }
}

fn draw_text(
    canvas: &mut Canvas<'_>,
    run: &[u8],
    rect: Rect,
    align: TextAlign,
    nowrap: bool,
    max_lines: u16,
) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let line_height = typeset::line_height() as i32;
    if nowrap {
        // One line at its natural width; whatever leaves the box is clipped, not wrapped.
        let top = rect.y + (rect.h - line_height).max(0) / 2;
        if rect.x >= 0 && top >= 0 {
            canvas.push_clip(rect);
            canvas.draw_text(run, rect.x as usize, top as usize);
            canvas.pop_clip();
        }
        return;
    }
    // A left-aligned run that fits needs no breaking: the panel's pages are mostly readings,
    // and running the breaker over one of them every frame measures the same run twice.
    if align == TextAlign::Start && max_lines >= 1 && !run.contains(&b'\n') && !run.contains(&b'\r')
    {
        let width = canvas.measure(run);
        let top = rect.y + (rect.h - line_height).max(0) / 2;
        if width <= rect.w as usize && rect.x >= 0 && top >= 0 {
            canvas.draw_text(run, rect.x as usize, top as usize);
            return;
        }
    }
    let paragraph = typeset::wrap(run, rect.w as usize, usize::from(max_lines));
    let height = paragraph.count as i32 * line_height;
    let top = rect.y + (rect.h - height).max(0) / 2;
    for (index, line) in paragraph.lines[..paragraph.count].iter().enumerate() {
        let last = index + 1 == paragraph.count;
        let mut x = match align {
            TextAlign::Center => rect.x + (rect.w - line.width as i32) / 2,
            TextAlign::End => rect.x + rect.w - line.width as i32,
            _ => rect.x,
        };
        let gaps = line.gaps.max(1) as i32;
        let extra = if align == TextAlign::Justify && !last {
            (rect.w - line.width as i32).max(0) / gaps
        } else {
            0
        };
        let y = top + index as i32 * line_height;
        let mut at = line.start;
        while at < line.end {
            let Some((ch, len)) = typeset::next_char(run, at) else {
                break;
            };
            canvas.draw_char(ch, x as isize, y as usize);
            x += super::font::Face::new().advance(ch) as i32;
            if ch == ' ' {
                x += extra;
            }
            at += len;
        }
        if paragraph.truncated && last {
            canvas.draw_char('…', x as isize, y as usize);
        }
    }
}
