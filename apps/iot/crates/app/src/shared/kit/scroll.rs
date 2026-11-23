//! Scroll state for a page whose content outgrows its box: offsets, the fling after release, the
//! edge bounce, and the travel a thumb needs. A page measures its regions once, then drives them
//! with [`ScrollState::scroll`] and advances them with [`ScrollState::tick`].

use iot_core::ui::{ScrollId, ScrollInfo, Ui};

/// The least release speed that starts a fling, in px/s.
pub const FLING_MIN_PX_S: u16 = 250;

/// The fastest a fling starts, whatever the swipe measured.
pub const FLING_MAX_PX_S: u16 = 1600;

/// The speed a glide stops at, in px/s.
pub const STOP_PX_S: i16 = 150;

/// Lines one swipe steps before the fling takes over.
pub const STEP_LINES: i8 = 3;

/// Pixels one line of the panel's default face occupies; a taller face says so with `set_line_px`.
pub const LINE_PX: i16 = 12;

/// The two layout-neutral switches a page sets for all its regions: edge bounce and thumb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollConfig {
    pub bounce: bool,
    pub bar: bool,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self {
            bounce: true,
            bar: true,
        }
    }
}

/// The live state of `N` named regions: where each shows, and how fast it is still moving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollState<const N: usize> {
    areas: [ScrollInfo; N],
    velocity: [i16; N],
    line_px: [i16; N],
    held: [bool; N],
    config: ScrollConfig,
}

impl<const N: usize> Default for ScrollState<N> {
    fn default() -> Self {
        Self {
            areas: [ScrollInfo::default(); N],
            velocity: [0; N],
            line_px: [LINE_PX; N],
            held: [false; N],
            config: ScrollConfig::default(),
        }
    }
}

impl<const N: usize> ScrollState<N> {
    /// Reads each region's rectangle and travel out of a laid-out declaration.
    pub fn measure(&mut self, ui: &Ui<'_, '_>, ids: [ScrollId; N], config: ScrollConfig) {
        for (area, id) in self.areas.iter_mut().zip(ids) {
            *area = ui.scroll_info(id).unwrap_or_default();
        }
        self.velocity = [0; N];
        self.config = config;
    }

    pub fn areas(&self) -> &[ScrollInfo; N] {
        &self.areas
    }

    /// The height of one line in `area`, which a swipe steps by.
    pub fn set_line_px(&mut self, area: u8, px: i16) {
        if let Some(slot) = self.line_px.get_mut(usize::from(area)) {
            *slot = px.max(1);
        }
    }

    pub fn config(&self) -> ScrollConfig {
        self.config
    }

    pub fn set_config(&mut self, config: ScrollConfig) {
        self.config = config;
    }

    /// The region a panel point falls in, as the index the intents name it by.
    pub fn area_at(&self, x: i32, y: i32) -> Option<u8> {
        self.areas
            .iter()
            .position(|area| !area.id.is_none() && area.rect.contains(x, y))
            .and_then(|index| u8::try_from(index).ok())
    }

    /// Steps `lines` and starts a fling at `speed`, which is what one swipe asks for.
    pub fn scroll(&mut self, area: u8, lines: i8, speed: u16) {
        let index = usize::from(area);
        if index >= N {
            return;
        }
        let slot = self.areas[index];
        if slot.id.is_none() {
            return;
        }
        self.held[index] = false;
        let max = i32::from(slot.max());
        if max == 0 {
            self.areas[index].offset = 0;
            self.velocity[index] = 0;
            return;
        }
        let limit = self.limit(slot);
        let line_px = self.line_px[index];
        let moved = i32::from(slot.offset) + i32::from(lines) * i32::from(line_px);
        self.areas[index].offset = moved.clamp(-limit, max + limit) as i16;
        let speed = i16::try_from(speed.min(FLING_MAX_PX_S)).unwrap_or(i16::MAX);
        self.velocity[index] = if speed >= FLING_MIN_PX_S as i16 {
            if lines < 0 { -speed } else { speed }
        } else {
            0
        };
    }

