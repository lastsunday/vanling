//! The scroll host every demo gallery shares: one finger, one region, and the arena that
//! decides whether that finger tapped a control, scrolled the box, or dragged a slide.
//!
//! A press over a control is remembered rather than claimed. The first sample past
//! [`DRAG_SLOP`] decides the axis: mostly vertical claims the region under the press and
//! replays the travel already made; mostly horizontal over a slide control is handed to the
//! page, which owns the live offset. Without the remembered press, a scroll that started on a
//! control lost its first few pixels and a drag could not tell a tap from a slide.

use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::{Rect, ScrollId, ScrollInfo, Ui};

use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::route::PageId;
use crate::shared::snapshot::{ScrollAreaView, ScrollShift};
use crate::shared::ui::scroll::{
    BAR, DRAG_SLOP, Drag, Press, STEP_LINES, ScrollConfig, ScrollState, fling_speed, lift,
};

/// A scroll frame blits the pixels already in the buffer, so it fits the loop's 20 ms cadence.
pub const REPAINT_MS: u64 = 20;

/// One finger dragging a control sideways. The page owns the preview; the arena only keeps the
/// origin so the next sample can keep reporting travel from where the finger landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slide {
    id: u8,
    tag: u16,
    x0: i16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scroller {
    scroll: ScrollState<1>,
    drag: Option<Drag>,
    press: Option<Press>,
    slide: Option<Slide>,
}

impl Scroller {
    pub fn measure(&mut self, ui: &Ui<'_, '_>, id: ScrollId, config: ScrollConfig) {
        self.scroll.measure(ui, [id], config);
    }

    pub fn area(&self) -> ScrollInfo {
        self.scroll.areas()[0]
    }

    pub fn moving(&self) -> bool {
        self.scroll.moving()
    }

