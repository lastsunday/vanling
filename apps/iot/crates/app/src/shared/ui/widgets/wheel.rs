//! One vertical wheel: arbitrary labels, drag, momentum, snap, optional wrap.
//!
//! Every geometry comes from the rect the parent hands in; drawing and hit testing share
//! [`Metrics`], so the row a finger lands on is the row drawn under it.

use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const MIN_ROW_H: i16 = 28;
pub const MAX_ROW_H: i16 = 44;
pub const MIN_VISIBLE: i32 = 3;
pub const MAX_VISIBLE: i32 = 7;

/// The least release speed that carries momentum, in px/s.
pub const FLING_MIN_PX_S: i16 = 250;

/// How quickly a release bleeds off, in px/s².
pub const ACCEL_PX_S2: i32 = 2400;

/// The longest a fling may travel before snapping, in rows.
const MAX_TRAVEL_ROWS: i32 = 6;

/// The ease that takes the wheel from its release point to the chosen row.
const EASE_MS: i32 = 120;

/// Row units per row: a Q8.8 fixed point so sub-row travel survives without floats.
const UNIT: i32 = 256;

/// What a wheel shows: any fixed run of labels, or a numeric range with a step and zero pad.
#[derive(Clone, Copy, Debug)]
pub enum Labels<'a> {
    Fixed(&'a [&'a [u8]]),
    Numbers {
        min: u16,
        max: u16,
        step: u16,
        pad: u8,
    },
}

impl Labels<'_> {
    pub fn count(self) -> u16 {
        match self {
            Self::Fixed(items) => u16::try_from(items.len()).unwrap_or(u16::MAX),
            Self::Numbers { min, max, step, .. } if step > 0 && max >= min => {
                (max - min) / step + 1
            }
            Self::Numbers { .. } => 0,
        }
    }

    /// The number a row stands for; fixed labels answer their index.
    pub fn value(self, index: u16) -> u16 {
        match self {
            Self::Numbers { min, step, .. } => min.saturating_add(index.saturating_mul(step)),
            Self::Fixed(_) => index,
        }
    }

    /// Writes one row's glyphs into `buf`, cutting a fixed run that outgrows it.
    pub fn write(self, index: u16, buf: &mut [u8; 16]) -> &[u8] {
        match self {
            Self::Fixed(items) => {
                let run = items.get(usize::from(index)).copied().unwrap_or(b"");
                let len = run.len().min(buf.len());
                buf[..len].copy_from_slice(&run[..len]);
                &buf[..len]
            }
            Self::Numbers { pad, .. } => {
                let mut digits = [0u8; 6];
                let text = format_u16(self.value(index), &mut digits);
                let width = usize::from(pad).max(text.len()).min(buf.len());
                let lead = width - text.len();
                buf[..lead].fill(b'0');
                buf[lead..width].copy_from_slice(text);
                &buf[..width]
            }
        }
    }
}

/// The part of a wheel a frame draws, kept `Copy` for the snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WheelView {
    /// The centre row in Q8.8 row units.
    pub position: i32,
    pub count: u16,
    pub wrap: bool,
    pub moving: bool,
}

impl WheelView {
    pub fn index(self) -> u16 {
        map_index(round_div(self.position, UNIT), self.count, self.wrap)
    }
}

/// A wheel's live state: where it rests, where it is going, and the finger that moves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WheelState {
    position: i32,
    target: i32,
    count: u16,
    wrap: bool,
    held: bool,
}

impl Default for WheelState {
    fn default() -> Self {
        Self::new(0, 1, false)
    }
}

impl WheelState {
    pub fn new(index: u16, count: u16, wrap: bool) -> Self {
        let count = count.max(1);
        let index = index.min(count - 1);
        let position = i32::from(index) * UNIT;
        Self {
            position,
            target: position,
            count,
            wrap,
            held: false,
        }
    }

    pub fn view(self) -> WheelView {
        WheelView {
            position: self.position,
            count: self.count,
            wrap: self.wrap,
            moving: self.moving(),
        }
    }

    pub fn index(self) -> u16 {
        map_index(round_div(self.position, UNIT), self.count, self.wrap)
    }

    pub fn count(self) -> u16 {
        self.count
    }

    pub fn moving(self) -> bool {
        !self.held && self.position != self.target
    }

    /// Rebuilds the wheel for a new range, keeping the row it shows when that row still exists.
    pub fn set_count(&mut self, count: u16) {
        self.count = count.max(1);
        if self.wrap {
            self.normalise();
        } else if self.index() >= self.count {
            let last = i32::from(self.count - 1) * UNIT;
            self.position = last;
            self.target = last;
        }
    }