    /// Moves a region by `delta` pixels with the finger down; past an edge the travel is halved.
    pub fn drag(&mut self, area: u8, delta: i16) {
        let index = usize::from(area);
        if index >= N {
            return;
        }
        let slot = self.areas[index];
        if slot.id.is_none() {
            return;
        }
        self.held[index] = true;
        self.velocity[index] = 0;
        let max = i32::from(slot.max());
        if max == 0 {
            self.areas[index].offset = 0;
            return;
        }
        let limit = self.limit(slot);
        let offset = i32::from(slot.offset);
        let delta = i32::from(delta);
        let target = offset + delta;
        let damped = if delta > 0 && target > max {
            if offset >= max {
                offset + delta / 2
            } else {
                max + (target - max) / 2
            }
        } else if delta < 0 && target < 0 {
            if offset <= 0 {
                offset + delta / 2
            } else {
                target / 2
            }
        } else {
            target
        };
        self.areas[index].offset = damped.clamp(-limit, max + limit) as i16;
    }

    /// Lets go at `speed` px/s: below the fling threshold the region just comes home.
    pub fn fling(&mut self, area: u8, speed: i16) {
        let index = usize::from(area);
        if index >= N || self.areas[index].id.is_none() {
            return;
        }
        self.held[index] = false;
        let cap = FLING_MAX_PX_S as i16;
        let speed = speed.clamp(-cap, cap);
        self.velocity[index] = if speed.unsigned_abs() >= FLING_MIN_PX_S {
            speed
        } else {
            0
        };
    }

    /// Advances every region one frame: the glide decays, an edge gives, a bounce comes home.
    pub fn tick(&mut self, step_ms: u16) {
        for index in 0..N {
            let area = self.areas[index];
            if area.id.is_none() || self.held[index] {
                continue;
            }
            let max = i32::from(area.max());
            let limit = self.limit(area);
            let mut offset = i32::from(area.offset);
            let mut velocity = i32::from(self.velocity[index]);

            if velocity != 0 {
                offset += velocity * i32::from(step_ms) / 1000;
                velocity -= velocity / 8;
                if (velocity > 0 && offset > max) || (velocity < 0 && offset < 0) {
                    velocity -= velocity / 2;
                }
                if velocity.abs() < i32::from(STOP_PX_S) {
                    velocity = 0;
                }
                offset = offset.clamp(-limit, max + limit);
            }

            if velocity == 0 && !(0..=max).contains(&offset) {
                let edge = offset.clamp(0, max);
                let gap = edge - offset;
                offset = if gap.abs() <= 1 {
                    edge
                } else {
                    offset + gap / 2
                };
            }

            self.velocity[index] = velocity as i16;
            self.areas[index].offset = offset as i16;
        }
    }

    /// Whether any region still has to move: a glide in flight or a bounce on its way home.
    pub fn moving(&self) -> bool {
        self.areas
            .iter()
            .zip(&self.velocity)
            .zip(&self.held)
            .any(|((area, velocity), held)| {
                !*held
                    && (*velocity != 0
                        || area.offset < 0
                        || i32::from(area.offset) > i32::from(area.max()))
            })
    }

    /// Drops every offset and glide: the page is no longer shown.
    pub fn reset(&mut self) {
        for area in &mut self.areas {
            area.offset = 0;
        }
        self.velocity = [0; N];
        self.held = [false; N];
    }

    /// How far past an edge a region may go: a quarter viewport, or zero if it fits or bounce is off.
    fn limit(&self, area: ScrollInfo) -> i32 {
        if self.config.bounce && area.max() > 0 {
            i32::from(area.viewport) / 4
        } else {
            0
        }
    }
}

/// The thumb's width, and the shortest thumb a long list keeps.
pub const BAR: i16 = 2;
pub const MIN_THUMB: i32 = 8;

/// How long after the last drag sample a lift may still read as a flick. Three touch scans:
/// past that the finger stopped moving without a new sample, so the tail velocity is stale.
pub const STILL_GAP_MS: u32 = 30;

/// One finger's claim on a region: where it pressed, how fast it has moved, and whether it did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drag {
    pub id: u8,
    pub area: u8,
    pub y: i16,
    pub at_ms: u32,
    /// Offset pixels per second, positive when the content moves up.
    pub velocity: i16,
    pub moved: bool,
}

impl Drag {
    pub const fn new(id: u8, area: u8, y: i16, at_ms: u32) -> Self {
        Self {
            id,
            area,
            y,
            at_ms,
            velocity: 0,
            moved: false,
        }
    }

