use crate::demand::DemandKey;
use crate::drivers::input::FingerLast;
use crate::drivers::motion::{MotionCapabilities, MotionCounts, MotionSample};
use crate::drivers::network::{
    AccessPoint, ApList, Credentials, JoinRequest, LinkPhase, LinkStatus, NetworkError, Ssid,
};
use crate::intent::*;
use crate::keyboard::Layer;
use crate::paint::Rgb;
use crate::state::test_support::*;

mod demand {
    use super::*;
    use crate::config::{MotionConfig, Unreachable};
    use crate::demand::{ALL_KEYS, Claim, DemandKey, DemandKind, Mode, Sensitivity};

    fn fully_wired() -> DeviceManager {
        fully_wired_on(DisplayPage::Ambient)
    }

    fn fully_wired_on(boot_page: DisplayPage) -> DeviceManager {
        DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(MotionCapabilities::EMPTY),
                audio: true,
                playback: true,
                camera: true,
                ..Backed::NONE
            },
            demand_keys: alloc::vec![
                DemandKey::MOTION,
                DemandKey::AUDIO,
                DemandKey::CAMERA,
                DemandKey::DISPLAY
            ],
            boot_page,
            ..Default::default()
        })
    }

    #[test]
    fn a_pages_class_follows_the_page_that_draws_the_device() {
        assert_eq!(
            DisplayPage::Attitude
                .demand(ALL_KEYS)
                .claim(DemandKey::MOTION)
                .class,
            Sensitivity::Primary,
            "the page that draws the motion sample may wait nothing for it"
        );
        assert_eq!(
            DisplayPage::Audio
                .demand(ALL_KEYS)
                .claim(DemandKey::AUDIO)
                .class,
            Sensitivity::Primary,
            "and the same for the page that draws the envelope"
        );

        for page in CYCLE {
            if matches!(page, DisplayPage::Attitude | DisplayPage::Camera) {
                continue;
            }
            assert_eq!(
                page.demand(ALL_KEYS).claim(DemandKey::MOTION).class,
                Sensitivity::Secondary,
                "{page:?}"
            );
        }

        for page in CYCLE {
            assert_eq!(
                page.demand(ALL_KEYS).claim(DemandKey::MOTION).mode,
                Mode::Continuous,
                "{page:?}"
            );
        }
    }

    #[test]
    fn a_device_the_board_did_not_declare_is_never_claimed() {
        let panel_only = DeviceManager::boot(Setup {
            demand_keys: alloc::vec![DemandKey::DISPLAY],
            ..Default::default()
        });
        let demand = panel_only.demand();

        assert_eq!(demand.claim(DemandKey::MOTION).mode, Mode::Off);
        assert_eq!(demand.claim(DemandKey::AUDIO).mode, Mode::Off);
        assert_eq!(demand.claim(DemandKey::CAMERA).mode, Mode::Off);
        assert_eq!(demand.claim(DemandKey::DISPLAY).mode, Mode::Continuous);
        assert!(
            !demand.keys().any(|key| key.kind() == DemandKind::Motion),
            "a device the board never declared must not appear in the demand at all"
        );
    }

    #[test]
    fn a_page_claims_every_device_it_is_asked_about_and_nothing_more() {
        let claimed = DisplayPage::Camera.demand(ALL_KEYS);

        assert_eq!(claimed.keys().count(), ALL_KEYS.len());
        assert_eq!(claimed.claim(DemandKey::CAMERA).mode, Mode::Continuous);
        for page in CYCLE {
            assert_eq!(
                page.demand(ALL_KEYS).keys().count(),
                ALL_KEYS.len(),
                "{page:?} must claim every device the board declared"
            );
        }
    }

    #[test]
    fn only_the_page_that_draws_the_envelope_claims_the_microphone() {
        for page in CYCLE {
            let claimed = page.demand(ALL_KEYS).claim(DemandKey::AUDIO);
            if page == DisplayPage::Audio {
                assert_eq!(claimed.mode, Mode::Continuous, "{page:?}");
            } else {
                assert_eq!(
                    claimed.mode,
                    Mode::Off,
                    "{page:?} draws no envelope, so it must not claim the microphone"
                );
            }
        }
    }

    #[test]
    fn two_of_one_kind_can_be_demanded_apart() {
        let twin = DeviceManager::boot(Setup {
            demand_keys: alloc::vec![
                DemandKey::instance(DemandKind::Audio, 0),
                DemandKey::instance(DemandKind::Audio, 1),
            ],
            ..Default::default()
        });
        let mut demand = twin.demand().clone();
        demand.set(
            DemandKey::instance(DemandKind::Audio, 0),
            Claim::new(Mode::Continuous, 0, Sensitivity::Primary),
        );
        demand.set(
            DemandKey::instance(DemandKind::Audio, 1),
            Claim::new(Mode::Continuous, 5_000, Sensitivity::Secondary),
        );

        assert_eq!(
            demand
                .claim(DemandKey::instance(DemandKind::Audio, 0))
                .period_ms,
            0,
            "narrowing one sensor must not reach its twin"
        );
        assert_eq!(
            demand
                .claim(DemandKey::instance(DemandKind::Audio, 1))
                .period_ms,
            5_000
        );
    }

    #[test]
    fn demand_follows_the_page_instead_of_lagging_it() {
        let mut manager = landed(fully_wired());
        let before = manager.demand().clone();

        manager.apply_business(BusinessIntent::Page(PageIntent::NextPage));

        assert_ne!(
            manager.demand(),
            &before,
            "the page moved, so the demand it claims should have moved with it"
        );
        assert_eq!(
            manager.demand(),
            &DeviceManager::proposed(&manager.state, &manager.demand_keys, &manager.override_),
            "a held demand a fresh derivation would contradict has fallen behind"
        );
    }

    #[test]
    fn every_write_that_feeds_a_proposal_reaches_the_held_demand() {
        let mut by_page = landed(fully_wired());
        by_page.apply_business(BusinessIntent::Page(PageIntent::NextPage));
        assert_eq!(
            by_page.demand(),
            &DeviceManager::proposed(&by_page.state, &by_page.demand_keys, &by_page.override_)
        );

        for manager in [
            DeviceManager::boot(Setup {
                backed: Backed {
                    camera: true,
                    ..Backed::NONE
                },
                ..Default::default()
            }),
            DeviceManager::boot(Setup {
                backed: Backed {
                    audio: true,
                    ..Backed::NONE
                },
                ..Default::default()
            }),
            DeviceManager::boot(Setup {
                backed: Backed {
                    playback: true,
                    ..Backed::NONE
                },
                ..Default::default()
            }),
            DeviceManager::boot(Setup {
                backed: Backed {
                    motion: Some(MotionCapabilities::EMPTY),
                    ..Backed::NONE
                },
                boot_page: DisplayPage::Attitude,
                ..Default::default()
            }),
        ] {
            assert_eq!(
                manager.demand(),
                &DeviceManager::proposed(&manager.state, &manager.demand_keys, &manager.override_)
            );
        }
    }

    #[test]
    fn a_committed_demand_survives_being_read_back() {
        let mut manager = landed(fully_wired_on(DisplayPage::Camera));
        assert_eq!(manager.state().page, DisplayPage::Camera);
        assert_eq!(
            manager.demand().claim(DemandKey::CAMERA).mode,
            Mode::Continuous
        );

        let narrowed = manager.demand().without(DemandKey::MOTION);
        manager.commit_demand(narrowed);

        assert_eq!(manager.demand().claim(DemandKey::MOTION).mode, Mode::Off);
        assert_eq!(
            manager.demand().claim(DemandKey::CAMERA).mode,
            Mode::Continuous
        );
        assert_ne!(
            manager.demand(),
            &DeviceManager::proposed(&manager.state, &manager.demand_keys, &manager.override_),
            "a committed demand is the one place the two are allowed to differ"
        );
    }

    #[test]
    fn a_refused_device_leaves_the_committed_demand_but_nothing_else_does() {
        let mut manager = landed(fully_wired());
        let refused = Unreachable {
            key: DemandKey::MOTION,
            mode: Mode::OnChange,
            config: MotionConfig::PolledRegisters,
        };

        manager.commit_demand(manager.demand().without(refused.key));

        assert_eq!(manager.demand().claim(DemandKey::MOTION).mode, Mode::Off);
        assert_eq!(
            manager.demand().claim(DemandKey::DISPLAY).mode,
            Mode::Continuous,
            "one device going unmet is not a reason to drop the others"
        );
        assert_eq!(
            manager.demand().claim(DemandKey::INTERACTION).mode,
            Mode::Continuous,
            "nothing a demand says may switch the interaction path off"
        );
    }

    #[test]
    fn a_page_change_re_proposes_over_a_committed_demand() {
        let mut manager = landed(fully_wired());
        manager.commit_demand(manager.demand().without(DemandKey::MOTION));
        assert_eq!(manager.demand().claim(DemandKey::MOTION).mode, Mode::Off);

        manager.apply_business(BusinessIntent::Page(PageIntent::NextPage));

        assert_ne!(
            manager.demand().claim(DemandKey::MOTION).mode,
            Mode::Off,
            "a page that asks for motion again must be heard again"
        );
    }
}