    /// Rebuilds the wheel for a range that may have shrunk, keeping the nearest row inside it.
    pub fn clamp_count(&mut self, count: u16) {
        let count = count.max(1);
        if !self.wrap {
            self.set_count(count);
            return;
        }
        self.count = count;
        let last = i32::from(count - 1) * UNIT;
        self.position = self.position.clamp(-UNIT / 2, last);
        self.target = self.target.clamp(-UNIT / 2, last);
    }

    pub fn set_index(&mut self, index: u16) {
        let index = map_index(i32::from(index), self.count, self.wrap);
        self.position = i32::from(index) * UNIT;
        self.target = self.position;
    }

    /// Stops a glide under the finger.
    pub fn press(&mut self) {
        self.held = true;
        self.target = self.position;
    }

    /// Moves the wheel `dy` pixels with the finger down; past an end it gives a half row.
    pub fn drag(&mut self, dy: i16, row_h: i16) {
        self.held = true;
        self.target = self.position;
        let delta = i32::from(dy) * UNIT / i32::from(row_h.max(1));
        self.position += delta;
        if self.wrap {
            self.normalise();
        } else {
            let last = i32::from(self.count - 1) * UNIT;
            self.position = self.position.clamp(-UNIT / 2, last + UNIT / 2);
        }
    }

    /// Lets go at `speed` px/s: a fling projects a landing row, a slow lift takes the nearest.
    pub fn release(&mut self, speed: i16, row_h: i16) {
        self.held = false;
        let row_h = i32::from(row_h.max(1));
        let speed = i32::from(speed);
        let speed = if speed.unsigned_abs() >= FLING_MIN_PX_S as u32 {
            speed
        } else {
            0
        };
        let travel_px = if speed == 0 {
            0
        } else {
            (speed * speed / (2 * ACCEL_PX_S2)).min(MAX_TRAVEL_ROWS * row_h)
        };
        let travel = speed.signum() * travel_px * UNIT / row_h;
        let landing = round_div(self.position + travel, UNIT);
        self.target = match self.wrap {
            true => landing * UNIT,
            false => landing.clamp(0, i32::from(self.count - 1)) * UNIT,
        };
        if self.target == self.position {
            self.settle();
        }
    }

    /// Picks the row `offset` rows from the centre, as a tap on that row asks.
    pub fn tap_row(&mut self, offset: i32) {
        let landing = round_div(self.position, UNIT) + offset;
        let landing = map_index(landing, self.count, self.wrap);
        self.position = i32::from(landing) * UNIT;
        self.target = self.position;
        self.held = false;
    }

    /// Advances the ease one frame.
    pub fn tick(&mut self, step_ms: u16) {
        if self.held || self.position == self.target {
            return;
        }
        let delta = self.target - self.position;
        let step = (delta.abs() * i32::from(step_ms.clamp(1, 1_000)) / EASE_MS)
            .max(UNIT / 4)
            .min(delta.abs());
        self.position += step * delta.signum();
        if self.position == self.target {
            self.settle();
        }
    }

    fn settle(&mut self) {
        self.position = self.target;
        if self.wrap {
            self.normalise();
        }
    }

    fn normalise(&mut self) {
        let span = i32::from(self.count) * UNIT;
        self.position = self.position.rem_euclid(span);
        self.target = self.target.rem_euclid(span);
    }
}

fn round_div(value: i32, unit: i32) -> i32 {
    if value >= 0 {
        (value + unit / 2) / unit
    } else {
        -((-value + unit / 2) / unit)
    }
}

fn map_index(index: i32, count: u16, wrap: bool) -> u16 {
    let count = i32::from(count.max(1));
    if wrap {
        index.rem_euclid(count) as u16
    } else {
        index.clamp(0, count - 1) as u16
    }
}

/// The row height and band a rect gives the wheel: 3~7 odd rows, 28~44 px each, centred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub row_h: i16,
    pub visible: i32,
    pub top: i32,
    pub centre: i32,
}

impl Metrics {
    /// The fixed selection band: the top and bottom of the centre row.
    pub fn band(self) -> (i32, i32) {
        (self.centre, self.centre + i32::from(self.row_h))
    }
}

pub fn metrics(rect: Rect) -> Metrics {
    let rows = (rect.h / i32::from(MIN_ROW_H)).clamp(MIN_VISIBLE, MAX_VISIBLE);
    let visible = if rows % 2 == 0 { rows - 1 } else { rows };
    let row_h = (rect.h / visible).clamp(1, i32::from(MAX_ROW_H)) as i16;
    let top = rect.y + (rect.h - i32::from(row_h) * visible) / 2;
    let centre = top + (visible / 2) * i32::from(row_h);
    Metrics {
        row_h,
        visible,
        top,
        centre,
    }
}