    /// Takes the next sample and answers how far the content moved.
    pub fn step(&mut self, y: i16, at_ms: u32) -> i16 {
        let delta = i32::from(self.y) - i32::from(y);
        if self.at_ms != 0 {
            let dt = at_ms.saturating_sub(self.at_ms).max(1);
            let instant = (delta * 1000 / dt as i32)
                .clamp(-i32::from(FLING_MAX_PX_S), i32::from(FLING_MAX_PX_S))
                as i16;
            self.velocity = if self.velocity == 0 {
                instant
            } else {
                (self.velocity + instant) / 2
            };
        }
        self.y = y;
        self.at_ms = at_ms;
        self.moved = true;
        delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }

    /// The speed a lift carries, when the drag tail is recent enough to be a flick.
    pub fn release(&self, at_ms: Option<u32>) -> i16 {
        match at_ms {
            Some(at_ms) if self.moved && at_ms.saturating_sub(self.at_ms) <= STILL_GAP_MS => {
                self.velocity
            }
            _ => 0,
        }
    }
}

/// Lifts the drag `id` owns, answering the area it claimed and the speed it lends.
///
/// A lift from another finger must not stop the glide, so a mismatched id leaves the slot alone.
pub fn lift(slot: &mut Option<Drag>, id: u8, at_ms: Option<u32>) -> Option<(u8, i16)> {
    let drag = slot.take_if(|drag| drag.id == id)?;
    Some((drag.area, drag.release(at_ms)))
}

/// The thumb's top and height inside a viewport, or `None` when the content fits.
pub fn thumb(viewport: i32, offset: i16, max: i16, min: i32) -> Option<(i16, i16)> {
    let max = i32::from(max);
    if viewport <= 0 || max <= 0 {
        return None;
    }
    let floor = min.min(viewport);
    let height = (viewport * viewport / (viewport + max)).clamp(floor, viewport);
    let travel = viewport - height;
    let top = travel * i32::from(offset).clamp(0, max) / max;
    Some((top as i16, height as i16))
}

