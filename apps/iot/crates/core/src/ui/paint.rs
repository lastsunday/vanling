//! The fg: what each laid-out node puts on the canvas.

use super::canvas::Canvas;
use super::color::{Color, Palette};
use super::font::Metrics;
use super::geometry::Rect;
use super::layout::{
    Decoration, DecorationStyle, NodeInfo, NodeKind, Overflow, Span, Style, TextAlign,
};
use super::typeset;

/// What one step of a line's walk hands its caller.
enum Step {
    Glyph(char, i32, Style),
    Segment(Style, i32, i32),
}

/// Draws one laid-out node: its run, its paint leaf, or its own paper.
pub(crate) fn node(
    canvas: &mut Canvas<'_>,
    info: NodeInfo,
    text: &[u8],
    palette: &Palette,
    spans: &[Span],
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
    let color = palette.get(info.style.color);
    match info.kind {
        NodeKind::Text { start, len } => {
            paper(canvas, info, palette);
            let run = &text[usize::from(start)..usize::from(start + len)];
            draw_text(canvas, start, run, content, info.style, palette, spans);
        }
        NodeKind::Paint { slot } => {
            canvas.set_color(color);
            paint(slot, canvas, content);
            canvas.set_color(None);
        }
        NodeKind::Box => paper(canvas, info, palette),
    }
    if info.clipped {
        canvas.pop_clip();
    }
}

/// Lays the node's own paper down: its background, or the ground turned over, inside its outline.
fn paper(canvas: &mut Canvas<'_>, info: NodeInfo, palette: &Palette) {
    if let Some(background) = palette.get(info.style.background_color) {
        canvas.set_color(Some(background));
        canvas.fill_round_rect(info.rect, info.style.radius);
        canvas.set_color(None);
    } else if info.style.has_fill() {
        canvas.fill_round_rect(info.rect, info.style.radius);
    }
    if info.style.has_border() {
        canvas.set_color(palette.get(info.style.color));
        canvas.stroke_round_rect(info.rect, info.style.radius);
        canvas.set_color(None);
    }
}