/// The row offset a panel point picks, or `None` on blank space or outside the range.
pub fn row_at(rect: Rect, view: WheelView, x: i32, y: i32) -> Option<i32> {
    let m = metrics(rect);
    if !rect.contains(x, y) {
        return None;
    }
    let centre = round_div(view.position, UNIT);
    let offset = view.position - centre * UNIT;
    let top = m.centre - offset * i32::from(m.row_h) / UNIT;
    let row = (y - top).div_euclid(i32::from(m.row_h));
    if row.abs() > m.visible / 2 {
        return None;
    }
    let index = centre + row;
    (view.wrap || (0..i32::from(view.count)).contains(&index)).then_some(row)
}

/// The column a panel point picks across `columns` equal shares of `rect`.
pub fn column_at(rect: Rect, columns: u8, x: i32, y: i32) -> Option<u8> {
    if !rect.contains(x, y) || columns == 0 {
        return None;
    }
    let column = (x - rect.x) * i32::from(columns) / rect.w.max(1);
    u8::try_from(column.clamp(0, i32::from(columns) - 1)).ok()
}

pub fn column_rect(rect: Rect, columns: u8, index: u8) -> Rect {
    let columns = i32::from(columns.max(1));
    let index = i32::from(index).clamp(0, columns - 1);
    let left = rect.x + rect.w * index / columns;
    let right = rect.x + rect.w * (index + 1) / columns;
    Rect::new(left, rect.y, right - left, rect.h)
}

