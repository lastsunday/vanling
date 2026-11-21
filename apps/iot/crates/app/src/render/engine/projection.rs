//! State → the values a renderer draws from.

use crate::shared::model::{Breath, DeviceState, LightState, MAX_LIGHTS, SurfaceRole};
use crate::shared::snapshot::{
    BreathSnapshot, FrameSnapshot, LightSnapshot, PageSnapshot, TouchSnapshot,
};
use iot_core::demand::EffectiveDemand;
use iot_core::light::{MODE_BREATH, MODE_OFF, MODE_SOLID, rgb_hue, scale_brightness};

use super::appearance::LightAppearance;

/// The lights as the device shows them: a lamp carries the link's news instead of its own
/// state, because a device with no page has nowhere else to put it.
pub(super) fn status_lights(state: &DeviceState) -> [LightState; MAX_LIGHTS] {
    let mut lights = state.lights;
    if let Some(status) = state.link_notice() {
        for (light, role) in lights.iter_mut().zip(state.surface_roles) {
            if role == SurfaceRole::Lamp {
                *light = status;
            }
        }
    }
    lights
}

/// Appearance of the light under `state`, without time information.
pub fn light_appearance(state: LightState) -> LightAppearance {
    match state {
        LightState::Off => LightAppearance::Off,
        LightState::Solid { color, brightness } => {
            LightAppearance::Color(scale_brightness(color, brightness))
        }
        LightState::Breath(breath) => LightAppearance::Breathing(breath),
    }
}

fn light_snapshot(state: LightState) -> LightSnapshot {
    match state {
        LightState::Off => LightSnapshot {
            mode: MODE_OFF,
            brightness: 0,
            hue: 0,
            breath: BreathSnapshot::default(),
        },
        LightState::Solid { color, brightness } => LightSnapshot {
            mode: MODE_SOLID,
            brightness,
            hue: rgb_hue(color),
            breath: BreathSnapshot::default(),
        },
        LightState::Breath(Breath {
            period_ms,
            hue_period_ms,
            hue_span,
            min_brightness,
            max_brightness,
            saturation,
            hue,
            group,
            group_len,
        }) => LightSnapshot {
            mode: MODE_BREATH,
            brightness: max_brightness,
            hue: if group_len > 0 { group[0] } else { hue },
            breath: BreathSnapshot {
                period_ms: period_ms.try_into().unwrap_or(u16::MAX),
                hue_period_ms: hue_period_ms.try_into().unwrap_or(u16::MAX),
                hue_span,
                group_len,
                min_brightness,
                max_brightness,
                saturation,
            },
        },
    }
}