mod boot {
    use super::*;

    #[test]
    fn state_is_clean() {
        let manager = DeviceManager::new();
        assert_eq!(manager.light_state(), BREATH, "boot wakes to breathing");
        assert_eq!(DeviceState::default().lights[0], BREATH);
        assert_eq!(DeviceManager::default().light_state(), BREATH);
        let state = manager.state();
        assert_eq!(manager.touch_state(), None);
        assert_eq!(state.touch.tap_count, 0);
        assert_eq!(state.touch.press_count, 0);
        assert_eq!(state.touch.long_press_count, 0);
        assert_eq!(state.touch.ghost_count, 0);
        assert_eq!(state.touch.swipe_count, 0);
        assert_eq!(state.touch.two_finger_runs, 0);
        assert_eq!(state.touch.frames, 0);
        assert_eq!(state.touch.chip_gesture_id, 0);
        assert_eq!(state.touch.last_swipe, None);
        assert_eq!(state.touch.last_gesture_origin, None);
        assert_eq!(state.touch.last_gesture_end, None);
    }

    #[test]
    fn the_wiring_record_is_what_the_page_table_asks() {
        let bare = DeviceManager::new().state().backed;
        assert_eq!(bare, Backed::NONE);
        assert_eq!(
            bare.motion_caps(),
            MotionCapabilities::EMPTY,
            "a board with no part declares nothing, whatever the board would report"
        );
        assert!(!DisplayPage::Attitude.is_backed(bare));

        let wired = DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(MotionCapabilities::TAP),
                audio: true,
                ..Backed::NONE
            },
            ..Default::default()
        })
        .state()
        .backed;
        assert_eq!(wired.motion, Some(MotionCapabilities::TAP));
        assert_eq!(wired.motion_caps(), MotionCapabilities::TAP);
        assert!(DisplayPage::Attitude.is_backed(wired));
        assert!(DisplayPage::Audio.is_backed(wired));
        assert!(!DisplayPage::Speaker.is_backed(wired));
    }

    #[test]
    fn every_present_device_claims_its_key_in_a_fixed_order() {
        let all = Backed {
            motion: Some(MotionCapabilities::EMPTY),
            audio: true,
            playback: true,
            camera: true,
            ..Backed::NONE
        };
        assert_eq!(
            all.demand_keys(),
            alloc::vec![
                DemandKey::DISPLAY,
                DemandKey::MOTION,
                DemandKey::AUDIO,
                DemandKey::CAMERA
            ],
            "a page claims exactly the devices the board declared; playback owns no demand key"
        );
    }

    #[test]
    fn an_absent_device_never_enters_the_demand() {
        assert_eq!(
            Backed::NONE.demand_keys(),
            alloc::vec![DemandKey::DISPLAY],
            "only the panel is claimed when the board declared nothing else"
        );
    }
}

mod motion {
    use super::*;
    use crate::drivers::motion::{MotionBatch, MotionEvent, MotionEvents, TiltDir};

    const ALL_CAPS: MotionCapabilities = MotionCapabilities::TAP
        .union(MotionCapabilities::STEP)
        .union(MotionCapabilities::STILL)
        .union(MotionCapabilities::MOVING)
        .union(MotionCapabilities::ACTIVITY)
        .union(MotionCapabilities::TILT)
        .union(MotionCapabilities::SHAKE)
        .union(MotionCapabilities::LIFT_PLACE)
        .union(MotionCapabilities::POSTURE);

    fn sample() -> MotionSample {
        MotionSample::from_raw([1, 2, 3], [4, 5, 6], TEST_SCALE, 3, 0)
    }

    fn poll(manager: &mut DeviceManager, events: &[MotionEvent]) {
        let mut batch = MotionEvents::new();
        for &event in events {
            batch.push(event);
        }
        manager.apply_operation(op(InputEvent::Motion(MotionBatch {
            sample: sample(),
            events: batch,
        })));
    }

    type Bucket = fn(&MotionCounts) -> u16;