    /// Whether a finger already owns this gesture: a scroll or a slide in flight. A page that
    /// reads its own canvas must not answer the lift that belongs to that gesture.
    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.slide.is_some()
    }

    pub fn area_at(&self, x: i32, y: i32) -> Option<u8> {
        self.scroll.area_at(x, y)
    }

    pub fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    /// Puts the box at `offset`, for tests that need a control below the fold on screen.
    #[cfg(test)]
    pub(crate) fn park(&mut self, offset: i16) {
        let delta = offset.wrapping_sub(self.scroll.areas()[0].offset);
        self.scroll.drag(0, delta);
    }

    pub fn reset(&mut self) {
        self.scroll.reset();
        self.drag = None;
        self.press = None;
        self.slide = None;
    }

    /// The region as the view draws it: box, thumb gutter, offset and whether a thumb belongs.
    pub fn view(&self) -> ScrollAreaView {
        let area = self.area();
        let max = area.max();
        ScrollAreaView {
            rect: area.rect,
            header: Rect::default(),
            gutter: Rect::new(
                area.rect.right() + i32::from(BAR),
                area.rect.y,
                i32::from(BAR),
                area.rect.h,
            ),
            offset: area.offset,
            max,
            bar: self.scroll.config().bar && max > 0,
        }
    }

    /// The tag a panel point lands on, zero when none: a tag past the region is not a control.
    pub fn tag_at(&self, tags: impl Fn(i32, i32, i16) -> u16, x: i32, y: i32) -> u16 {
        let area = self.area();
        if !area.rect.contains(x, y) {
            return 0;
        }
        tags(x, y, area.offset)
    }

    /// Takes the finger's claim on a region; the first one wins until it lifts.
    pub fn claim(&mut self, id: u8, area: u8, y: i16, at_ms: u32) -> bool {
        if self.drag.is_some() || self.slide.is_some() {
            return false;
        }
        self.scroll.drag(area, 0);
        self.drag = Some(Drag::new(id, area, y, at_ms));
        true
    }

    /// Runs one gallery intent; answers the release speed when the intent was a lift.
    pub fn apply(&mut self, intent: GalleryIntent) -> Option<i16> {
        match intent {
            GalleryIntent::Tap { .. } | GalleryIntent::Key { .. } | GalleryIntent::Pad { .. } => {
                self.press = None;
            }
            GalleryIntent::Slide { id, tag, .. } => {
                if self.slide.is_none()
                    && let Some(x0) = self
                        .press
                        .take()
                        .filter(|press| press.id == id && press.tag == tag)
                        .map(|press| press.x)
                {
                    self.slide = Some(Slide { id, tag, x0 });
                }
            }
            GalleryIntent::SlideEnd { id, .. } => {
                if self.slide.is_some_and(|slide| slide.id == id) {
                    self.slide = None;
                }
                self.press = None;
            }
            GalleryIntent::Press { id, tag, x, y } => {
                let x = i16::try_from(x).unwrap_or(i16::MAX);
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                self.press = Some(Press { id, tag, x, y });
                if tag == 0
                    && let Some(area) = self.area_at(i32::from(x), i32::from(y))
                {
                    self.claim(id, area, y, 0);
                    self.press = None;
                }
            }
            GalleryIntent::Grab { id, area, y } => {
                self.press = None;
                self.claim(id, area, y, 0);
            }
            GalleryIntent::Drag { id, x, y, at_ms } => {
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) {
                    let delta = drag.step(y, at_ms);
                    self.scroll.drag(drag.area, delta);
                } else if let Some(press) = self.press.take().filter(|press| press.id == id) {
                    if let Some(area) = self.area_at(i32::from(press.x), i32::from(press.y)) {
                        self.scroll.drag(area, 0);
                        let mut drag = Drag::new(id, area, press.y, 0);
                        let delta = drag.step(y, at_ms);
                        self.scroll.drag(area, delta);
                        self.drag = Some(drag);
                    }
                } else if let Some(area) = self.area_at(i32::from(x), i32::from(y)) {
                    self.claim(id, area, y, at_ms);
                }
            }
            GalleryIntent::Release {
                id,
                area,
                lines,
                speed,
                at_ms,
                ..
            } => {
                self.press = None;
                if self.slide.is_some_and(|slide| slide.id == id) {
                    self.slide = None;
                }
                let mut released = None;
                if let Some((area, speed)) = lift(&mut self.drag, id, at_ms) {
                    self.scroll.fling(area, speed);
                    released = Some(speed);
                }
                self.scroll
                    .scroll(area, lines, if lines == 0 { 0 } else { speed });
                return released;
            }
            GalleryIntent::Scroll { area, lines, speed } => {
                self.scroll.scroll(area, lines, speed);
            }
        }
        None
    }

    /// What one gesture asks of the gallery. `tag_at` says which control was hit, if any;
    /// `slide` says whether that control takes a horizontal drag of its own.
    pub fn bind(
        &self,
        op: &InputIntent,
        wire: u8,
        tag_at: impl Fn(i32, i32) -> u16,
        slide: impl Fn(u16) -> bool,
    ) -> Option<BusinessIntent> {
        let gallery = |intent| Some(BusinessIntent::Gallery { wire, intent });
        let tracked = self.drag.as_ref().map(|drag| drag.area);
        let area = |x: u16, y: u16| self.area_at(i32::from(x), i32::from(y));
        match op.event {
            InputEvent::Gesture(GestureEvent::Tap {
                id, x, y, at_ms, ..
            }) => {
                if self.slide.is_some_and(|slide| slide.id == id) {
                    return gallery(GalleryIntent::SlideEnd {
                        id,
                        tag: self.slide.map_or(0, |slide| slide.tag),
                        dx: 0,
                    });
                }
                if let Some(area) = tracked {
                    return gallery(GalleryIntent::Release {
                        id,
                        area,
                        lines: 0,
                        speed: 0,
                        at_ms: Some(at_ms),
                        x,
                        y,
                    });
                }
                let tag = tag_at(i32::from(x), i32::from(y));
                if tag == 0 {
                    return None;
                }
                gallery(GalleryIntent::Tap { tag })
            }
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let tag = tag_at(i32::from(x), i32::from(y));
                if tag == 0 && area(x, y).is_none() {
                    return None;
                }
                gallery(GalleryIntent::Press { id, tag, x, y })
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
                if self.drag.as_ref().is_some_and(|drag| drag.id == id) {
                    return gallery(GalleryIntent::Drag { id, x, y, at_ms });
                }
                if let Some(open) = self.slide.filter(|slide| slide.id == id) {
                    let dx = i32::from(x) - i32::from(open.x0);
                    return gallery(GalleryIntent::Slide {
                        id,
                        tag: open.tag,
                        dx: clamp(dx),
                    });
                }
                if let Some(press) = self.press.filter(|press| press.id == id) {
                    let dx = i32::from(x) - i32::from(press.x);
                    let dy = i32::from(y) - i32::from(press.y);
                    if dx.abs() < i32::from(DRAG_SLOP) && dy.abs() < i32::from(DRAG_SLOP) {
                        return None;
                    }
                    if press.tag != 0 && slide(press.tag) && dx.abs() > dy.abs() {
                        return gallery(GalleryIntent::Slide {
                            id,
                            tag: press.tag,
                            dx: clamp(dx),
                        });
                    }
                    return gallery(GalleryIntent::Drag { id, x, y, at_ms });
                }
                if let Some(area) = area(x, y) {
                    return gallery(GalleryIntent::Grab {
                        id,
                        area,
                        y: i16::try_from(y).unwrap_or(i16::MAX),
                    });
                }
                gallery(GalleryIntent::Drag { id, x, y, at_ms })
            }
            InputEvent::Gesture(GestureEvent::Swipe {
                id,
                direction,
                x,
                y,
                end_x,
                end_y,
                held_ms,
                distance_px,
                at_ms,
            }) => {
                if let Some(open) = self.slide.filter(|slide| slide.id == id) {
                    return gallery(GalleryIntent::SlideEnd {
                        id,
                        tag: open.tag,
                        dx: clamp(i32::from(end_x) - i32::from(open.x0)),
                    });
                }
                if matches!(direction, SwipeDirection::Left | SwipeDirection::Right) {
                    let tag = tag_at(i32::from(x), i32::from(y));
                    if tag != 0 && slide(tag) {
                        return gallery(GalleryIntent::SlideEnd {
                            id,
                            tag,
                            dx: clamp(i32::from(end_x) - i32::from(x)),
                        });
                    }
                }
                let (lines, speed) = match direction {
                    SwipeDirection::Up => (STEP_LINES, fling_speed(distance_px, held_ms)),
                    SwipeDirection::Down => (-STEP_LINES, fling_speed(distance_px, held_ms)),
                    _ => (0, 0),
                };
                let area = tracked
                    .or_else(|| area(end_x, end_y))
                    .or_else(|| area(x, y))?;
                gallery(GalleryIntent::Release {
                    id,
                    area,
                    lines,
                    speed,
                    at_ms: Some(at_ms),
                    x,
                    y,
                })
            }
            InputEvent::Gesture(GestureEvent::LongPress { id, x, y, .. }) => {
                if let Some(open) = self.slide.filter(|slide| slide.id == id) {
                    return gallery(GalleryIntent::SlideEnd {
                        id,
                        tag: open.tag,
                        dx: 0,
                    });
                }
                let area = tracked.or_else(|| area(x, y))?;
                gallery(GalleryIntent::Release {
                    id,
                    area,
                    lines: 0,
                    speed: 0,
                    at_ms: None,
                    x,
                    y,
                })
            }
            _ => None,
        }
    }
}