pub fn draw(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    view: WheelView,
    labels: Labels<'_>,
    theme: &Theme,
) {
    let m = metrics(rect);
    let centre = round_div(view.position, UNIT);
    let offset = view.position - centre * UNIT;
    let top = m.centre - offset * i32::from(m.row_h) / UNIT;
    canvas.push_clip(rect);
    for row in -(m.visible / 2)..=(m.visible / 2) {
        let index = centre + row;
        if !view.wrap && !(0..i32::from(view.count)).contains(&index) {
            continue;
        }
        let index = map_index(index, view.count, view.wrap);
        let mut buf = [0u8; 16];
        let text = labels.write(index, &mut buf);
        if text.is_empty() {
            continue;
        }
        let width = canvas.measure(text) as i32;
        let y = top + row * i32::from(m.row_h);
        canvas.set_color(Some(if row == 0 { theme.accent } else { theme.muted }));
        canvas.draw_text(
            text,
            (rect.x + (rect.w - width) / 2).max(0) as usize,
            (y + (i32::from(m.row_h) - 12) / 2).max(0) as usize,
        );
    }
    canvas.set_color(Some(theme.muted));
    let (band_top, band_bottom) = m.band();
    for edge in [band_top, band_bottom] {
        canvas.draw_line(
            rect.x as isize,
            edge as isize,
            rect.right() as isize,
            edge as isize,
        );
    }
    canvas.pop_clip();
    canvas.set_color(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(0, 0, 60, 132);

    fn settle(wheel: &mut WheelState) -> u16 {
        for _ in 0..1_000 {
            if !wheel.moving() {
                return wheel.index();
            }
            wheel.tick(20);
        }
        panic!("the wheel never came to rest");
    }

    #[test]
    fn the_geometry_follows_the_rect_it_is_given() {
        let small = metrics(Rect::new(0, 0, 60, 88));
        assert_eq!((small.visible, small.row_h), (3, 29));
        let panel_wheel = metrics(BOX);
        assert_eq!(
            (panel_wheel.visible, panel_wheel.row_h),
            (3, 44),
            "132 px keeps three rows at the cap"
        );
        assert_eq!(
            panel_wheel.band(),
            (panel_wheel.centre, panel_wheel.centre + 44),
            "the band is fixed to the rect, not to the wheel position"
        );
        let panel = metrics(Rect::new(10, 20, 60, 140));
        assert_eq!((panel.visible, panel.row_h), (5, 28));
        assert_eq!(panel.top, 20);
        let tall = metrics(Rect::new(0, 0, 60, 300));
        assert_eq!((tall.visible, tall.row_h), (7, 42));
        assert_eq!(tall.top, 3, "the band centres in the rect");
    }

    #[test]
    fn the_selection_band_stays_put_while_the_wheel_moves() {
        use iot_core::light::rgb565;
        use iot_core::ui::{Canvas, ThemeId};

        let theme = Theme::of(ThemeId::Dark);
        let muted = rgb565(theme.muted.rgb888()).to_be_bytes();
        let rect = Rect::new(0, 0, 56, 132);
        let (band_top, band_bottom) = metrics(rect).band();
        for position in [9 * UNIT, 9 * UNIT + UNIT / 2] {
            let mut frame = [0u8; 56 * 132 * 2];
            {
                let mut canvas = Canvas::new(&mut frame, 56, 132, Some(theme.ground.rgb888()));
                draw(
                    &mut canvas,
                    rect,
                    WheelView {
                        position,
                        count: 24,
                        wrap: true,
                        moving: false,
                    },
                    Labels::Numbers {
                        min: 0,
                        max: 23,
                        step: 1,
                        pad: 2,
                    },
                    &theme,
                );
            }
            for y in [band_top, band_bottom] {
                let i = y as usize * 56 * 2;
                assert_eq!(
                    [frame[i], frame[i + 1]],
                    muted,
                    "the band line at {y} moved with position {position}"
                );
            }
        }
    }

    #[test]
    fn a_tap_picks_the_row_under_the_finger() {
        let view = WheelView {
            position: 5 * UNIT,
            count: 24,
            wrap: true,
            moving: false,
        };
        let m = metrics(BOX);
        for (row, offset) in [(0, 0), (-1, -1), (1, 1)] {
            let y = m.centre + offset * i32::from(m.row_h) + 4;
            assert_eq!(row_at(BOX, view, 10, y), Some(row));
        }
        assert_eq!(row_at(BOX, view, 10, BOX.y - 1), None);
    }

    #[test]
    fn a_fixed_run_answers_its_own_labels_and_a_number_run_its_step() {
        let fixed = Labels::Fixed(&[b"OFF", b"LOW", b"HIGH"]);
        assert_eq!(fixed.count(), 3);
        let mut buf = [0u8; 16];
        assert_eq!(fixed.write(2, &mut buf), b"HIGH");
        let numbers = Labels::Numbers {
            min: 5,
            max: 23,
            step: 3,
            pad: 2,
        };
        assert_eq!(numbers.count(), 7);
        assert_eq!(numbers.value(6), 23);
        assert_eq!(numbers.write(1, &mut buf), b"08");
    }

    #[test]
    fn a_slow_lift_snaps_to_the_nearest_row() {
        let mut wheel = WheelState::new(4, 12, false);
        wheel.press();
        wheel.drag(-15, 44);
        wheel.release(100, 44);
        assert_eq!(settle(&mut wheel), 4);
    }

    #[test]
    fn a_fling_carries_several_rows_before_it_snaps() {
        let mut wheel = WheelState::new(0, 24, false);
        wheel.press();
        wheel.drag(-20, 44);
        wheel.release(800, 44);
        assert!(wheel.moving());
        let rest = settle(&mut wheel);
        assert!(
            (3..=5).contains(&rest),
            "an 800 px/s fling landed on {rest}"
        );
    }

    #[test]
    fn the_ends_stop_and_come_back() {
        let mut wheel = WheelState::new(0, 12, false);
        wheel.press();
        wheel.drag(-40, 44);
        wheel.release(0, 44);
        assert_eq!(settle(&mut wheel), 0);
        wheel.set_index(11);
        wheel.press();
        wheel.drag(80, 44);
        wheel.release(0, 44);
        assert_eq!(settle(&mut wheel), 11);
    }

    #[test]
    fn a_wrapping_wheel_passes_its_ends() {
        let mut wheel = WheelState::new(23, 24, true);
        wheel.press();
        wheel.drag(44, 44);
        wheel.release(0, 44);
        assert_eq!(settle(&mut wheel), 0, "midnight wraps");
        wheel.tap_row(-1);
        assert_eq!(wheel.index(), 23);
    }

    #[test]
    fn a_shorter_range_pulls_a_wrapped_index_back_inside() {
        let mut wheel = WheelState::new(30, 31, true);
        wheel.set_count(28);
        assert!(wheel.index() < 28);
    }

    #[test]
    fn a_tap_on_a_row_selects_it_without_momentum() {
        let mut wheel = WheelState::new(6, 24, false);
        wheel.tap_row(2);
        assert_eq!(wheel.index(), 8);
        assert!(!wheel.moving());
        wheel.tap_row(-5);
        assert_eq!(wheel.index(), 3);
    }

    #[test]
    fn three_columns_split_the_rect_and_a_point_finds_its_own() {
        let rect = Rect::new(10, 20, 300, 140);
        assert_eq!(column_at(rect, 3, 10, 30), Some(0));
        assert_eq!(column_at(rect, 3, 160, 30), Some(1));
        assert_eq!(column_at(rect, 3, 309, 30), Some(2));
        assert_eq!(column_at(rect, 3, 310, 30), None);
        let middle = column_rect(rect, 3, 1);
        assert_eq!((middle.x, middle.w), (110, 100));
    }
}