    #[test]
    fn the_page_switch_skips_pages_the_board_lacks() {
        let mut disabled = landed(DeviceManager::new());
        apply_page(&mut disabled, triple_click());
        assert_eq!(disabled.state().page, DisplayPage::Ambient);

        let mut enabled = landed(DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(ALL_CAPS),
                ..Backed::NONE
            },
            ..Default::default()
        }));
        apply_page(&mut enabled, triple_click());
        assert_eq!(enabled.state().page, DisplayPage::Attitude);
        apply_page(&mut enabled, triple_click());
        assert_eq!(enabled.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn motion_samples_are_kept_only_when_motion_is_enabled() {
        let mut disabled = DeviceManager::new();
        disabled.apply_operation(op(InputEvent::Motion(MotionBatch::new(sample()))));
        assert_eq!(disabled.motion_state(), None);
        assert_eq!(disabled.motion_counts_state(), MotionCounts::default());

        let mut enabled = DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(ALL_CAPS),
                ..Backed::NONE
            },
            ..Default::default()
        });
        enabled.apply_operation(op(InputEvent::Motion(MotionBatch::new(sample()))));
        assert_eq!(enabled.motion_state(), Some(sample()));
        assert_eq!(
            enabled.motion_counts_state(),
            MotionCounts::default(),
            "a silent poll records no semantic"
        );
    }

    #[test]
    fn every_semantic_files_under_its_own_bucket() {
        let cases: [(MotionEvent, Bucket); 14] = [
            (MotionEvent::Tap { count: 1 }, |c| c.taps),
            (MotionEvent::Tap { count: 2 }, |c| c.double_taps),
            (MotionEvent::Tap { count: 3 }, |c| c.triple_taps),
            (MotionEvent::Still, |c| c.still),
            (MotionEvent::Moving, |c| c.moving),
            (MotionEvent::Activity, |c| c.activity),
            (MotionEvent::Step, |c| c.steps),
            (MotionEvent::TiltEnter(TiltDir::Left), |c| c.tilt_enters),
            (MotionEvent::TiltExit(TiltDir::Left), |c| c.tilt_exits),
            (MotionEvent::Shake, |c| c.shakes),
            (MotionEvent::Lift, |c| c.lifts),
            (MotionEvent::Place, |c| c.places),
            (MotionEvent::Portrait, |c| c.portraits),
            (MotionEvent::Landscape, |c| c.landscapes),
        ];
        for (event, bucket) in cases {
            let mut manager = DeviceManager::boot(Setup {
                backed: Backed {
                    motion: Some(ALL_CAPS),
                    ..Backed::NONE
                },
                ..Default::default()
            });
            poll(&mut manager, &[event]);
            let counts = manager.motion_counts_state();
            assert_eq!(bucket(&counts), 1, "{event:?} counted once");
            let others: u16 = cases
                .iter()
                .filter(|(other, _)| *other != event)
                .map(|(_, bucket)| bucket(&counts))
                .sum();
            assert_eq!(others, 0, "{event:?} touched no other bucket");
        }
    }

    #[test]
    fn one_poll_counts_every_semantic_it_carried() {
        let mut manager = DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(ALL_CAPS),
                ..Backed::NONE
            },
            ..Default::default()
        });
        poll(
            &mut manager,
            &[
                MotionEvent::Shake,
                MotionEvent::TiltEnter(TiltDir::Left),
                MotionEvent::Still,
            ],
        );
        let counts = manager.motion_counts_state();
        assert_eq!(counts.shakes, 1);
        assert_eq!(counts.tilt_enters, 1);
        assert_eq!(counts.still, 1);
    }

    #[test]
    fn repeated_polls_accumulate_and_then_saturate() {
        let mut manager = DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(ALL_CAPS),
                ..Backed::NONE
            },
            ..Default::default()
        });
        poll(&mut manager, &[MotionEvent::Place]);
        poll(&mut manager, &[MotionEvent::Place]);
        poll(&mut manager, &[MotionEvent::Place]);
        assert_eq!(manager.motion_counts_state().places, 3);

        let mut saturated = MotionCounts {
            places: u16::MAX,
            ..MotionCounts::default()
        };
        saturated.bump(MotionEvent::Place);
        assert_eq!(
            saturated.places,
            u16::MAX,
            "a counter that wrapped would under-report a long session"
        );
    }

    #[test]
    fn a_board_without_a_motion_source_counts_nothing() {
        let mut manager = DeviceManager::new();
        poll(
            &mut manager,
            &[MotionEvent::Shake, MotionEvent::Tap { count: 1 }],
        );
        assert_eq!(manager.motion_counts_state(), MotionCounts::default());
        assert_eq!(
            manager.state().backed.motion_caps(),
            MotionCapabilities::EMPTY
        );
    }

    #[test]
    fn a_knock_and_a_finger_tap_are_recorded_as_separate_acts() {
        let mut manager = DeviceManager::boot(Setup {
            backed: Backed {
                motion: Some(ALL_CAPS),
                ..Backed::NONE
            },
            ..Default::default()
        });
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 20,
            y: 30,
            held_ms: 40,
            at_ms: 40,
        })));
        poll(&mut manager, &[MotionEvent::Tap { count: 1 }]);
        assert_eq!(
            manager.state().touch.tap_count,
            1,
            "the finger tap still tallies"
        );
        assert_eq!(
            manager.motion_counts_state().taps,
            1,
            "the knock is not folded into the finger tap"
        );
    }
}

mod business {
    use super::*;

    #[test]
    fn set_light_replaces_state_and_snapshot() {
        let mut manager = DeviceManager::new();
        manager.apply_business(set_light(0, LightState::Off));
        assert_eq!(manager.light_state(), LightState::Off);
        let solid = LightState::Solid {
            color: Rgb(3, 4, 5),
            brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
        };
        manager.apply_business(set_light(0, solid));
        assert_eq!(manager.light_state(), solid);
        assert_eq!(
            manager.state().lights[0],
            solid,
            "the snapshot mirrors the applied target"
        );
        manager.apply_business(set_light(7, LightState::Off));
        assert_eq!(manager.light_state_at(1), LightState::Off);
        manager.apply_business(BusinessIntent::Invalid);
        assert_eq!(manager.light_state(), solid, "an invalid intent is a no-op");
    }

    #[test]
    fn a_link_report_lands_whole_and_an_unwired_board_reads_off() {
        let mut manager = DeviceManager::new();
        assert_eq!(manager.state().network.link.phase, LinkPhase::Off);

        let status = LinkStatus {
            phase: LinkPhase::Connected,
            ap: Some(AccessPoint {
                ssid: Ssid::from_octets(b"neighbour"),
                bssid: [0xaa; 6],
                channel: 11,
                rssi: -45,
                secured: true,
            }),
            address: Some(Ipv4::from_octets([192, 168, 1, 136])),
            ..LinkStatus::default()
        };
        manager.apply_business(BusinessIntent::Network(NetworkIntent::LinkReport(status)));
        assert_eq!(manager.state().network.link, status);
    }
}

mod touch {
    use super::*;

    #[test]
    fn snapshots_store_and_replace_the_last_frame() {
        let mut manager = DeviceManager::new();
        let first = TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 1,
                y: 2,
                status: TouchStatus::Down,
            }; MAX_TOUCH_POINTS],
            len: 1,
        };
        manager.apply_operation(op(InputEvent::Touch(first)));
        assert_eq!(manager.touch_state(), Some(first));
        assert_eq!(
            manager.state().touch.press_count,
            0,
            "a raw frame is a snapshot, not the classifier press pulse"
        );
        assert_eq!(
            manager.light_state(),
            LightState::default(),
            "an operation snapshot never moves a light"
        );
        let second = TouchEvent {
            points: [TouchPoint {
                id: 1,
                x: 3,
                y: 4,
                status: TouchStatus::Contact,
            }; MAX_TOUCH_POINTS],
            len: 1,
        };
        manager.apply_operation(op(InputEvent::Touch(second)));
        assert_eq!(
            manager.touch_state(),
            Some(second),
            "the latest frame replaces the previous snapshot"
        );
    }

    #[test]
    fn heartbeat_counts_only_applied_frames() {
        let mut manager = DeviceManager::new();
        assert_eq!(manager.state().touch.frames, 0);
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 10,
            y: 20,
        })));
        assert_eq!(
            manager.state().touch.frames,
            0,
            "a classifier press pulse is not a raw frame"
        );
        manager.apply_operation(touch_event(TouchStatus::Down));
        manager.apply_operation(touch_event(TouchStatus::Contact));
        assert_eq!(
            manager.state().touch.frames,
            2,
            "every stored snapshot ticks the heartbeat once"
        );
        manager.apply_operation(tap());
        assert_eq!(
            manager.state().touch.frames,
            2,
            "resolved gestures never tick the raw-frame heartbeat"
        );
    }
}