fn draw_text(
    canvas: &mut Canvas<'_>,
    start: u16,
    run: &[u8],
    rect: Rect,
    style: Style,
    palette: &Palette,
    spans: &[Span],
) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    // One face per box: a run the vector table cannot cover falls back whole, so a line is
    // never half outlines and half pixels.
    let metrics = style.metrics().covering(run);
    let line_height = typeset::line_height(metrics) as i32;
    if style.nowrap {
        // One line at its natural width; whatever leaves the box sideways is clipped, not
        // wrapped. The box's own height is not a clip: a descender reaches below it, and only
        // the ancestor (a scroll viewport, or the page) decides where the fg stops.
        let width = metrics.measure(run) as i32;
        let top = rect.y + (rect.h - line_height).max(0) / 2;
        if rect.x < 0 || top < 0 {
            return;
        }
        if width <= rect.w || style.overflow == Overflow::Clip {
            let left = match style.text_align {
                TextAlign::Center => rect.x + (rect.w - width).max(0) / 2,
                TextAlign::End => rect.x + (rect.w - width).max(0),
                _ => rect.x,
            };
            canvas.push_x_clip(rect);
            paint_run(
                canvas, start, run, left, top, metrics, style, palette, spans, 0,
            );
            canvas.pop_clip();
            return;
        }
        // The run is wider than its box and asked for the mark: spell the cut.
        let room = rect.w.max(1) as usize;
        let ellipsis = metrics.advance('…');
        let paragraph = typeset::wrap(run, room, 1, metrics);
        let line = paragraph.lines[0];
        let kept = &run[line.start..line.end];
        let used = metrics.measure(kept) as i32 + ellipsis as i32;
        let left = match style.text_align {
            TextAlign::Center => rect.x + (rect.w - used).max(0) / 2,
            TextAlign::End => rect.x + (rect.w - used).max(0),
            _ => rect.x,
        };
        canvas.push_x_clip(rect);
        let end = paint_run(
            canvas,
            start + line.start as u16,
            kept,
            left,
            top,
            metrics,
            style,
            palette,
            spans,
            0,
        );
        let fg = palette.get(style.color);
        draw_glyph(canvas, '…', end, top, metrics, style, fg);
        canvas.pop_clip();
        return;
    }
    // A left-aligned run that fits needs no breaking: the panel's pages are mostly readings,
    // and running the breaker over one of them every frame measures the same run twice.
    if style.text_align == TextAlign::Start
        && style.max_lines >= 1
        && !run.contains(&b'\n')
        && !run.contains(&b'\r')
    {
        let width = metrics.measure(run) as i32;
        let top = rect.y + (rect.h - line_height).max(0) / 2;
        if width <= rect.w && rect.x >= 0 && top >= 0 {
            paint_run(
                canvas, start, run, rect.x, top, metrics, style, palette, spans, 0,
            );
            return;
        }
    }
    let paragraph = typeset::wrap(run, rect.w as usize, usize::from(style.max_lines), metrics);
    let height = paragraph.count as i32 * line_height;
    let top = rect.y + (rect.h - height).max(0) / 2;
    for (index, line) in paragraph.lines[..paragraph.count].iter().enumerate() {
        let last = index + 1 == paragraph.count;
        let x = match style.text_align {
            TextAlign::Center => rect.x + (rect.w - line.width as i32) / 2,
            TextAlign::End => rect.x + rect.w - line.width as i32,
            _ => rect.x,
        };
        let gaps = line.gaps.max(1) as i32;
        let extra = if style.text_align == TextAlign::Justify && !last {
            (rect.w - line.width as i32).max(0) / gaps
        } else {
            0
        };
        let y = top + index as i32 * line_height;
        let line_start = start + line.start as u16;
        let end = paint_run(
            canvas,
            line_start,
            &run[line.start..line.end],
            x,
            y,
            metrics,
            style,
            palette,
            spans,
            extra,
        );
        if paragraph.truncated && last {
            let ellipsis = metrics.advance('…') as i32;
            let fg = palette.get(style.color);
            draw_glyph(canvas, '…', end + extra, y, metrics, style, fg);
            draw_decorations(
                canvas,
                style,
                palette.get(style.decoration_color).or(fg),
                end + extra,
                end + extra + ellipsis,
                y,
                metrics,
            );
        }
    }
}

/// Draws one line, span by span, and returns the pen after its last glyph.
#[allow(clippy::too_many_arguments)]
fn paint_run(
    canvas: &mut Canvas<'_>,
    start: u16,
    run: &[u8],
    left: i32,
    top: i32,
    metrics: Metrics,
    style: Style,
    palette: &Palette,
    spans: &[Span],
    extra: i32,
) -> i32 {
    if has_paper(style, spans) {
        walk(
            canvas,
            start,
            run,
            left,
            metrics,
            style,
            spans,
            extra,
            |canvas, step| {
                if let Step::Segment(style, left, right) = step
                    && let Some(color) = palette.get(style.background_color)
                {
                    highlight(canvas, color, left, right, top, metrics.height() as i32);
                }
            },
        );
    }
    walk(
        canvas,
        start,
        run,
        left,
        metrics,
        style,
        spans,
        extra,
        |canvas, step| match step {
            Step::Glyph(ch, pen, run_style) => {
                let fg = palette.get(run_style.color);
                draw_glyph(canvas, ch, pen, top, metrics, run_style, fg);
            }
            Step::Segment(run_style, left, right) => {
                let fg = palette.get(run_style.color);
                let decoration = palette.get(run_style.decoration_color).or(fg);
                draw_decorations(canvas, run_style, decoration, left, right, top, metrics);
            }
        },
    )
}