fn clamp(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// One page's pure scroll shift, when nothing but the offset moved.
pub fn shift(
    page: PageId,
    prev: &crate::shared::snapshot::FrameSnapshot,
    next: &crate::shared::snapshot::FrameSnapshot,
    area: fn(&crate::shared::snapshot::PageSnapshot) -> ScrollAreaView,
    moving: fn(&crate::shared::snapshot::PageSnapshot) -> bool,
) -> Option<ScrollShift> {
    if prev.page != page || next.page != page {
        return None;
    }
    let before_area = area(&prev.pages);
    let after_area = area(&next.pages);
    if before_area.rect.w <= 0 || before_area.rect.h <= 0 {
        return None;
    }
    let shift = after_area.offset.wrapping_sub(before_area.offset);
    let moving_ended = moving(&prev.pages) && !moving(&next.pages);
    let mut before = before_area;
    let mut after = after_area;
    before.offset = 0;
    after.offset = 0;
    if before != after {
        return None;
    }
    if !moving_ended && shift == 0 {
        return None;
    }
    Some(ScrollShift::new(&[after_area], &[shift]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::surface::{finish, headless};
    use iot_core::ui::{Rect, Style};

    const WIRE: u8 = 1;
    const AREA: ScrollId = ScrollId::new(1);
    const TAG: u16 = 7;

    /// A gallery 100 px tall over 400 px of run, with a control covering its whole box.
    fn rig() -> (Scroller, Rect) {
        let mut ui = headless();
        ui.scroll(Style::new().height(100).scroll_area(AREA), |ui| {
            ui.node(Style::new().height(400).tag(TAG));
        });
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, ScrollConfig::default());
        let rect = scroll.area().rect;
        (scroll, rect)
    }

    fn at(x: u16, y: u16) -> InputIntent {
        InputIntent {
            source: 0,
            event: InputEvent::Gesture(GestureEvent::Press { id: 0, x, y }),
        }
    }

    fn drag(x: u16, y: u16) -> InputIntent {
        InputIntent {
            source: 0,
            event: InputEvent::Gesture(GestureEvent::Drag {
                id: 0,
                x,
                y,
                at_ms: 20,
            }),
        }
    }

    fn tap(x: u16, y: u16) -> InputIntent {
        InputIntent {
            source: 0,
            event: InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms: 40,
            }),
        }
    }

    fn intent(bound: BusinessIntent) -> GalleryIntent {
        match bound {
            BusinessIntent::Gallery { intent, .. } => intent,
            other => panic!("not a gallery intent: {other:?}"),
        }
    }

    fn send(page: &mut Scroller, op: &InputIntent, slide: bool) -> Option<GalleryIntent> {
        let bound = page.bind(op, WIRE, |_, _| TAG, |_| slide)?;
        let intent = intent(bound);
        page.apply(intent);
        Some(intent)
    }

    #[test]
    fn a_drag_from_a_control_claims_the_region_and_replays_its_first_travel() {
        let (mut page, rect) = rig();
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let from = u16::try_from(rect.bottom() - 10).unwrap();
        assert_eq!(
            send(&mut page, &at(x, from), false),
            Some(GalleryIntent::Press {
                id: 0,
                tag: TAG,
                x,
                y: from
            })
        );
        assert_eq!(
            send(&mut page, &drag(x, from - 20), false),
            Some(GalleryIntent::Drag {
                id: 0,
                x,
                y: from - 20,
                at_ms: 20
            })
        );
        assert_eq!(
            page.area().offset,
            20,
            "the travel made before the axis lock is not lost"
        );
    }

    #[test]
    fn a_tap_after_a_press_still_reaches_its_control() {
        let (mut page, rect) = rig();
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let y = u16::try_from(rect.y + rect.h / 2).unwrap();
        send(&mut page, &at(x, y), false);
        assert_eq!(
            send(&mut page, &tap(x, y), false),
            Some(GalleryIntent::Tap { tag: TAG }),
            "the remembered press must not swallow the tap"
        );
        assert_eq!(page.area().offset, 0, "a tap does not move the gallery");
    }

    #[test]
    fn travel_below_the_slop_waits_and_then_replays_from_the_press() {
        let (mut page, rect) = rig();
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let y = u16::try_from(rect.y + rect.h / 2).unwrap();
        send(&mut page, &at(x, y), false);
        assert_eq!(send(&mut page, &drag(x, y - 3), false), None);
        send(&mut page, &drag(x, y - 10), false);
        assert_eq!(page.area().offset, 10);
    }

    #[test]
    fn a_horizontal_drag_over_a_slide_control_is_not_a_scroll() {
        let (mut page, rect) = rig();
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let y = u16::try_from(rect.y + rect.h / 2).unwrap();
        send(&mut page, &at(x, y), true);
        assert_eq!(
            send(&mut page, &drag(x + 30, y + 2), true),
            Some(GalleryIntent::Slide {
                id: 0,
                tag: TAG,
                dx: 30
            })
        );
        assert_eq!(page.area().offset, 0, "a slide does not scroll the box");
        assert_eq!(
            send(&mut page, &drag(x - 30, y + 2), true),
            Some(GalleryIntent::Slide {
                id: 0,
                tag: TAG,
                dx: -30
            })
        );
        assert_eq!(
            send(&mut page, &tap(x - 30, y + 2), true),
            Some(GalleryIntent::SlideEnd {
                id: 0,
                tag: TAG,
                dx: 0
            })
        );
    }

    #[test]
    fn a_vertical_drag_over_a_slide_control_still_scrolls() {
        let (mut page, rect) = rig();
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let y = u16::try_from(rect.y + rect.h / 2).unwrap();
        send(&mut page, &at(x, y), true);
        assert!(matches!(
            send(&mut page, &drag(x + 2, y - 20), true),
            Some(GalleryIntent::Drag { .. })
        ));
        assert_eq!(page.area().offset, 20);
    }

    #[test]
    fn a_horizontal_swipe_on_a_slide_control_reports_its_travel_once() {
        let (mut page, rect) = rig();
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let y = u16::try_from(rect.y + rect.h / 2).unwrap();
        let swipe = InputIntent {
            source: 0,
            event: InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Left,
                x,
                y,
                end_x: x.wrapping_sub(80),
                end_y: y,
                held_ms: 40,
                distance_px: 80,
                at_ms: 40,
            }),
        };
        assert_eq!(
            send(&mut page, &swipe, true),
            Some(GalleryIntent::SlideEnd {
                id: 0,
                tag: TAG,
                dx: -80
            })
        );
    }
}