mod tallies {
    use super::*;

    #[test]
    fn tap_tallies_stamps_origin_and_accumulates() {
        let mut manager = DeviceManager::new();
        for _ in 0..3 {
            manager.apply_operation(tap());
        }
        let state = manager.state();
        assert_eq!(state.touch.tap_count, 3);
        assert_eq!(
            state.touch.last_gesture_origin,
            Some((1, 2)),
            "the tap records its touch point as the gesture origin"
        );
        assert_eq!(
            manager.light_state(),
            LightState::default(),
            "the tally never moves the light; only the translated business intent does"
        );
    }

    #[test]
    fn long_press_tallies_on_its_own_row() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(long_press());
        assert_eq!(manager.state().touch.long_press_count, 1);
        assert_eq!(
            manager.state().touch.press_count,
            0,
            "the hold's down-edge press and the long-press are separate tallies"
        );
    }

    #[test]
    fn tap_count_wraps_at_255() {
        let mut manager = DeviceManager::new();
        for _ in 0..(u8::MAX as u32).saturating_add(1) {
            manager.apply_operation(tap());
        }
        assert_eq!(
            manager.state().touch.tap_count,
            0,
            "255 taps wrap to 0, keeping the diagnostic digit bounded"
        );
    }

    #[test]
    fn press_pulse_tallies_upserts_and_wraps() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(press(0, 12, 34));
        manager.apply_operation(press(0, 12, 34));
        assert_eq!(manager.state().touch.press_count, 2);
        assert_eq!(
            manager.state().touch.points,
            [
                Some(LivePoint {
                    id: 0,
                    x: 12,
                    y: 34
                }),
                None
            ]
        );
        let mut wraps = DeviceManager::new();
        for _ in 0..(u8::MAX as u32).saturating_add(1) {
            wraps.apply_operation(press(0, 12, 34));
        }
        assert_eq!(
            wraps.state().touch.press_count,
            0,
            "255 presses wrap to 0, keeping the diagnostic digit bounded"
        );
    }

    #[test]
    fn ghost_tallies_and_saturates() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        assert_eq!(manager.state().touch.ghost_count, 2);
        for _ in 0..(u8::MAX as u32).saturating_add(1) {
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        }
        assert_eq!(
            manager.state().touch.ghost_count,
            u8::MAX,
            "the ghost tally saturates instead of wrapping, preserving the record"
        );
    }

    #[test]
    fn swipe_tallies_records_travel_and_wraps() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(swipe());
        let state = manager.state();
        assert_eq!(state.touch.swipe_count, 1);
        assert_eq!(state.touch.last_swipe, Some((SwipeDirection::Down, 73)));
        assert_eq!(state.touch.last_gesture_origin, Some((80, 30)));
        assert_eq!(state.touch.last_gesture_end, Some((80, 60)));
        assert_eq!(state.touch.held_ms, 42);
        assert_eq!(
            state.lights[0],
            LightState::default(),
            "the swipe op tallies; the light move is the translated business intent"
        );
        assert_eq!(
            state.touch.points,
            [None, None],
            "resolution frees the live slot"
        );
        assert_eq!(
            state.touch.live_dir[0], 0,
            "resolution clears the movement arrow"
        );
        let mut wraps = DeviceManager::new();
        for _ in 0..(u8::MAX as u32).saturating_add(1) {
            wraps.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                x: 3,
                y: 9,
                end_x: 9,
                end_y: 3,
                direction: SwipeDirection::Up,
                held_ms: 0,
                distance_px: 60,
            })));
        }
        assert_eq!(
            wraps.state().touch.swipe_count,
            0,
            "255 swipes wrap to 0, keeping the diagnostic digit bounded"
        );
    }

    #[test]
    fn chip_gesture_stores_verbatim_without_touching_light() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(op(InputEvent::ChipGesture(0x10)));
        assert_eq!(manager.state().touch.chip_gesture_id, 0x10);
        assert_eq!(manager.light_state(), LightState::default());
    }
}

mod separation {
    use super::*;

    #[test]
    fn light_moves_only_via_a_business_intent() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(tap());
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        manager.apply_operation(press(0, 12, 34));
        manager.apply_operation(touch_event(TouchStatus::Contact));
        manager.apply_operation(touch_event(TouchStatus::Release));
        assert_eq!(manager.light_state(), LightState::default());
        assert_eq!(manager.state().touch.tap_count, 1);
        assert_eq!(manager.state().touch.ghost_count, 1);
        assert_eq!(
            manager.state().touch.press_count,
            1,
            "only a classifier press pulse earns a press tally, never a raw frame"
        );
        let target = LightState::Solid {
            color: Rgb(1, 2, 3),
            brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
        };
        manager.apply_business(set_light(0, target));
        assert_eq!(manager.light_state(), target);
        manager.apply_business(BusinessIntent::Invalid);
        assert_eq!(
            manager.light_state(),
            target,
            "an invalid intent is a no-op"
        );
        assert_eq!(
            manager.state().touch.tap_count,
            1,
            "the business move never inflates the operation tally"
        );
    }

    #[test]
    fn off_tap_full_path_records_diagnostics_and_keeps_off() {
        let mut manager = DeviceManager::new();
        manager.apply_business(set_light(0, LightState::Off));
        let op = tap();
        manager.apply_operation(op);
        let business = translate(&op, &manager.state());
        manager.apply_business(business);
        assert_eq!(business, BusinessIntent::Invalid);
        assert_eq!(manager.light_state(), LightState::Off);
        assert_eq!(manager.state().touch.tap_count, 1);
        assert_eq!(manager.state().touch.last_gesture_origin, Some((1, 2)));
    }

    #[test]
    fn off_swipe_full_path_records_diagnostics_and_keeps_off() {
        let mut manager = DeviceManager::new();
        manager.apply_business(set_light(0, LightState::Off));
        let op = swipe();
        manager.apply_operation(op);
        let business = translate(&op, &manager.state());
        manager.apply_business(business);
        assert_eq!(business, BusinessIntent::Invalid);
        assert_eq!(manager.light_state(), LightState::Off);
        assert_eq!(manager.state().touch.swipe_count, 1);
        assert_eq!(
            manager.state().touch.last_swipe,
            Some((SwipeDirection::Down, 73))
        );
    }
}

mod trackers {
    use super::*;
    use crate::drivers::input::{FINGER_SWIPE, FINGER_TAP};

