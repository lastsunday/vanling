//! The renderer registry: one entry per wired surface, reconciled from device state.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::demand::EffectiveDemand;
use crate::render::{Activity, RenderController, Renderer, Slot};
use crate::state::DeviceState;

struct Entry {
    activity: Activity,
    renderer: Box<dyn Renderer>,
}

/// The renderers a device carries, fed from one [`RenderController`] diff.
pub struct Registry {
    controller: RenderController,
    renderers: Vec<Entry>,
}

impl Registry {
    pub const fn new() -> Self {
        Self {
            controller: RenderController::new(),
            renderers: Vec::new(),
        }
    }

    /// Adds a renderer and hands it the appearances the controller has already seen, so a
    /// renderer that arrives after the first diff is not left showing nothing.
    pub fn register(&mut self, renderer: Box<dyn Renderer>, now_ms: u32) {
        let mut entry = Entry {
            activity: Activity::Idle,
            renderer,
        };
        let slots: Vec<Slot> = entry.renderer.slots().to_vec();
        for slot in slots {
            if let Some(appearance) = self.controller.current(slot) {
                entry.activity = entry.renderer.on_appearance(appearance, now_ms);
            }
        }
        self.renderers.push(entry);
    }

    /// Diffs the state, fans the changed appearances out to the renderers that claim those
    /// slots, and steps the ones that asked for time. Answers whether any still wants frames.
    pub fn tick(&mut self, state: &DeviceState, demand: &EffectiveDemand, now_ms: u32) -> bool {
        let controller = &mut self.controller;
        let entries = &mut self.renderers;
        controller.reconcile(state, demand, |appearance| {
            let slot = appearance.slot();
            for entry in entries.iter_mut() {
                if entry.renderer.slots().contains(&slot) {
                    entry.activity = entry.renderer.on_appearance(appearance.clone(), now_ms);
                }
            }
        });
        let mut any = false;
        for entry in self.renderers.iter_mut() {
            if entry.activity == Activity::TimeDriven {
                entry.activity = entry.renderer.step(now_ms);
            }
            if entry.activity == Activity::TimeDriven {
                any = true;
            }
        }
        any
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::EffectiveDemand;
    use crate::render::{LightAppearance, SlotAppearance};
    use crate::state::{DeviceManager, LightState, SurfaceRole};
    use alloc::rc::Rc;
    use core::cell::RefCell;

    struct Recorder {
        slots: [Slot; 1],
        seen: Rc<RefCell<Vec<LightAppearance>>>,
    }

    impl Renderer for Recorder {
        fn slots(&self) -> &[Slot] {
            &self.slots
        }

        fn on_appearance(&mut self, appearance: SlotAppearance, _now_ms: u32) -> Activity {
            if let SlotAppearance::Surface { appearance, .. } = appearance {
                self.seen.borrow_mut().push(appearance);
            }
            Activity::Idle
        }

        fn step(&mut self, _now_ms: u32) -> Activity {
            Activity::Idle
        }
    }

    fn state() -> DeviceState {
        let mut state = DeviceManager::new().state();
        state.surface_roles[0] = SurfaceRole::Lamp;
        state.lights[0] = LightState::boot();
        state
    }

    #[test]
    fn a_renderer_registered_after_the_first_diff_catches_up() {
        let state = state();
        let demand = EffectiveDemand::default();
        let mut registry = Registry::new();
        registry.tick(&state, &demand, 0);

        let seen = Rc::new(RefCell::new(Vec::new()));
        registry.register(
            Box::new(Recorder {
                slots: [Slot::Surface(0)],
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