/// Walks a run, handing each glyph and each same-styled stretch to `step`. A stretch ends
/// where a span begins or ends, so decorations stop and start with the colour.
#[allow(clippy::too_many_arguments)]
fn walk(
    canvas: &mut Canvas<'_>,
    start: u16,
    run: &[u8],
    left: i32,
    metrics: Metrics,
    style: Style,
    spans: &[Span],
    extra: i32,
    mut step: impl FnMut(&mut Canvas<'_>, Step),
) -> i32 {
    let mut at = 0;
    let mut pen = left;
    let mut segment = left;
    let mut held = effective(style, spans, start);
    while at < run.len() {
        let Some((ch, len)) = typeset::next_char(run, at) else {
            break;
        };
        let next = effective(style, spans, start + (at + len) as u16);
        step(canvas, Step::Glyph(ch, pen, held));
        pen += metrics.advance(ch) as i32;
        if ch == ' ' {
            pen += extra;
        }
        at += len;
        if !same(held, next) {
            step(canvas, Step::Segment(held, segment, pen));
            held = next;
            segment = pen;
        }
    }
    step(canvas, Step::Segment(held, segment, pen));
    pen
}

/// The node's style with the span covering `at` laid over it.
fn effective(node: Style, spans: &[Span], at: u16) -> Style {
    match spans.iter().find(|span| at >= span.start && at < span.end) {
        Some(span) => span.style.over(node),
        None => node,
    }
}

fn same(a: Style, b: Style) -> bool {
    a.color == b.color
        && a.decoration_color == b.decoration_color
        && a.background_color == b.background_color
        && a.decorations() == b.decorations()
        && a.decoration_style_of() == b.decoration_style_of()
}

fn has_paper(style: Style, spans: &[Span]) -> bool {
    style.background_color != super::color::ColorId::NONE
        || spans
            .iter()
            .any(|span| span.style.background_color != super::color::ColorId::NONE)
}

/// One glyph, widened once over itself when the weight is heavy.
fn draw_glyph(
    canvas: &mut Canvas<'_>,
    ch: char,
    left: i32,
    top: i32,
    metrics: Metrics,
    style: Style,
    fg: Option<Color>,
) {
    canvas.set_color(fg);
    if style.weight().is_heavy() {
        canvas.draw_char_heavy_at(ch, left as isize, top as usize, metrics);
    } else {
        canvas.draw_char_at(ch, left as isize, top as usize, metrics);
    }
}

/// The paper behind one stretch of a line.
fn highlight(canvas: &mut Canvas<'_>, color: Color, left: i32, right: i32, top: i32, height: i32) {
    canvas.set_color(Some(color));
    for y in top..top + height {
        for x in left..right {
            canvas.plot_pixel(x as isize, y as isize);
        }
    }
    canvas.set_color(None);
}

/// Underline, overline and strike-through across the fg between `left` and `right`.
fn draw_decorations(
    canvas: &mut Canvas<'_>,
    style: Style,
    color: Option<Color>,
    left: i32,
    right: i32,
    top: i32,
    metrics: Metrics,
) {
    let decoration = style.decorations();
    if decoration == Decoration::NONE || right <= left {
        return;
    }
    canvas.set_color(color);
    let ascent = metrics.ascent() as i32;
    let baseline = top + ascent;
    let thickness = i32::from(style.thick());
    for (line, y) in [
        (Decoration::UNDERLINE, baseline + 1),
        (Decoration::OVERLINE, top),
        (Decoration::LINE_THROUGH, baseline - ascent / 2),
    ] {
        if decoration.has(line) {
            draw_rule(
                canvas,
                left,
                right,
                y,
                style.decoration_style_of(),
                thickness,
            );
        }
    }
    canvas.set_color(None);
}

fn draw_rule(
    canvas: &mut Canvas<'_>,
    left: i32,
    right: i32,
    y: i32,
    style: DecorationStyle,
    thickness: i32,
) {
    let rows: &[i32] = if style == DecorationStyle::Double {
        &[y, y + 2]
    } else {
        &[y]
    };
    for &row in rows {
        for step in 0..thickness {
            let mut x = left;
            while x < right {
                let on = match style {
                    DecorationStyle::Dotted => (x - left) % 2 == 0,
                    DecorationStyle::Dashed => ((x - left) / 4) % 2 == 0,
                    _ => true,
                };
                let drop = if style == DecorationStyle::Wavy {
                    i32::from(((x - left) / 2) % 2 != 0)
                } else {
                    0
                };
                if on {
                    canvas.plot_pixel(x as isize, (row + step + drop) as isize);
                }
                x += 1;
            }
        }
    }
}