/// The speed a swipe was released at, in px/s: its average over the whole gesture.
pub fn fling_speed(distance_px: u16, held_ms: u16) -> u16 {
    if held_ms == 0 {
        return 0;
    }
    let speed = u32::from(distance_px) * 1000 / u32::from(held_ms);
    u16::try_from(speed.min(u32::from(FLING_MAX_PX_S))).unwrap_or(FLING_MAX_PX_S)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::ui::{BoxConstraints, PX, Screen, Style, Ui};

    const AREA: ScrollId = ScrollId::new(1);

    /// One region, 40 px tall over `content` px of run, measured the way a page measures.
    fn state(content: i16, config: ScrollConfig) -> ScrollState<1> {
        let mut ui = Ui::headless(100, 40, Screen::default());
        ui.scroll(Style::new().height(40).scroll_area(AREA), |ui| {
            ui.node(Style::new().height(content));
        });
        ui.layout(BoxConstraints::tight(100 * PX, 40 * PX));
        let mut state = ScrollState::default();
        state.measure(&ui, [AREA], config);
        state
    }

    fn settle(state: &mut ScrollState<1>) -> i16 {
        for _ in 0..1_000 {
            if !state.moving() {
                return state.areas()[0].offset;
            }
            state.tick(20);
        }
        panic!("the scroll never came to rest");
    }

    #[test]
    fn a_swipe_speed_is_its_average_over_the_whole_gesture() {
        assert_eq!(fling_speed(120, 100), 1200);
        assert_eq!(fling_speed(0, 100), 0);
        assert_eq!(fling_speed(10, 0), 0);
        assert_eq!(
            fling_speed(2_000, 100),
            FLING_MAX_PX_S,
            "a fast flick is capped"
        );
    }

    #[test]
    fn a_fling_glides_for_a_share_of_its_release_speed() {
        let mut state = state(1_000, ScrollConfig::default());
        state.scroll(0, STEP_LINES, 1_600);
        assert_eq!(state.areas()[0].offset, 36, "the swipe steps first");
        let rest = settle(&mut state);
        let glide = i32::from(rest - 36);
        assert!(
            (220..=290).contains(&glide),
            "a 1600 px/s fling travelled {glide} px"
        );
    }

    #[test]
    fn a_downward_swipe_at_the_top_comes_home_to_the_top() {
        let mut state = state(1_000, ScrollConfig::default());
        state.scroll(0, -STEP_LINES, 1_600);
        assert_eq!(state.velocity[0], -1_600, "the fling follows the swipe");
        assert_eq!(settle(&mut state), 0);
    }

    #[test]
    fn a_downward_swipe_below_the_top_moves_up() {
        let mut state = state(1_000, ScrollConfig::default());
        state.scroll(0, STEP_LINES, 0);
        let before = state.areas()[0].offset;
        state.scroll(0, -STEP_LINES, 1_600);
        assert_eq!(state.areas()[0].offset, before - 36);
        assert_eq!(state.velocity[0], -1_600);
        assert_eq!(settle(&mut state), 0);
    }

    #[test]
    fn a_drag_follows_the_finger_and_damps_past_the_edge() {
        let mut state = state(100, ScrollConfig::default());
        state.drag(0, 30);
        assert_eq!(state.areas()[0].offset, 30);
        state.drag(0, 100);
        assert_eq!(
            state.areas()[0].offset,
            70,
            "the edge gives a quarter viewport"
        );
        state.drag(0, 100);
        assert_eq!(state.areas()[0].offset, 70, "it never passes the limit");
        state.drag(0, -100);
        assert_eq!(state.areas()[0].offset, -10, "the other edge gives too");
    }

    #[test]
    fn a_held_region_does_not_spring_until_the_finger_lifts() {
        let mut state = state(100, ScrollConfig::default());
        state.drag(0, 100);
        state.tick(20);
        assert_eq!(state.areas()[0].offset, 70);
        assert!(!state.moving(), "the finger owns the offset");
        state.fling(0, 0);
        assert_eq!(settle(&mut state), 60);
    }

    #[test]
    fn a_release_flings_in_the_direction_it_was_dragged() {
        let mut state = state(1_000, ScrollConfig::default());
        state.drag(0, 40);
        state.fling(0, -800);
        assert_eq!(state.velocity[0], -800);
        assert!(state.moving());
        assert_eq!(settle(&mut state), 0, "it keeps moving up");
    }

    #[test]
    fn a_slow_release_does_not_fling() {
        let mut state = state(1_000, ScrollConfig::default());
        state.fling(0, 100);
        assert_eq!(state.velocity[0], 0);
        assert!(!state.moving());
    }

    #[test]
    fn a_slow_release_steps_without_a_fling() {
        let config = ScrollConfig {
            bounce: false,
            ..ScrollConfig::default()
        };
        let mut state = state(1_000, config);
        state.scroll(0, -STEP_LINES, 200);
        assert_eq!(state.areas()[0].offset, 0, "the top edge clamps the step");
        assert_eq!(state.velocity[0], 0);
        assert!(!state.moving());
    }

    #[test]
    fn the_bottom_edge_gives_and_comes_home() {
        let mut state = state(100, ScrollConfig::default());
        state.scroll(0, 10, 1_600);
        assert_eq!(state.areas()[0].offset, 70, "max plus a quarter viewport");
        let mut deepest = 0;
        for _ in 0..1_000 {
            if !state.moving() {
                break;
            }
            state.tick(20);
            deepest = deepest.max(state.areas()[0].offset);
        }
        assert_eq!(state.areas()[0].offset, 60, "it comes back to the edge");
        assert!(deepest <= 70, "the bounce never passes its limit");
    }

    #[test]
    fn a_region_that_fits_never_moves() {
        let mut state = state(20, ScrollConfig::default());
        state.scroll(0, 10, 1_600);
        assert_eq!(state.areas()[0].offset, 0);
        assert!(!state.moving());
    }

    #[test]
    fn the_bounce_switch_clamps_at_the_edge() {
        let config = ScrollConfig {
            bounce: false,
            ..ScrollConfig::default()
        };
        let mut state = state(100, config);
        state.scroll(0, 10, 0);
        assert_eq!(state.areas()[0].offset, 60);
        assert!(!state.moving());
    }

    #[test]
    fn a_point_finds_the_region_it_landed_in() {
        let state = state(100, ScrollConfig::default());
        assert_eq!(state.area_at(10, 10), Some(0));
        assert_eq!(state.area_at(10, 50), None);
        assert_eq!(state.area_at(-1, 10), None);
    }

    #[test]
    fn a_hidden_page_drops_its_offsets_and_its_glide() {
        let mut state = state(1_000, ScrollConfig::default());
        state.scroll(0, STEP_LINES, 1_600);
        state.reset();
        assert_eq!(state.areas()[0].offset, 0);
        assert!(!state.moving());
    }
}