    #[test]
    fn live_points_follow_down_contact_release() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 10,
                y: 20,
                status: TouchStatus::Down,
            }; MAX_TOUCH_POINTS],
            len: 1,
        })));
        manager.apply_operation(op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint {
                id: 1,
                x: 150,
                y: 40,
                status: TouchStatus::Contact,
            }; MAX_TOUCH_POINTS],
            len: 1,
        })));
        assert_eq!(
            manager.state().touch.points,
            [
                Some(LivePoint {
                    id: 0,
                    x: 10,
                    y: 20
                }),
                Some(LivePoint {
                    id: 1,
                    x: 150,
                    y: 40
                })
            ],
            "each tracker upserts into its own slot"
        );
        assert_eq!(
            manager.state().touch.two_finger_runs,
            1,
            "two live slots in one transition counts one run"
        );
        manager.apply_operation(op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 0,
                y: 0,
                status: TouchStatus::Release,
            }; MAX_TOUCH_POINTS],
            len: 1,
        })));
        assert_eq!(
            manager.state().touch.points,
            [
                None,
                Some(LivePoint {
                    id: 1,
                    x: 150,
                    y: 40
                })
            ],
            "a release frees only the tracker's own slot"
        );
        manager.apply_operation(op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint {
                id: 1,
                x: 0,
                y: 0,
                status: TouchStatus::Release,
            }; MAX_TOUCH_POINTS],
            len: 1,
        })));
        assert_eq!(manager.state().touch.points, [None, None]);
        assert_eq!(
            manager.state().touch.two_finger_runs,
            1,
            "releasing a finger never grows the run counter"
        );
    }

    #[test]
    fn live_dir_tracks_moves_and_keeps_axis_under_deadband() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 100,
            y: 200,
        })));
        assert_eq!(
            manager.state().touch.live_dir,
            [0, 0],
            "a fresh landing reads as still until the finger moves"
        );
        manager.apply_operation(touch_at(0, 100, 150, TouchStatus::Contact));
        assert_eq!(
            manager.state().touch.live_dir[0],
            SwipeDirection::Up.code(),
            "climbing the panel is an up-move in framebuffer space"
        );
        manager.apply_operation(touch_at(0, 60, 150, TouchStatus::Contact));
        assert_eq!(
            manager.state().touch.live_dir[0],
            SwipeDirection::Left.code()
        );
        manager.apply_operation(touch_at(0, 61, 151, TouchStatus::Contact));
        assert_eq!(
            manager.state().touch.live_dir[0],
            SwipeDirection::Left.code(),
            "sub-deadband jitter keeps the last axis instead of flickering to 0"
        );
    }

    #[test]
    fn live_dir_reads_each_finger_independently() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(touch_at(0, 100, 200, TouchStatus::Down));
        manager.apply_operation(touch_at(1, 200, 200, TouchStatus::Down));
        manager.apply_operation(touch_at(1, 150, 200, TouchStatus::Contact));
        assert_eq!(
            manager.state().touch.live_dir,
            [0, SwipeDirection::Left.code()],
            "each slot carries only its own finger's axis"
        );
        manager.apply_operation(touch_at(1, 0, 0, TouchStatus::Release));
        assert_eq!(
            manager.state().touch.live_dir,
            [0, 0],
            "releasing a finger frees its slot back to still"
        );
    }

    #[test]
    fn simultaneous_presses_upsert_slots_and_count_runs() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(press(0, 10, 20));
        assert_eq!(
            manager.state().touch.points,
            [
                Some(LivePoint {
                    id: 0,
                    x: 10,
                    y: 20
                }),
                None
            ],
            "the first press lands in the lowest free slot"
        );
        manager.apply_operation(press(1, 50, 60));
        assert_eq!(
            manager.state().touch.two_finger_runs,
            1,
            "two live slots count one two-finger run"
        );
        manager.apply_operation(press(0, 11, 21));
        assert_eq!(
            manager.state().touch.two_finger_runs,
            1,
            "an upsert over a live slot is not a new run"
        );
    }

    #[test]
    fn two_finger_runs_saturates_at_255() {
        let mut manager = DeviceManager::new();
        let down = [
            TouchPoint {
                id: 1,
                x: 10,
                y: 10,
                status: TouchStatus::Down,
            },
            TouchPoint {
                id: 2,
                x: 150,
                y: 90,
                status: TouchStatus::Down,
            },
        ];
        let up = [
            TouchPoint {
                id: 1,
                x: 0,
                y: 0,
                status: TouchStatus::Release,
            },
            TouchPoint {
                id: 2,
                x: 0,
                y: 0,
                status: TouchStatus::Release,
            },
        ];
        let mut down_snapshot = [down[0]; MAX_TOUCH_POINTS];
        down_snapshot[1] = down[1];
        let mut up_snapshot = [up[0]; MAX_TOUCH_POINTS];
        up_snapshot[1] = up[1];
        for _ in 0..(u8::MAX as u32).saturating_add(1) {
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: down_snapshot,
                len: 2,
            })));
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: up_snapshot,
                len: 2,
            })));
        }
        assert_eq!(
            manager.state().touch.two_finger_runs,
            u8::MAX,
            "the run counter saturates instead of wrapping, preserving the record"
        );
    }

    #[test]
    fn gestures_stamp_their_own_slot_and_free_the_point() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(press(0, 40, 50));
        manager.apply_operation(press(1, 120, 30));
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            x: 10,
            y: 200,
            end_x: 10,
            end_y: 50,
            direction: SwipeDirection::Up,
            held_ms: 30,
            distance_px: 77,
        })));
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
            id: 1,
            x: 120,
            y: 40,
            held_ms: 90,
            at_ms: 90,
        })));
        assert_eq!(
            manager.state().touch.points,
            [None, None],
            "each resolved gesture frees only its own slot"
        );
        assert_eq!(
            manager.state().touch.finger,
            [
                FingerLast {
                    kind: FINGER_SWIPE,
                    value: 77,
                },
                FingerLast {
                    kind: FINGER_TAP,
                    value: 90,
                }
            ],
            "concurrent fingers record their own last gestures, slot-aligned"
        );
    }

    #[test]
    fn diagonal_swipe_frees_the_live_slot_and_arrow() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(touch_at(0, 60, 60, TouchStatus::Down));
        manager.apply_operation(touch_at(0, 80, 80, TouchStatus::Contact));
        let light = manager.state().lights[0];
        assert_ne!(
            manager.state().touch.live_dir[0],
            0,
            "the slide mints a diagonal arrow"
        );
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            x: 60,
            y: 60,
            end_x: 80,
            end_y: 80,
            direction: SwipeDirection::DownRight,
            held_ms: 40,
            distance_px: 28,
        })));
        let state = manager.state();
        assert_eq!(
            state.lights[0], light,
            "an operation never moves the light; the identity resolves in translate"
        );
        assert_eq!(state.touch.swipe_count, 1, "the diagonal still tallies");
        assert_eq!(
            state.touch.last_swipe,
            Some((SwipeDirection::DownRight, 28))
        );
        assert_eq!(
            state.touch.points[0], None,
            "resolution frees the live slot"
        );
        assert_eq!(
            state.touch.live_dir[0], 0,
            "resolution clears the movement arrow"
        );
    }

    #[test]
    fn last_gesture_points_follow_resolved_gestures() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(swipe());
        assert_eq!(
            manager.state().touch.last_gesture_origin,
            Some((80, 30)),
            "a swipe records its origin"
        );
        assert_eq!(manager.state().touch.last_gesture_end, Some((80, 60)));

        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 11,
            y: 12,
            held_ms: 0,
            at_ms: 0,
        })));
        assert_eq!(
            manager.state().touch.last_gesture_origin,
            Some((11, 12)),
            "a tap stamps its own point as the origin"
        );
        assert_eq!(
            manager.state().touch.last_gesture_end,
            None,
            "a single-point tap clears the previous trailing point"
        );

        manager.apply_operation(swipe());
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x: 13,
            y: 14,
            held_ms: 0,
        })));
        assert_eq!(manager.state().touch.last_gesture_origin, Some((13, 14)));
        assert_eq!(
            manager.state().touch.last_gesture_end,
            None,
            "a single-point long-press clears the previous trailing point"
        );

        manager.apply_operation(swipe());
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            x: 21,
            y: 22,
            end_x: 23,
            end_y: 24,
            direction: SwipeDirection::DownRight,
            held_ms: 0,
            distance_px: 3,
        })));
        assert_eq!(
            manager.state().touch.last_gesture_origin,
            Some((21, 22)),
            "a slide shows where it started"
        );
        assert_eq!(
            manager.state().touch.last_gesture_end,
            Some((23, 24)),
            "a slide shows where it ended"
        );
    }
}