pub(super) fn frame_snapshot(
    state: &DeviceState,
    pages: PageSnapshot,
    demand: &EffectiveDemand,
) -> FrameSnapshot {
    FrameSnapshot {
        demand: *demand,
        touch: TouchSnapshot {
            taps: state.touch.view.tap_count,
            presses: state.touch.view.press_count,
            long_presses: state.touch.view.long_press_count,
            ghost: state.touch.view.ghost_count,
            swipes: state.touch.view.swipe_count,
            last_swipe_dir: state.touch.view.last_swipe.map_or(0, |(dir, _)| dir.code()),
            last_swipe_dist: state.touch.view.last_swipe.map_or(0, |(_, dist)| dist),
            last_gesture_origin: state.touch.view.last_gesture_origin,
            last_gesture_end: state.touch.view.last_gesture_end,
            chip_gesture_id: state.touch.view.chip_gesture_id,
            points: state.touch.points.map(|p| p.map(|l| (l.x, l.y))),
            live_dir: state.touch.live_dir,
            two_finger_runs: state.touch.two_finger_runs,
            frames: state.touch.frames,
            finger: state.touch.view.finger,
            held_ms: state.touch.view.held_ms,
        },
        lights: state.lights.map(light_snapshot),
        page: state.page(),
        steady: state.page().steady(),
        link: state.network.link,
        mac: state.network.mac,
        aps: state.network.aps,
        pages,
        motion: state.motion.sample,
        motion_counts: state.motion.counts,
        motion_caps: state.backed.motion_caps(),
        audio: state.audio_snapshot(),
        playback: state.playback_snapshot(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::Differ;
    use super::super::test_support::*;
    use super::*;
    use crate::shared::intent::{AudioIntent, BusinessIntent, NavigationIntent};
    use crate::shared::snapshot::{BreathSnapshot, LightSnapshot};
    use crate::state::{AudioPhase, Backed, DeviceManager, PageId};
    use iot_core::demand::DemandKey;
    use iot_core::drivers::audio::{AudioEnvelope, AudioSample};
    use iot_core::drivers::network::{LinkStatus, SetupHost};
    use iot_core::light::Rgb;

    #[test]
    fn the_board_address_reaches_the_screen_with_the_rest_of_the_header() {
        let mac = [0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef];
        let mut state = state(LightState::boot());
        state.network.mac = Some(mac);
        assert_eq!(
            frame_snapshot(&state, PageSnapshot::default(), &EffectiveDemand::default()).mac,
            Some(mac)
        );
    }

    #[test]
    fn the_audio_page_draws_the_capture_the_phase_points_at() {
        let mut manager = DeviceManager::boot(crate::state::Boot {
            backed: Backed {
                audio: true,
                ..Backed::NONE
            },
            ..Default::default()
        });
        let mut envelope = AudioEnvelope::default();
        let mut samples = [0_i16; iot_core::drivers::audio::SAMPLES_PER_COLUMN as usize];
        samples[0] = i16::MIN;
        envelope.push(&samples);
        let poll = AudioSample {
            envelope,
            elapsed_ms: 1_000,
            restarts: 2,
            dba_lsb: 0,
            fault: None,
        };
        manager.apply_capture(poll);
        assert_eq!(
            frame_snapshot(&manager.state(), PageSnapshot::default(), manager.demand())
                .audio
                .phase,
            AudioPhase::Idle,
            "polling alone never starts a capture"
        );
        manager.apply_business(BusinessIntent::Audio(AudioIntent::Start));
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
        let recording =
            frame_snapshot(&manager.state(), PageSnapshot::default(), manager.demand()).audio;
        assert_eq!(recording.phase, AudioPhase::Recording);
        assert_eq!(
            recording.envelope, poll.envelope,
            "a live capture draws live"
        );
        assert_eq!(
            recording.restarts, 2,
            "the repair count has to reach the screen, or a stalled capture that was \
             recovered from still reads as a capture that simply went quiet"
        );

        manager.apply_business(BusinessIntent::Audio(AudioIntent::Stop));
        let stopped =
            frame_snapshot(&manager.state(), PageSnapshot::default(), manager.demand()).audio;
        assert_eq!(stopped.phase, AudioPhase::Stopped);
        assert_eq!(
            stopped.envelope, poll.envelope,
            "the latch stands in once the driver moves on"
        );
    }

    #[test]
    fn the_frame_projection_owns_every_state_domain() {
        // Compile-time guard rather than a behaviour test: `frame_snapshot` projects
        // `DeviceState` field by field, and a new domain must be looked at there instead of
        // silently reading as its `Default` on the screen.
        fn project_every_field(state: DeviceState) {
            let DeviceState {
                backed: _,
                nav: _,
                setup_requested: _,
                lights: _,
                surface_roles: _,
                notice: _,
                motion: _,
                audio: _,
                playback: _,
                network: _,
                touch: _,
                dropped_intents: _,
            } = state;
        }
        project_every_field(DeviceState::default());
    }

    #[test]
    fn press_and_frame_snapshots_carry_coordinates() {
        let mut ctl = Differ::new();
        let mut manager = landed();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::shared::intent::recognize(
            iot_core::drivers::input::InputEvent::Gesture(
                iot_core::drivers::input::GestureEvent::Press {
                    id: 0,
                    x: 42,
                    y: 97,
                },
            ),
            0,
        ));
        let state = manager.state();
        let mut expected = frame_for(state.lights[0]);
        expected.touch.presses = 1;
        expected.touch.points = [Some((42, 97)), None];
        assert_eq!(collect(&mut ctl, &state), vec![Seen::Frame(expected)]);
        let mut ctl = Differ::new();
        let mut manager = landed();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::shared::intent::recognize(
            iot_core::drivers::input::InputEvent::Touch(iot_core::drivers::input::TouchEvent {
                points: [iot_core::drivers::input::TouchPoint {
                    id: 0,
                    x: 7,
                    y: 8,
                    status: iot_core::drivers::input::TouchStatus::Down,
                }; iot_core::drivers::input::MAX_TOUCH_POINTS],
                len: 1,
            }),
            0,
        ));
        manager.apply_input(crate::shared::intent::recognize(
            iot_core::drivers::input::InputEvent::Touch(iot_core::drivers::input::TouchEvent {
                points: [iot_core::drivers::input::TouchPoint {
                    id: 0,
                    x: 9,
                    y: 10,
                    status: iot_core::drivers::input::TouchStatus::Contact,
                }; iot_core::drivers::input::MAX_TOUCH_POINTS],
                len: 1,
            }),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let Seen::Frame(frame) = seen[0].clone() else {
            panic!("first diff must carry the frame");
        };
        assert_eq!(
            (frame.touch.frames, frame.touch.points[0]),
            (2, Some((9, 10)))
        );
    }

    #[test]
    fn swipe_chip_and_tap_reach_the_frame() {
        let mut ctl = Differ::new();
        let mut manager = crate::state::DeviceManager::new();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::shared::intent::recognize(
            iot_core::drivers::input::InputEvent::Gesture(
                iot_core::drivers::input::GestureEvent::Swipe {
                    id: 0,
                    x: 20,
                    y: 90,
                    end_x: 20,
                    end_y: 15,
                    direction: iot_core::drivers::input::SwipeDirection::Up,
                    held_ms: 80,
                    distance_px: 99,
                },
            ),
            0,
        ));
        manager.apply_input(crate::shared::intent::recognize(
            iot_core::drivers::input::InputEvent::ChipGesture(0x10),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let Seen::Frame(frame) = seen[0].clone() else {
            panic!("first diff must carry the frame");
        };
        assert_eq!(frame.touch.swipes, 1);
        assert_eq!(
            frame.touch.last_swipe_dir,
            iot_core::drivers::input::SwipeDirection::Up.code()
        );
        assert_eq!(frame.touch.last_swipe_dist, 99);
        assert_eq!(frame.touch.last_gesture_origin, Some((20, 90)));
        assert_eq!(frame.touch.last_gesture_end, Some((20, 15)));
        assert_eq!(frame.touch.chip_gesture_id, 0x10);
        let mut ctl = Differ::new();
        let mut manager = crate::state::DeviceManager::new();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::shared::intent::recognize(
            iot_core::drivers::input::InputEvent::Gesture(
                iot_core::drivers::input::GestureEvent::Tap {
                    id: 0,
                    x: 5,
                    y: 7,
                    held_ms: 590,
                    at_ms: 590,
                },
            ),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let Seen::Frame(frame) = seen[0].clone() else {
            panic!("first diff must carry the frame");
        };
        assert_eq!(frame.touch.taps, 1);
        assert_eq!(frame.touch.held_ms, 590);
        assert_eq!(
            frame.touch.last_gesture_origin,
            Some((5, 7)),
            "a tap reports its touch point"
        );
    }

    #[test]
    fn light_snapshot_maps_every_mode() {
        assert_eq!(
            light_snapshot(LightState::Off),
            LightSnapshot {
                mode: MODE_OFF,
                brightness: 0,
                hue: 0,
                breath: BreathSnapshot::default(),
            }
        );
        assert_eq!(
            light_snapshot(LightState::Solid {
                color: Rgb(255, 0, 0),
                brightness: 140,
            }),
            LightSnapshot {
                mode: MODE_SOLID,
                brightness: 140,
                hue: 0,
                breath: BreathSnapshot::default(),
            },
            "solid reports its brightness and hue, no breathing dimensions"
        );
        assert_eq!(
            light_snapshot(LightState::Breath(crate::state::Breath {
                hue: 17,
                group: [0, 0, 0, 0, 0, 0, 0],
                group_len: 0,
                ..BOOT_BREATH
            }))
            .hue,
            17,
            "an empty group falls back to the standalone hue"
        );
        let breath = light_snapshot(LightState::Breath(BOOT_BREATH));
        assert_eq!(
            (breath.mode, breath.brightness, breath.hue),
            (MODE_BREATH, 80, 16),
            "a breathing light reports its ceiling and group head"
        );
        assert_eq!(
            breath.breath,
            BreathSnapshot {
                period_ms: crate::state::LightState::DEFAULT_PERIOD_MS as u16,
                hue_period_ms: crate::state::LightState::DEFAULT_HUE_PERIOD_MS as u16,
                hue_span: crate::state::LightState::DEFAULT_HUE_SPAN,
                group_len: crate::state::LightState::DEFAULT_GROUP_LEN,
                min_brightness: crate::state::LightState::DEFAULT_MIN_BRIGHTNESS,
                max_brightness: crate::state::LightState::DEFAULT_MAX_BRIGHTNESS,
                saturation: crate::state::LightState::DEFAULT_SATURATION,
            },
            "breathing carries every dimension the overlay prints"
        );
    }

    #[test]
    fn light_appearance_maps_every_state() {
        assert_eq!(light_appearance(LightState::Off), LightAppearance::Off);
        assert_eq!(
            light_appearance(LightState::Solid {
                color: Rgb(1, 2, 3),
                brightness: 255,
            }),
            LightAppearance::Color(Rgb(1, 2, 3))
        );
        assert_eq!(
            light_appearance(LightState::Solid {
                color: Rgb(200, 100, 50),
                brightness: 128,
            }),
            LightAppearance::Color(Rgb(100, 50, 25)),
            "solid brightness scales the color down toward black"
        );
        assert!(matches!(
            light_appearance(LightState::Breath(crate::state::Breath {
                period_ms: 1,
                hue_period_ms: 2,
                hue_span: 3,
                min_brightness: 1,
                max_brightness: 3,
                saturation: 4,
                hue: 5,
                group: [6, 7, 8, 0, 0, 0, 0],
                group_len: 3,
            })),
            LightAppearance::Breathing(_)
        ));
    }

    #[test]
    fn the_link_notice_lands_on_every_lamp_and_no_screen() {
        for (kinds, expected) in [
            (
                [SurfaceRole::Screen, SurfaceRole::Lamp],
                alloc::vec![
                    (0, LightAppearance::Off),
                    (1, light_appearance(LightState::SETUP))
                ],
            ),
            (
                [SurfaceRole::Lamp, SurfaceRole::Lamp],
                alloc::vec![
                    (0, light_appearance(LightState::SETUP)),
                    (1, light_appearance(LightState::SETUP)),
                ],
            ),
            (
                [SurfaceRole::Screen, SurfaceRole::Absent],
                alloc::vec![(0, LightAppearance::Off)],
            ),
            ([SurfaceRole::Absent, SurfaceRole::Absent], alloc::vec![]),
        ] {
            let mut ctl = Differ::new();
            let status = DeviceState {
                lights: [LightState::Off, LightState::Off],
                surface_roles: kinds,
                backed: crate::state::Backed {
                    network: true,
                    ..crate::state::Backed::NONE
                },
                network: crate::state::NetworkState {
                    link: LinkStatus {
                        mode: iot_core::drivers::network::NetworkMode::Setup {
                            host: SetupHost::AccessPoint,
                            cancellable: false,
                        },
                        ..LinkStatus::default()
                    },
                    ..Default::default()
                },
                ..DeviceState::default()
            };
            assert_eq!(
                light_appearances(collect(&mut ctl, &status)),
                expected,
                "{kinds:?}"
            );
        }
    }

    #[test]
    fn the_snapshot_carries_the_demand_in_force_not_the_pages_own() {
        let setup = crate::state::Boot {
            backed: Backed {
                motion: Some(iot_core::drivers::motion::MotionCapabilities::EMPTY),
                audio: true,
                camera: true,
                ..Backed::NONE
            },
            demand_keys: alloc::vec![
                DemandKey::MOTION,
                DemandKey::AUDIO,
                DemandKey::CAMERA,
                DemandKey::DISPLAY
            ],
            boot_page: PageId::Audio,
            ..Default::default()
        };
        let mut quiet_manager = DeviceManager::boot(setup.clone());
        quiet_manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
        assert_eq!(quiet_manager.state().page(), PageId::Audio);
        let quiet = *quiet_manager.demand();
        assert_eq!(
            quiet.claim(DemandKey::AUDIO).mode,
            iot_core::demand::Mode::Continuous
        );

        let mut manager = DeviceManager::boot(crate::state::Boot {
            overrides: iot_core::demand::parse_override("audio=off"),
            ..setup
        });
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
        assert_eq!(
            manager.demand().claim(DemandKey::AUDIO).mode,
            iot_core::demand::Mode::Off
        );

        let snapshot = frame_snapshot(&manager.state(), PageSnapshot::default(), manager.demand());
        assert_eq!(
            snapshot.demand.claim(DemandKey::AUDIO).mode,
            iot_core::demand::Mode::Off
        );
        assert_eq!(
            frame_snapshot(&manager.state(), PageSnapshot::default(), &quiet)
                .demand
                .claim(DemandKey::AUDIO)
                .mode,
            iot_core::demand::Mode::Continuous,
            "the page's own demand is still there to be passed when it is the one in force"
        );
    }
}
