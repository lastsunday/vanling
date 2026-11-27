//! The renderers a device carries, fed from one diff.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::appearance::Topic;
use super::differ::Differ;
use super::renderer::{Drive, Renderer};
use crate::shared::model::DeviceState;
use crate::shared::snapshot::PageSnapshot;
use iot_core::demand::EffectiveDemand;

struct Slot {
    drive: Drive,
    renderer: Box<dyn Renderer>,
}

/// The renderers a device carries, fed from one [`Differ`] diff.
pub struct RenderSet {
    differ: Differ,
    slots: Vec<Slot>,
}

impl RenderSet {
    pub const fn new() -> Self {
        Self {
            differ: Differ::new(),
            slots: Vec::new(),
        }
    }

    /// Adds a renderer and hands it the appearances the differ has already seen, so a
    /// renderer that arrives after the first diff is not left showing nothing.
    pub fn register(&mut self, renderer: Box<dyn Renderer>, now_ms: u32) {
        let mut slot = Slot {
            drive: Drive::Idle,
            renderer,
        };
        let topics: Vec<Topic> = slot.renderer.topics().to_vec();
        for topic in topics {
            if let Some(appearance) = self.differ.latest(topic) {
                slot.drive = slot
                    .drive
                    .or(slot.renderer.on_appearance(appearance, now_ms));
            }
        }
        self.slots.push(slot);
    }

    /// Diffs the state, fans the changed appearances out to the renderers that claim those
    /// topics, and steps the ones that asked for time. Answers whether any still wants frames.
    pub fn tick(
        &mut self,
        state: &DeviceState,
        pages: PageSnapshot,
        demand: &EffectiveDemand,
        now_ms: u32,
    ) -> bool {
        let differ = &mut self.differ;
        let slots = &mut self.slots;
        differ.diff(state, pages, demand, |appearance| {
            let topic = appearance.topic();
            for slot in slots.iter_mut() {
                if slot.renderer.topics().contains(&topic) {
                    slot.drive = slot
                        .drive
                        .or(slot.renderer.on_appearance(appearance, now_ms));
                }
            }
        });
        let mut any = false;
        for slot in self.slots.iter_mut() {
            if slot.drive == Drive::TimeDriven {
                slot.drive = slot.renderer.step(now_ms);
            }
            if slot.drive == Drive::TimeDriven {
                any = true;
            }
        }
        any
    }

    /// The panel's last geometry, so input can name what it touched.
    pub fn layout(&self) -> iot_core::ui::Layout {
        self.slots
            .iter()
            .find_map(|slot| slot.renderer.layout())
            .unwrap_or_default()
    }

    /// Ships what the last [`RenderSet::tick`] buffered. Awaited rather than folded into the
    /// tick because the screen's transfer yields between DMA chunks, and the tick must stay a
    /// plain state diff for the callers that only want to know whether frames are still due.
    pub async fn flush(&mut self) {
        for slot in self.slots.iter_mut() {
            if let Some(flush) = slot.renderer.flush() {
                flush.await;
            }
        }
    }
}

impl Default for RenderSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Appearance, LightAppearance};
    use crate::state::{DeviceManager, LightState, SurfaceRole};
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use iot_core::demand::EffectiveDemand;

    struct Recorder {
        topics: [Topic; 1],
        seen: Rc<RefCell<Vec<LightAppearance>>>,
    }

    impl Renderer for Recorder {
        fn topics(&self) -> &[Topic] {
            &self.topics
        }

        fn on_appearance(&mut self, appearance: Appearance<'_>, _now_ms: u32) -> Drive {
            if let Appearance::Surface { light, .. } = appearance {
                self.seen.borrow_mut().push(light);
            }
            Drive::Idle
        }

        fn step(&mut self, _now_ms: u32) -> Drive {
            Drive::Idle
        }
    }

    fn state() -> DeviceState {
        let mut state = DeviceManager::new().state();
        state.surface_roles[0] = SurfaceRole::Lamp;
        state.lights[0] = LightState::boot();
        state
    }

    struct TwoTopics {
        steps: Rc<RefCell<u32>>,
    }

    impl Renderer for TwoTopics {
        fn topics(&self) -> &[Topic] {
            &[Topic::Frame, Topic::Surface(0)]
        }

        fn on_appearance(&mut self, appearance: Appearance<'_>, _now_ms: u32) -> Drive {
            match appearance {
                Appearance::Frame(_) => Drive::TimeDriven,
                _ => Drive::Idle,
            }
        }

        fn step(&mut self, _now_ms: u32) -> Drive {
            *self.steps.borrow_mut() += 1;
            Drive::Idle
        }
    }

    #[test]
    fn a_later_appearance_does_not_erase_an_earlier_one_asking_for_time() {
        let state = state();
        let demand = EffectiveDemand::default();
        let mut renderers = RenderSet::new();
        let steps = Rc::new(RefCell::new(0));
        renderers.register(
            Box::new(TwoTopics {
                steps: Rc::clone(&steps),
            }),
            0,
        );

        renderers.tick(&state, PageSnapshot::default(), &demand, 0);

        assert_eq!(
            *steps.borrow(),
            1,
            "the frame asked for time, and the surface behind it answered idle"
        );
    }

    #[test]
    fn a_renderer_registered_after_the_first_diff_catches_up() {
        let state = state();
        let demand = EffectiveDemand::default();
        let mut renderers = RenderSet::new();
        renderers.tick(&state, PageSnapshot::default(), &demand, 0);

        let seen = Rc::new(RefCell::new(Vec::new()));
        renderers.register(
            Box::new(Recorder {
                topics: [Topic::Surface(0)],
                seen: Rc::clone(&seen),
            }),
            100,
        );

        assert!(
            !seen.borrow().is_empty(),
            "a late renderer was left showing nothing until the state next moved"
        );
    }
}