fn touch_at(id: u8, x: u16, y: u16, status: TouchStatus) -> OperationIntent {
    op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint { id, x, y, status }; MAX_TOUCH_POINTS],
        len: 1,
    }))
}

// The page's moves, walked exhaustively rather than along the paths a test happened to pick.
//
// The tests above are examples: one path somebody thought of, with the state it expected beside
// it. That is the wrong shape for a move whose *precondition* an earlier move took away — such a
// rule can be green for every state its own test walks into while being wrong for a state none of
// them does. So this walks the reachable states instead: a breadth-first sweep over every move the
// page has, from the state a booted device is in, with [`broken`]'s invariants checked on every
// state a move lands on. The moves that reached a broken state are printed with it, so a failure
// here is a reproduction rather than a verdict.
//
// Two limits of the alphabet decide what the sweep's answer is worth. Every report it sends is one
// the radio really makes — `Connected` carries an address, `Connecting` may or may not have a name
// yet, `Off` and `Scanning` carry neither — because a sweep free to invent a report would find
// states the device cannot be in and call them reachable. And states are compared through [`key`]:
// a field left out that a move reads would merge two states the page tells apart.
mod reachable {
    use super::*;
    use crate::keyboard::{self, DialogChoice, Key};
    use alloc::collections::{BTreeSet, VecDeque};
    use alloc::string::String;

    const LIST_X: u16 = 30;

    const SWEEP_CAP: usize = 20_000;

    fn key(state: &DeviceState) -> Vec<u8> {
        let mut key = Vec::with_capacity(64);
        key.push(state.page as u8);
        key.push(u8::from(state.network.link.is_leavable()));
        encode_ap(&mut key, state.network.link.ap);
        key.push(u8::from(state.network.link.address.is_some()));
        match state.network.aps {
            None => key.push(0),
            Some(list) => {
                key.push(1);
                key.push(list.len() as u8);
                for index in 0..list.len() {
                    let Some(ap) = list.get(index) else {
                        break;
                    };
                    encode_ap(&mut key, Some(ap));
                    key.push(u8::from(list.is_saved(index)));
                }
            }
        }
        encode_ap(&mut key, state.network.setup.selected);
        key.push(u8::from(state.network.setup.entry.is_some()));
        key.push(u8::from(state.network.setup.keyboard));
        key.push(state.network.setup.list_top);
        match state.network.setup.confirm {
            None => key.push(0),
            Some(Confirm::Leave) => key.push(1),
            Some(Confirm::Forget(ap)) => {
                key.push(2);
                encode_ap(&mut key, Some(ap));
            }
        }
        key.push(u8::from(state.network.leave));
        key.push(u8::from(state.network.scan_now));
        match state.network.join {
            None => key.push(0),
            Some(JoinRequest::Typed(_)) => key.push(1),
            Some(JoinRequest::Held(_)) => key.push(2),
        }
        key.push(u8::from(state.network.forget.is_some()));
        key
    }

    fn encode_ap(bytes: &mut Vec<u8>, ap: Option<AccessPoint>) {
        match ap {
            None => bytes.push(0),
            Some(ap) => {
                bytes.push(1);
                bytes.extend_from_slice(&ap.bssid);
                bytes.push(u8::from(ap.secured));
            }
        }
    }

    // The invariants the page's own drawing and hit rules rest on, read off a state some move
    // reached: `None` when they all hold.
    //
    // Each one is the negation of a state that draws nothing, offers nothing, or offers
    // something that is not there — the failures this page has no way to be loud about, because
    // what breaks is what the panel puts on the glass.
    fn broken(state: &DeviceState) -> Option<&'static str> {
        if state.network.setup.keyboard && state.network.setup.entry.is_none() {
            return Some(
                "the keyboard is up with no draft under it. The keys are drawn from the draft \
                     and the strip is hidden with them, so the page draws nothing between the \
                     header and the bottom edge, and no control on it can be reached",
            );
        }
        if state.network.setup.entry.is_some() && state.network.setup.selected.is_none() {
            return Some(
                "a draft is kept for no row, so nothing names the network it would be handed \
                     to",
            );
        }
        if state.network.setup.confirm == Some(Confirm::Leave) && !state.network.link.is_leavable()
        {
            return Some(
                "the page is asking about leaving a link that is not up, so the box has no \
                     answer left to take",
            );
        }
        if state.network.setup.rescan_wait && state.network.link.is_leavable() {
            return Some(
                "the strip is waiting for a survey round under a link it would offer to leave",
            );
        }
        None
    }

    fn path(origin: &[(Option<usize>, &'static str)], at: usize, this: &str) -> String {
        let mut names = Vec::new();
        let mut here = Some(at);
        while let Some(index) = here {
            let (parent, name) = origin[index];
            names.push(name);
            here = parent;
        }
        names.reverse();
        let mut text = String::new();
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            text.push_str(name);
        }
        if !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(this);
        text
    }

    fn band_y(band: usize) -> u16 {
        (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16
    }

    fn applied(manager: &mut DeviceManager, event: InputEvent) {
        let operation = op(event);
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    fn tap_at(manager: &mut DeviceManager, x: u16, y: u16) {
        applied(
            manager,
            InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms: 40,
            }),
        );
    }

    fn hold_at(manager: &mut DeviceManager, x: u16, y: u16) {
        applied(
            manager,
            InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x,
                y,
                held_ms: 500,
            }),
        );
    }

    fn swipe(manager: &mut DeviceManager, direction: SwipeDirection, end_y: u16) {
        let y = band_y(1);
        applied(
            manager,
            InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction,
                x: LIST_X,
                y,
                end_x: LIST_X,
                end_y,
                held_ms: 40,
                distance_px: 60,
            }),
        );
    }

    fn tap_key(manager: &mut DeviceManager, key: Key) {
        let cell = keyboard::cell_of(key)
            .or_else(|| {
                (0..keyboard::KEYS).find(|cell| keyboard::key_of(Layer::Abc, *cell) == Some(key))
            })
            .expect("the grid holds every key");
        let rect = keyboard::key_box(cell).expect("a cell of the grid");
        tap_at(
            manager,
            (rect.x + rect.w / 2) as u16,
            (rect.y + rect.h / 2) as u16,
        );
    }

    fn tap_answer(manager: &mut DeviceManager, choice: DialogChoice) {
        let choice = keyboard::dialog_answer_box(choice);
        tap_at(
            manager,
            (choice.x + choice.w / 2) as u16,
            (choice.y + choice.h / 2) as u16,
        );
    }

    fn ap_named(ssid: &[u8], last: u8) -> AccessPoint {
        AccessPoint {
            ssid: Ssid::from_octets(ssid),
            bssid: [0x02, 0, 0, 0, 0, last],
            channel: 6,
            rssi: -55,
            secured: true,
        }
    }

    fn neighbour() -> AccessPoint {
        ap_named(b"neighbour", 1)
    }

    fn cafe() -> AccessPoint {
        ap_named(b"cafe", 2)
    }

    fn link_to(manager: &mut DeviceManager, status: LinkStatus) {
        manager.apply_business(BusinessIntent::Network(NetworkIntent::LinkReport(status)));
    }

    fn scan_of(manager: &mut DeviceManager, rows: &[AccessPoint]) {
        manager.apply_scan(ApList::from_ranked(rows));
    }

    fn tap_first_band(manager: &mut DeviceManager) {
        tap_at(manager, LIST_X, band_y(0));
    }
    fn tap_second_band(manager: &mut DeviceManager) {
        tap_at(manager, LIST_X, band_y(1));
    }
    fn tap_third_band(manager: &mut DeviceManager) {
        tap_at(manager, LIST_X, band_y(2));
    }
    fn tap_the_strip(manager: &mut DeviceManager) {
        tap_at(
            manager,
            160,
            (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
        );
    }
    fn tap_the_header(manager: &mut DeviceManager) {
        tap_at(manager, 2, 2);
    }
    fn tap_keep(manager: &mut DeviceManager) {
        tap_answer(manager, DialogChoice::Keep);
    }
    fn tap_the_action(manager: &mut DeviceManager) {
        tap_answer(manager, DialogChoice::Act);
    }
    fn hold_first_band(manager: &mut DeviceManager) {
        hold_at(manager, LIST_X, band_y(0));
    }
    fn hold_second_band(manager: &mut DeviceManager) {
        hold_at(manager, LIST_X, band_y(1));
    }
    fn slide_down(manager: &mut DeviceManager) {
        swipe(manager, SwipeDirection::Down, band_y(2));
    }
    fn slide_up(manager: &mut DeviceManager) {
        swipe(manager, SwipeDirection::Up, 0);
    }
    fn tap_a_character(manager: &mut DeviceManager) {
        tap_key(manager, Key::Char(Layer::Abc.face(0)));
    }
    fn tap_shift(manager: &mut DeviceManager) {
        tap_key(manager, Key::Shift);
    }
    fn tap_backspace(manager: &mut DeviceManager) {
        tap_key(manager, Key::Backspace);
    }
    fn tap_layer(manager: &mut DeviceManager) {
        tap_key(manager, Key::Layer);
    }
    fn tap_reveal(manager: &mut DeviceManager) {
        tap_key(manager, Key::Reveal);
    }
    fn tap_connect(manager: &mut DeviceManager) {
        tap_key(manager, Key::Connect);
    }
    fn tap_cancel(manager: &mut DeviceManager) {
        tap_key(manager, Key::Cancel);
    }
    fn triple_click(manager: &mut DeviceManager) {
        applied(manager, InputEvent::Button(ButtonEvent::TripleClick));
    }
    fn splash_over(manager: &mut DeviceManager) {
        manager.apply_business(BusinessIntent::Page(PageIntent::SplashOver));
    }
    fn click(manager: &mut DeviceManager) {
        applied(manager, InputEvent::Button(ButtonEvent::Click));
    }
    fn hold_the_button(manager: &mut DeviceManager) {
        applied(manager, InputEvent::Button(ButtonEvent::LongPress));
    }

    fn nothing_on_the_air(manager: &mut DeviceManager) {
        link_to(manager, LinkStatus::default());
    }
    fn looking_around(manager: &mut DeviceManager) {
        link_to(
            manager,
            LinkStatus {
                phase: LinkPhase::Scanning,
                ..LinkStatus::default()
            },
        );
    }
    fn attempt_without_a_name(manager: &mut DeviceManager) {
        link_to(
            manager,
            LinkStatus {
                phase: LinkPhase::Connecting,
                ap: None,
                address: None,
                ..LinkStatus::default()
            },
        );
    }
    fn associated(manager: &mut DeviceManager) {
        link_to(
            manager,
            LinkStatus {
                phase: LinkPhase::Connecting,
                ap: Some(neighbour()),
                address: None,
                ..LinkStatus::default()
            },
        );
    }
    fn connected(manager: &mut DeviceManager) {
        link_to(
            manager,
            LinkStatus {
                phase: LinkPhase::Connected,
                ap: Some(neighbour()),
                address: Some(Ipv4::from_octets([10, 0, 0, 2])),
                ..LinkStatus::default()
            },
        );
    }
    fn refused(manager: &mut DeviceManager) {
        link_to(
            manager,
            LinkStatus {
                phase: LinkPhase::Failed(NetworkError::Credentials),
                ap: Some(neighbour()),
                address: None,
                ..LinkStatus::default()
            },
        );
    }
    fn the_driver_refused(manager: &mut DeviceManager) {
        link_to(
            manager,
            LinkStatus {
                phase: LinkPhase::Failed(NetworkError::Driver),
                ap: None,
                address: None,
                ..LinkStatus::default()
            },
        );
    }

    fn scan_two(manager: &mut DeviceManager) {
        scan_of(manager, &[neighbour(), cafe()]);
    }
    fn scan_one_open(manager: &mut DeviceManager) {
        scan_of(
            manager,
            &[AccessPoint {
                secured: false,
                ..neighbour()
            }],
        );
    }
    fn scan_with_a_saved_row(manager: &mut DeviceManager) {
        let mut list = ApList::from_ranked(&[neighbour(), cafe()]);
        let mut store = SavedNetworks::empty();
        let credentials =
            Credentials::from_text("neighbour", "latte").expect("a credential the radio takes");
        store.remember(&credentials);
        let marks = store.mask(&list);
        list.mark_saved(marks);
        manager.apply_scan(list);
    }

    fn the_leave_is_carried_out(manager: &mut DeviceManager) {
        if manager.take_leave() {
            nothing_on_the_air(manager);
        }
    }
    fn the_typed_join_is_carried_out(manager: &mut DeviceManager) {
        if let Some(JoinRequest::Typed(_)) = manager.take_join() {
            attempt_without_a_name(manager);
        }
    }
    fn the_held_join_is_carried_out(manager: &mut DeviceManager) {
        if let Some(JoinRequest::Held(_)) = manager.take_join() {
            associated(manager);
        }
    }
    fn the_missing_copy_is_reported(manager: &mut DeviceManager) {
        if let Some(JoinRequest::Held(ssid)) = manager.take_join() {
            manager.apply_business(BusinessIntent::Network(NetworkIntent::NoCopy(ssid)));
        }
    }
    fn the_scan_request_is_carried_out(manager: &mut DeviceManager) {
        if manager.take_scan_now() {
            looking_around(manager);
            scan_two(manager);
        }
    }
    fn the_forget_is_carried_out(manager: &mut DeviceManager) {
        if manager.take_forget().is_some() {
            let mut list = manager.state().network.aps.unwrap_or_else(ApList::empty);
            list.mark_saved(0);
            manager.apply_scan(list);
        }
    }

    struct Move {
        name: &'static str,
        run: fn(&mut DeviceManager),
    }

    fn alphabet() -> Vec<Move> {
        alloc::vec![
            Move {
                name: "a tap on the first band",
                run: tap_first_band
            },
            Move {
                name: "a tap on the second band",
                run: tap_second_band
            },
            Move {
                name: "a tap on the third band",
                run: tap_third_band
            },
            Move {
                name: "a tap on the strip",
                run: tap_the_strip
            },
            Move {
                name: "a tap on the header",
                run: tap_the_header
            },
            Move {
                name: "a tap on the box's KEEP",
                run: tap_keep
            },
            Move {
                name: "a tap on the box's action",
                run: tap_the_action
            },
            Move {
                name: "a hold on the first band",
                run: hold_first_band
            },
            Move {
                name: "a hold on the second band",
                run: hold_second_band
            },
            Move {
                name: "a slide down the list",
                run: slide_down
            },
            Move {
                name: "a slide up the list",
                run: slide_up
            },
            Move {
                name: "a tap on a character key",
                run: tap_a_character
            },
            Move {
                name: "a tap on shift",
                run: tap_shift
            },
            Move {
                name: "a tap on backspace",
                run: tap_backspace
            },
            Move {
                name: "a tap on the layer key",
                run: tap_layer
            },
            Move {
                name: "a tap on reveal",
                run: tap_reveal
            },
            Move {
                name: "a tap on connect",
                run: tap_connect
            },
            Move {
                name: "a tap on cancel",
                run: tap_cancel
            },
            Move {
                name: "a triple click",
                run: triple_click
            },
            Move {
                name: "a click",
                run: click
            },
            Move {
                name: "a long press of the button",
                run: hold_the_button
            },
            Move {
                name: "the splash's clock",
                run: splash_over
            },
            Move {
                name: "the radio reporting nothing on the air",
                run: nothing_on_the_air
            },
            Move {
                name: "the radio looking around",
                run: looking_around
            },
            Move {
                name: "an attempt with no name yet",
                run: attempt_without_a_name
            },
            Move {
                name: "the radio associating",
                run: associated
            },
            Move {
                name: "the radio connected",
                run: connected
            },
            Move {
                name: "the credential refused",
                run: refused
            },
            Move {
                name: "the driver refusing",
                run: the_driver_refused
            },
            Move {
                name: "a scan of two networks",
                run: scan_two
            },
            Move {
                name: "a scan of an open network",
                run: scan_one_open
            },
            Move {
                name: "a scan with a saved row",
                run: scan_with_a_saved_row
            },
            Move {
                name: "the leave carried out",
                run: the_leave_is_carried_out
            },
            Move {
                name: "the typed credential carried out",
                run: the_typed_join_is_carried_out
            },
            Move {
                name: "the held copy carried out",
                run: the_held_join_is_carried_out
            },
            Move {
                name: "the store reporting no copy",
                run: the_missing_copy_is_reported
            },
            Move {
                name: "the scan request carried out",
                run: the_scan_request_is_carried_out
            },
            Move {
                name: "the forget carried out",
                run: the_forget_is_carried_out
            },
        ]
    }

    #[test]
    fn every_state_the_page_can_reach_keeps_its_invariants() {
        let moves = alphabet();
        let start = DeviceManager::boot(Setup {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            ..Default::default()
        });
        assert_eq!(
            broken(&start.state()),
            None,
            "a booted device is already in a state the page cannot draw"
        );

        let mut states: Vec<DeviceManager> = alloc::vec![start];
        let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
        seen.insert(key(&states[0].state()));
        let mut queue = VecDeque::from([0usize]);
        let mut origin: Vec<(Option<usize>, &'static str)> =
            alloc::vec![(None, "the device at boot")];
        let mut walked = 0usize;

        while let Some(here) = queue.pop_front() {
            walked += 1;
            assert!(
                walked <= SWEEP_CAP,
                "the sweep reached {SWEEP_CAP} states without settling: raise the cap, or cut \
                     the alphabet down to what the page really reads"
            );
            for step in &moves {
                let mut next = states[here].clone();
                (step.run)(&mut next);
                let state = next.state();
                if let Some(why) = broken(&state) {
                    panic!(
                        "the page can reach a state where {why}\nreached by {}",
                        path(&origin, here, step.name),
                    );
                }
                if seen.insert(key(&state)) {
                    states.push(next);
                    origin.push((Some(here), step.name));
                    queue.push_back(states.len() - 1);
                }
            }
        }

        let reached = |wanted: fn(&DeviceState) -> bool| {
            states.iter().any(|manager| wanted(&manager.state()))
        };
        assert!(
            reached(|state| state.network.setup.keyboard && state.network.setup.entry.is_some()),
            "the keyboard never came up over a draft"
        );
        assert!(
            reached(|state| state.network.setup.selected.is_some()
                && state.network.setup.entry.is_none()),
            "a row was never left picked with no draft: the state a link that came up leaves \
                 behind, and the one the pick after it reads"
        );
        assert!(
            reached(|state| state.network.setup.confirm == Some(Confirm::Leave)),
            "the strip never asked about leaving"
        );
        assert!(
            reached(|state| matches!(state.network.setup.confirm, Some(Confirm::Forget(_)))),
            "a hold never asked about a saved row"
        );
        assert!(
            reached(|state| state.network.leave),
            "a leave was never waiting for the network task"
        );
        assert!(
            reached(|state| state.network.scan_now),
            "the strip never asked to look around"
        );
        assert!(
            reached(|state| matches!(state.network.join, Some(JoinRequest::Typed(_)))),
            "a credential was never handed up"
        );
        assert!(
            reached(|state| matches!(state.network.join, Some(JoinRequest::Held(_)))),
            "a saved row was never asked for by name"
        );
        assert!(
            reached(|state| state.network.link.is_leavable()),
            "the link was never up"
        );
        assert!(
            reached(|state| state.page != DisplayPage::Network),
            "the page cycle was never walked"
        );
    }
}
