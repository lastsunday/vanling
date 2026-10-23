use super::*;
use crate::drivers::input::{ButtonEvent, MAX_TOUCH_POINTS, TouchEvent, TouchPoint, TouchStatus};
use crate::drivers::motion::MotionScale;
use crate::drivers::network::{
    AccessPoint, ApList, Credentials, Ipv4, LinkPhase, SavedNetworks, Ssid,
};
use crate::intent::translate;
use crate::state::page::CYCLE;

const TEST_SCALE: MotionScale = MotionScale::from_ranges(8_192, 64);

const BREATH: LightState = LightState::Breath(Breath {
    period_ms: 3_000,
    hue_period_ms: 60_000,
    hue_span: 255,
    min_brightness: 24,
    max_brightness: 80,
    saturation: 200,
    hue: 0,
    group: [16, 32, 3, 0, 0, 0, 0],
    group_len: 3,
});

fn op(event: InputEvent) -> OperationIntent {
    OperationIntent { source: 0, event }
}

fn tap() -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 1,
        y: 2,
        held_ms: 0,
        at_ms: 0,
    }))
}

fn triple_click() -> OperationIntent {
    op(InputEvent::Button(ButtonEvent::TripleClick))
}

fn long_press() -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::LongPress {
        id: 0,
        x: 3,
        y: 4,
        held_ms: 0,
    }))
}

fn swipe() -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        x: 80,
        y: 30,
        end_x: 80,
        end_y: 60,
        direction: SwipeDirection::Down,
        held_ms: 42,
        distance_px: 73,
    }))
}

fn touch_event(status: TouchStatus) -> OperationIntent {
    op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 0,
            x: 1,
            y: 2,
            status,
        }; MAX_TOUCH_POINTS],
        len: 1,
        contacts: 1,
    }))
}

fn set_light(instance: u8, state: LightState) -> BusinessIntent {
    BusinessIntent::SetLight { instance, state }
}

fn press(id: u8, x: u16, y: u16) -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::Press { id, x, y }))
}

/// Applies one operation and the business intent it resolves to, the way the
/// render loop does.
fn apply_page(manager: &mut DeviceManager, operation: OperationIntent) {
    manager.apply_operation(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_business(business);
}

/// A manager with its splash behind it: the device every test but the splash's own is about.
///
/// Every device boots through the splash (see [`DisplayPage::Splash`]), and the page is left by
/// the input task's clock — so a test that is about what a running device does starts here.
fn landed(mut manager: DeviceManager) -> DeviceManager {
    manager.apply_business(BusinessIntent::SplashOver);
    assert_ne!(manager.state().page, DisplayPage::Splash);
    manager
}

mod demand {
    use super::*;
    use crate::config::{MotionConfig, Unreachable};
    use crate::demand::{ALL_PLANES, PlaneKey};

    fn fully_wired() -> DeviceManager {
        DeviceManager::with_motion(true, MotionCapabilities::EMPTY)
            .with_planes(alloc::vec![
                PlaneKey::MOTION,
                PlaneKey::AUDIO,
                PlaneKey::CAMERA,
                PlaneKey::DISPLAY
            ])
            .with_audio(true)
            .with_playback(true)
            .with_camera(true)
    }

    #[test]
    fn a_pages_class_follows_the_page_that_draws_the_plane() {
        // The rule this pins: the class is a property of the page asking for it, not of
        // the plane. Getting it backwards is silent.
        assert_eq!(
            DisplayPage::Attitude
                .demand(ALL_PLANES)
                .plane(PlaneKey::MOTION)
                .class,
            Sensitivity::Primary,
            "the page that draws the motion sample may wait nothing for it"
        );
        assert_eq!(
            DisplayPage::Audio
                .demand(ALL_PLANES)
                .plane(PlaneKey::AUDIO)
                .class,
            Sensitivity::Primary,
            "and the same for the page that draws the envelope"
        );

        // Everywhere else the reading is background to what is on screen, so a budget may
        // batch it.
        for page in CYCLE {
            if matches!(page, DisplayPage::Attitude | DisplayPage::Camera) {
                continue;
            }
            assert_eq!(
                page.demand(ALL_PLANES).plane(PlaneKey::MOTION).class,
                Sensitivity::Secondary,
                "{page:?}"
            );
        }

        // The mode is unchanged by any of this: the class decides how long a reading
        // may go unread.
        for page in CYCLE {
            assert_eq!(
                page.demand(ALL_PLANES).plane(PlaneKey::MOTION).mode,
                Mode::Continuous,
                "{page:?}"
            );
        }
    }

    #[test]
    fn a_plane_the_board_did_not_declare_is_never_claimed() {
        // The plane list is the board's whole statement of what it can be asked about,
        // so a plane left out of it is not claimed at Off — it is absent, and absent
        // reads as Off without anyone having to say so twice.
        let panel_only = DeviceManager::new().with_planes(alloc::vec![PlaneKey::DISPLAY]);
        let demand = panel_only.demand();

        assert_eq!(demand.plane(PlaneKey::MOTION).mode, Mode::Off);
        assert_eq!(demand.plane(PlaneKey::AUDIO).mode, Mode::Off);
        assert_eq!(demand.plane(PlaneKey::CAMERA).mode, Mode::Off);
        assert_eq!(demand.plane(PlaneKey::DISPLAY).mode, Mode::Continuous);
        assert!(
            !demand.keys().any(|key| key.field() == DemandField::Motion),
            "a plane the board never declared must not appear in the demand at all"
        );
    }

    #[test]
    fn a_page_claims_every_plane_it_is_asked_about_and_nothing_more() {
        // `claim_for` is exhaustive per plane, so the page cannot forget one and inherit a
        // demand nobody chose. The page's list is the board's, so this is also the only
        // place a page decides what exists.
        let claimed = DisplayPage::Camera.demand(ALL_PLANES);

        assert_eq!(claimed.keys().count(), ALL_PLANES.len());
        assert_eq!(claimed.plane(PlaneKey::CAMERA).mode, Mode::Continuous);
        for page in CYCLE {
            assert_eq!(
                page.demand(ALL_PLANES).keys().count(),
                ALL_PLANES.len(),
                "{page:?} must claim every plane the board declared"
            );
        }
    }

    #[test]
    fn only_the_page_that_draws_the_envelope_claims_the_microphone() {
        // The capture is due every tick and is the most expensive of the input task's four
        // sources, so a page that reads no samples must not keep it running.
        for page in CYCLE {
            let claimed = page.demand(ALL_PLANES).plane(PlaneKey::AUDIO);
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
        // The reason the key carries an instance: two sensors of one type are two planes
        // the product can name separately.
        let twin = DeviceManager::new().with_planes(alloc::vec![
            PlaneKey::instance(DemandField::Audio, 0),
            PlaneKey::instance(DemandField::Audio, 1),
        ]);
        let mut demand = twin.demand().clone();
        demand.set(
            PlaneKey::instance(DemandField::Audio, 0),
            PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
        );
        demand.set(
            PlaneKey::instance(DemandField::Audio, 1),
            PlaneDemand::new(Mode::Continuous, 5_000, Sensitivity::Secondary),
        );

        assert_eq!(
            demand
                .plane(PlaneKey::instance(DemandField::Audio, 0))
                .period_ms,
            0,
            "narrowing one sensor must not reach its twin"
        );
        assert_eq!(
            demand
                .plane(PlaneKey::instance(DemandField::Audio, 1))
                .period_ms,
            5_000
        );
    }

    #[test]
    fn demand_follows_the_page_instead_of_lagging_it() {
        let mut manager = landed(fully_wired());
        let before = manager.demand().clone();

        manager.apply_business(BusinessIntent::NextPage);

        assert_ne!(
            manager.demand(),
            &before,
            "the page moved, so the demand it claims should have moved with it"
        );
        assert_eq!(
            manager.demand(),
            &DeviceManager::proposed(&manager.state, &manager.planes, &manager.override_),
            "a held demand a fresh derivation would contradict has fallen behind"
        );
    }

    #[test]
    fn every_write_that_feeds_a_proposal_reaches_the_held_demand() {
        // The held demand is only as fresh as the last `propose_demand`. These four
        // fields are everything a proposal is derived from, so every write path that
        // touches one has to re-derive — going through the public entry points rather
        // than poking `state` is what makes a missed recompute visible here.
        let mut by_page = landed(fully_wired());
        by_page.apply_business(BusinessIntent::NextPage);
        assert_eq!(
            by_page.demand(),
            &DeviceManager::proposed(&by_page.state, &by_page.planes, &by_page.override_)
        );

        for manager in [
            DeviceManager::with_motion(false, MotionCapabilities::EMPTY).with_camera(true),
            DeviceManager::with_motion(false, MotionCapabilities::EMPTY).with_audio(true),
            DeviceManager::with_motion(false, MotionCapabilities::EMPTY).with_playback(true),
            DeviceManager::with_motion(true, MotionCapabilities::EMPTY)
                .with_boot_page(DisplayPage::Attitude),
        ] {
            assert_eq!(
                manager.demand(),
                &DeviceManager::proposed(&manager.state, &manager.planes, &manager.override_)
            );
        }
    }

    #[test]
    fn a_committed_demand_survives_being_read_back() {
        // What the configurator writes is what everyone downstream must see, even
        // though a fresh derivation would say otherwise.
        let mut manager = landed(fully_wired().with_boot_page(DisplayPage::Camera));
        assert_eq!(manager.state().page, DisplayPage::Camera);
        assert_eq!(
            manager.demand().plane(PlaneKey::CAMERA).mode,
            Mode::Continuous
        );

        let narrowed = manager.demand().without(PlaneKey::MOTION);
        manager.commit_demand(narrowed);

        assert_eq!(manager.demand().plane(PlaneKey::MOTION).mode, Mode::Off);
        assert_eq!(
            manager.demand().plane(PlaneKey::CAMERA).mode,
            Mode::Continuous
        );
        assert_ne!(
            manager.demand(),
            &DeviceManager::proposed(&manager.state, &manager.planes, &manager.override_),
            "a committed demand is the one place the two are allowed to differ"
        );
    }

    #[test]
    fn a_refused_plane_leaves_the_committed_demand_but_nothing_else_does() {
        // What the render loop does with a refusal: the plane it names stops being claimed,
        // so no consumer downstream reads a promise the part cannot keep. The other planes
        // are untouched, and the interaction path is untouched whatever it says.
        let mut manager = landed(fully_wired());
        let refused = Unreachable {
            key: PlaneKey::MOTION,
            mode: Mode::OnChange,
            config: MotionConfig::PolledRegisters,
        };

        manager.commit_demand(manager.demand().without(refused.key));

        assert_eq!(manager.demand().plane(PlaneKey::MOTION).mode, Mode::Off);
        assert_eq!(
            manager.demand().plane(PlaneKey::DISPLAY).mode,
            Mode::Continuous,
            "one plane going unmet is not a reason to drop the others"
        );
        assert_eq!(
            manager.demand().plane(PlaneKey::INTERACTION).mode,
            Mode::Continuous,
            "nothing a demand says may switch the interaction path off"
        );
    }

    #[test]
    fn a_page_change_re_proposes_over_a_committed_demand() {
        // A committed demand is a statement about now. The next page is a new claim, and
        // the proposal has to come back — otherwise one refusal would silence the plane for
        // as long as the device is on.
        let mut manager = landed(fully_wired());
        manager.commit_demand(manager.demand().without(PlaneKey::MOTION));
        assert_eq!(manager.demand().plane(PlaneKey::MOTION).mode, Mode::Off);

        manager.apply_business(BusinessIntent::NextPage);

        assert_ne!(
            manager.demand().plane(PlaneKey::MOTION).mode,
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
        assert_eq!(state.tap_count, 0);
        assert_eq!(state.press_count, 0);
        assert_eq!(state.long_press_count, 0);
        assert_eq!(state.ghost_count, 0);
        assert_eq!(state.swipe_count, 0);
        assert_eq!(state.two_finger_runs, 0);
        assert_eq!(state.touch_frames, 0);
        assert_eq!(state.chip_gesture_id, 0);
        assert_eq!(state.last_swipe, None);
        assert_eq!(state.last_gesture_origin, None);
        assert_eq!(state.last_gesture_end, None);
    }
}

mod lamp {
    use super::*;
    use crate::drivers::network::Front;

    fn lamp_only() -> DeviceManager {
        DeviceManager::new()
            .with_network(true)
            .with_panel(Panel::Absent)
    }

    fn report(manager: &mut DeviceManager, status: LinkStatus) {
        manager.apply_business(BusinessIntent::LinkReport(status));
    }

    fn setup(manager: &mut DeviceManager) {
        report(
            manager,
            LinkStatus {
                setup: true,
                front: Front::AccessPoint,
                ..LinkStatus::default()
            },
        );
    }

    fn connecting(manager: &mut DeviceManager, saved: bool) {
        report(
            manager,
            LinkStatus {
                phase: LinkPhase::Connecting,
                saved,
                ..LinkStatus::default()
            },
        );
    }

    fn connected(manager: &mut DeviceManager) {
        report(
            manager,
            LinkStatus {
                phase: LinkPhase::Connected,
                address: Some(Ipv4::from_octets([192, 168, 4, 2])),
                saved: true,
                ..LinkStatus::default()
            },
        );
    }

    fn failed(manager: &mut DeviceManager, kind: NetworkError, saved: bool) {
        report(
            manager,
            LinkStatus {
                phase: LinkPhase::Failed(kind),
                saved,
                ..LinkStatus::default()
            },
        );
    }

    /// Run the announcement out in frame steps, asserting it ends inside a budget.
    fn drain(manager: &mut DeviceManager, budget_ms: u32) {
        manager.tick_notice(0);
        let mut elapsed = 0;
        while manager.notice_active() {
            manager.tick_notice(elapsed);
            elapsed += 20;
            assert!(elapsed < budget_ms, "the announcement never ended");
        }
    }

    const GREEN: LightState = LightState::Solid {
        color: Rgb(0, 255, 0),
        brightness: 64,
    };
    const AMBER: LightState = LightState::Solid {
        color: Rgb(255, 155, 0),
        brightness: 96,
    };
    const RED: LightState = LightState::Solid {
        color: Rgb(255, 0, 0),
        brightness: 96,
    };

    #[test]
    fn the_status_lamp_belongs_to_a_board_with_a_radio_and_no_screen() {
        // An unconfigured device is not a mode: its news is the splash's amber burst, and after
        // that the page owns the surface. The same device state on a board with a screen is the
        // page's, not the lamp's, and a board with no radio has no link to report.
        assert_eq!(lamp_only().state().status_light(), None);
        let screened = DeviceManager::new()
            .with_network(true)
            .with_panel(Panel::Wired);
        assert_eq!(screened.state().status_light(), None);
        let no_radio = DeviceManager::new()
            .with_network(false)
            .with_panel(Panel::Absent);
        assert_eq!(no_radio.state().status_light(), None);
    }

    #[test]
    fn the_lamp_tells_the_mode_from_the_attempt_and_from_the_resting_state() {
        let mut manager = lamp_only();
        setup(&mut manager);
        assert_eq!(manager.state().status_light(), Some(LightState::SETUP));

        // A credential is in hand and the radio is acting on it: the same blue, a step quicker.
        connecting(&mut manager, false);
        assert_eq!(manager.state().status_light(), Some(LightState::JOINING));

        // A credential the store already keeps is the product's own reconnect, not a mode: the
        // page keeps its light while the radio tries.
        connecting(&mut manager, true);
        assert_eq!(manager.state().status_light(), None);

        // And a device that keeps a network and is merely off it is not news either.
        report(
            &mut manager,
            LinkStatus {
                phase: LinkPhase::Off,
                saved: true,
                ..LinkStatus::default()
            },
        );
        assert_eq!(manager.state().status_light(), None);
    }

    #[test]
    fn a_join_that_worked_is_green_and_then_hands_the_lamp_back() {
        let mut manager = lamp_only();
        connecting(&mut manager, true);
        connected(&mut manager);
        assert_eq!(manager.state().status_light(), Some(GREEN));

        manager.tick_notice(0);
        assert_eq!(
            manager.state().status_light(),
            Some(GREEN),
            "the announcement is drawn before it is stepped away"
        );
        drain(&mut manager, 2_000);
        assert!(!manager.notice_active(), "the news is over");
        assert_eq!(
            manager.state().status_light(),
            None,
            "a saved device that is connected has nothing left to announce"
        );
    }

    #[test]
    fn the_news_outranks_the_mode_it_arrived_in() {
        // The green lands first and the radio's next report still says the access point is up:
        // the newest news is what the lamp is for, not the mode it interrupts.
        let mut manager = lamp_only();
        setup(&mut manager);
        connected(&mut manager);
        setup(&mut manager);
        assert_eq!(manager.state().status_light(), Some(GREEN));
    }

    #[test]
    fn a_refusal_flashes_and_ends_and_the_same_phase_does_not_restart_it() {
        let mut manager = lamp_only();
        failed(&mut manager, NetworkError::Credentials, false);
        assert_eq!(
            manager.state().status_light(),
            Some(RED),
            "a refusal starts lit"
        );
        manager.tick_notice(0);
        let announced = manager.state().notice;
        assert_eq!(
            announced.and_then(|notice| notice.started_ms),
            Some(0),
            "the first frame stamps the announcement"
        );

        // The survey republishes the same refusal every round; that is one event, not a new one.
        failed(&mut manager, NetworkError::Credentials, false);
        assert_eq!(manager.state().notice, announced);

        drain(&mut manager, 2_000);
        assert_eq!(
            manager.state().status_light(),
            None,
            "a refusal is an event, not the lamp's resting colour"
        );
    }

    #[test]
    fn a_boot_with_nothing_saved_flashes_amber_and_then_shows_the_page() {
        let mut manager = lamp_only();
        assert_eq!(manager.state().page, DisplayPage::Splash);
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
        assert_eq!(manager.state().status_light(), Some(AMBER));

        drain(&mut manager, 2_000);
        assert_eq!(
            manager.state().status_light(),
            None,
            "the page owns the lamp again once the news is over"
        );
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn a_splash_hold_opens_setup_instead_of_flashing_offline() {
        // The hold is the answer to an unconfigured device: the access point is the mode it
        // lands in, so there is nothing to announce as missing.
        let mut manager = lamp_only();
        manager.apply_business(BusinessIntent::RequestSetup);
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(manager.state().notice, None);
        assert_eq!(manager.state().page, DisplayPage::Network);
    }

    #[test]
    fn a_missing_network_is_amber_and_a_refused_credential_is_red() {
        // Not found is the one failure a person fixes by moving the device; a refused credential
        // is the one they fix by typing again.
        let mut missing = lamp_only();
        failed(&mut missing, NetworkError::NotFound, true);
        assert_eq!(
            missing.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
        assert_eq!(missing.state().status_light(), Some(AMBER));

        let mut refused = lamp_only();
        failed(&mut refused, NetworkError::Credentials, false);
        assert_eq!(
            refused.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Refused)
        );
        assert_eq!(refused.state().status_light(), Some(RED));
    }

    #[test]
    fn a_retried_failure_is_announced_once_until_the_link_connects() {
        let mut manager = lamp_only();
        failed(&mut manager, NetworkError::NotFound, true);
        drain(&mut manager, 2_000);

        // The retry leaves `Failed`, tries again and fails the same way: one outage, one burst.
        connecting(&mut manager, true);
        failed(&mut manager, NetworkError::NotFound, true);
        assert_eq!(manager.state().notice, None, "the retry is not new news");

        // Coming back clears the latch, so the next outage is news again.
        connected(&mut manager);
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Joined)
        );
        drain(&mut manager, 2_000);
        failed(&mut manager, NetworkError::NotFound, true);
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
    }
}

mod motion {
    use super::*;
    use crate::drivers::motion::{MotionBatch, MotionEvent, MotionEvents, TiltDir};

    /// Every semantic a product may declare, so a case can assert one
    /// bucket moved without restating the other thirteen.
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

    /// Reads one bucket, so a case names the field it means instead of
    /// repeating the struct type in every closure.
    type Bucket = fn(&MotionCounts) -> u16;

    #[test]
    fn the_page_switch_skips_pages_the_board_lacks() {
        let mut disabled = landed(DeviceManager::new());
        apply_page(&mut disabled, triple_click());
        assert_eq!(disabled.state().page, DisplayPage::Ambient);

        let mut enabled = landed(DeviceManager::with_motion(true, ALL_CAPS));
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

        let mut enabled = DeviceManager::with_motion(true, ALL_CAPS);
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
        // The tap engine's knock count and the four tilt directions both
        // split one enum into several buckets, so a case that only proves
        // "something counted" would miss a crossed wire between them.
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
            let mut manager = DeviceManager::with_motion(true, ALL_CAPS);
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
        // Arbitration keeps at most one event per family, but the families
        // coexist: a shake and the tilt it causes ride the same batch and
        // both happened, so counting only the highest-priority one would
        // hide the tilt from the tally.
        let mut manager = DeviceManager::with_motion(true, ALL_CAPS);
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
        let mut manager = DeviceManager::with_motion(true, ALL_CAPS);
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
        assert_eq!(manager.state().motion_caps, MotionCapabilities::EMPTY);
    }

    #[test]
    fn a_knock_and_a_finger_tap_are_recorded_as_separate_acts() {
        // Same word, different planes: the panel reports a finger on glass
        // while the accelerometer under it reports the enclosure moving.
        // Arbitration runs within a plane, so neither cancels the other.
        let mut manager = DeviceManager::with_motion(true, ALL_CAPS);
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 20,
            y: 30,
            held_ms: 40,
            at_ms: 40,
        })));
        poll(&mut manager, &[MotionEvent::Tap { count: 1 }]);
        assert_eq!(manager.state().tap_count, 1, "the finger tap still tallies");
        assert_eq!(
            manager.motion_counts_state().taps,
            1,
            "the knock is not folded into the finger tap"
        );
    }
}

mod audio {
    use super::*;
    use crate::drivers::audio::SAMPLES_PER_COLUMN;
    use crate::drivers::motion::MotionCapabilities;

    const ALL_CAPS: MotionCapabilities = MotionCapabilities::TAP;

    /// A capture poll carrying one loud column, so a test can tell which
    /// envelope a latch came from.
    fn loud(amplitude: i16) -> AudioSample {
        let mut samples = [0_i16; SAMPLES_PER_COLUMN as usize];
        samples[SAMPLES_PER_COLUMN as usize / 2] = amplitude;
        let mut envelope = AudioEnvelope::default();
        envelope.push(&samples);
        AudioSample {
            envelope,
            elapsed_ms: 7,
            restarts: 0,
            dba_lsb: 0,
        }
    }

    fn apply(manager: &mut DeviceManager, operation: OperationIntent) {
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    /// A board with both capabilities cycles to the audio page in two
    /// page turns, so the tests start from there and tap on it.
    fn on_audio_page() -> DeviceManager {
        let mut manager = landed(DeviceManager::with_motion(true, ALL_CAPS).with_audio(true));
        apply(&mut manager, triple_click());
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Audio);
        manager
    }

    /// Taps the page the manager is already on, leaving the page alone.
    fn tap_in_place(manager: &mut DeviceManager) {
        let page = manager.state().page;
        apply(manager, tap());
        assert_eq!(
            manager.state().page,
            page,
            "a tap on audio records instead of paging"
        );
    }

    #[test]
    fn the_audio_page_only_appears_on_a_board_that_has_one() {
        assert_eq!(
            DisplayPage::Ambient.next_page(true, false, false, false, false),
            DisplayPage::Attitude
        );
        assert_eq!(
            DisplayPage::Attitude.next_page(true, false, false, false, false),
            DisplayPage::Ambient,
            "a board without audio skips the page"
        );
        assert_eq!(
            DisplayPage::Ambient.next_page(false, true, false, false, false),
            DisplayPage::Audio
        );
        assert_eq!(
            DisplayPage::Attitude.next_page(true, true, false, false, false),
            DisplayPage::Audio
        );
    }

    #[test]
    fn the_page_switch_reaches_the_audio_page_and_back() {
        let mut manager = landed(DeviceManager::new().with_audio(true));
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Audio);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn the_camera_page_only_appears_on_a_board_that_has_one() {
        // The camera is last in the cycle, so a board with nothing else reaches it
        // directly from Ambient.
        assert_eq!(
            DisplayPage::Ambient.next_page(false, false, false, true, false),
            DisplayPage::Camera
        );
        assert_eq!(
            DisplayPage::Ambient.next_page(false, false, false, false, false),
            DisplayPage::Ambient,
            "a board without a camera never reaches the page"
        );
        assert_eq!(
            DisplayPage::Audio.next_page(false, false, false, true, false),
            DisplayPage::Camera,
            "the camera follows the pages that need a wired source"
        );
        assert_eq!(
            DisplayPage::Audio.next_page(false, false, false, false, false),
            DisplayPage::Ambient,
            "and is skipped when there is none"
        );
    }

    #[test]
    fn leaving_the_last_page_wraps_to_the_start_of_the_cycle() {
        // The Network page is the last entry in `CYCLE`, so leaving it wraps — and the
        // wrap target is the start, not merely "a page the board can back". That
        // distinction was the defect: leaving for any page other than the start meant the
        // pages after it skipped the start too, and `Ambient` was never shown again.
        assert_eq!(
            DisplayPage::Network.next_page(true, false, false, false, true),
            DisplayPage::Ambient,
            "a board that lost the link mid-cycle wraps rather than jumping ahead"
        );
        assert_eq!(
            DisplayPage::Network.next_page(false, false, false, false, true),
            DisplayPage::Ambient,
            "and a link is the only capability, so back to Ambient"
        );
        // The same transition one page earlier, for the board that has both: leaving the
        // camera lands on the Network page rather than skipping it, which is what makes
        // the wrap the start's answer rather than "wherever is next".
        assert_eq!(
            DisplayPage::Camera.next_page(false, false, false, true, true),
            DisplayPage::Network
        );
        assert_eq!(
            DisplayPage::Camera.next_page(true, false, false, false, false),
            DisplayPage::Ambient,
            "a board that lost the camera mid-cycle wraps rather than jumping ahead"
        );
        assert_eq!(
            DisplayPage::Camera.next_page(false, true, false, false, false),
            DisplayPage::Ambient
        );
        assert_eq!(
            DisplayPage::Camera.next_page(false, false, false, false, false),
            DisplayPage::Ambient,
            "and a camera is the only capability, so back to Ambient"
        );
    }

    #[test]
    fn every_page_the_board_can_back_is_shown_on_the_way_round() {
        // What the cycle exists for: one full turn visits each page the board can back
        // exactly once and comes back to where it started. The defect this replaces did
        // not fail on any single transition — each one went somewhere legal — so it is
        // the whole walk that has to be asserted, from every start, not one hop.
        //
        // Every capability combination, because a page is skipped exactly when a
        // neighbouring one in the order is absent. `network` gets its own rows rather
        // than only riding along with the full set: it is last in `CYCLE`, so a board
        // with a link and nothing else is the case where leaving the camera page and
        // wrapping are different transitions.
        for (motion, audio, playback, camera, network) in [
            (false, false, false, false, false),
            (true, false, false, false, false),
            (false, true, false, false, false),
            (false, false, true, false, false),
            (false, false, false, true, false),
            (false, false, false, false, true),
            (true, true, true, true, true),
            (false, true, true, true, true),
            (true, false, false, true, true),
            (true, true, true, true, false),
        ] {
            let backed: Vec<DisplayPage> = CYCLE
                .iter()
                .copied()
                .filter(|page| page.is_backed(motion, audio, playback, camera, network))
                .collect();
            assert!(
                !backed.is_empty(),
                "Ambient needs no capability, so there is always somewhere to go"
            );
            for start in CYCLE {
                let start_is_backed = backed.contains(&start);
                let mut seen = Vec::new();
                let mut page = start;
                for _ in 0..backed.len() {
                    page = page.next_page(motion, audio, playback, camera, network);
                    seen.push(page);
                }
                // Compared as a set: one full turn from a later page visits the same pages
                // in a rotated order, and the rotation is the point — what must not vary
                // is that the turn is a permutation of the backed pages.
                let mut expected = backed.clone();
                let mut walked = seen.clone();
                expected.sort_by_key(|page| page.cycle_index());
                walked.sort_by_key(|page| page.cycle_index());
                assert_eq!(
                    walked, expected,
                    "a full turn from {start:?} did not visit each backed page once \
                         (motion={motion} audio={audio} playback={playback} camera={camera} \
                         network={network})"
                );
                if start_is_backed {
                    assert_eq!(
                        page, start,
                        "and a full turn from a page the board backs comes back to it, so \
                             the toggle is a cycle rather than a drift"
                    );
                } else {
                    // Self-healing: sitting on a page the wiring lost does not trap the
                    // toggle there, and the walk above shows every page is still reached.
                    assert!(
                        backed.contains(&seen[0]),
                        "one tap off {start:?} enters the cycle rather than staying put"
                    );
                }
            }
            if backed.len() == 1 {
                // The only page this board can show is the only one it can move to, so
                // the toggle has somewhere to go rather than stranding the user.
                assert_eq!(
                    backed[0].next_page(motion, audio, playback, camera, network),
                    backed[0],
                    "a board that backs one page has nowhere to go and stays"
                );
            }
        }
    }

    #[test]
    fn the_boot_page_joins_the_cycle_it_was_asked_for() {
        // A build that names a page starts on it and then cycles by the declared order.
        let mut manager = landed(
            DeviceManager::with_motion(true, ALL_CAPS)
                .with_audio(true)
                .with_playback(true)
                .with_camera(true)
                .with_boot_page(DisplayPage::Camera),
        );
        assert_eq!(manager.state().page, DisplayPage::Camera);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Ambient);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Attitude);
    }

    #[test]
    fn the_boot_page_falls_back_for_every_capability_not_wired() {
        // The clamp used to name Camera, Audio and Speaker by hand and miss `Attitude`,
        // so a build asking for a page the board cannot show landed on it anyway.
        let bare = landed(DeviceManager::new());
        let default_page = bare.state().page;
        for page in CYCLE {
            let boot_page = landed(DeviceManager::new().with_boot_page(page))
                .state()
                .page;
            assert_eq!(
                boot_page, default_page,
                "{page:?} needs no capability, so a board with nothing wired still lands \
                     on the same page either way"
            );
        }
        let with_motion = landed(DeviceManager::with_motion(true, ALL_CAPS));
        assert_eq!(
            landed(
                DeviceManager::with_motion(true, ALL_CAPS).with_boot_page(DisplayPage::Attitude)
            )
            .state()
            .page,
            DisplayPage::Attitude,
            "the one capability it does have"
        );
        assert_eq!(
            with_motion.with_boot_page(DisplayPage::Camera).state().page,
            DisplayPage::Ambient,
            "and the one it does not"
        );
    }

    #[test]
    fn the_camera_page_cycles_from_the_board_and_back() {
        assert!(
            !DeviceManager::new().state().camera_enabled,
            "not declared until a board declares it"
        );
        let mut manager = landed(DeviceManager::new().with_camera(true));
        assert!(manager.state().camera_enabled);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Camera);
        apply(&mut manager, triple_click());
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn a_board_without_a_camera_never_lands_on_its_page() {
        // Every other capability present, so the cycle would be full without it.
        let mut manager = landed(DeviceManager::new().with_audio(true).with_playback(true));
        for _ in 0..8 {
            apply(&mut manager, triple_click());
            assert_ne!(
                manager.state().page,
                DisplayPage::Camera,
                "the camera page is gated on the wiring, not on how long you press"
            );
        }
    }

    #[test]
    fn a_capture_poll_is_kept_only_when_audio_is_enabled() {
        let mut disabled = DeviceManager::new();
        disabled.apply_capture(loud(8_000));
        assert_eq!(disabled.state().audio.live.envelope.committed(), 0);

        let mut enabled = DeviceManager::new().with_audio(true);
        enabled.apply_capture(loud(8_000));
        assert_eq!(enabled.state().audio.live.envelope.committed(), 1);
        assert_eq!(
            enabled.state().audio.phase,
            AudioPhase::Idle,
            "not captured yet"
        );
    }

    #[test]
    fn a_scan_list_is_kept_only_when_a_link_is_wired() {
        let list = ApList::from_ranked(&[AccessPoint {
            ssid: Ssid::from_octets(b"neighbour"),
            bssid: [1, 2, 3, 4, 5, 6],
            channel: 11,
            rssi: -55,
            secured: true,
        }]);

        let mut disabled = DeviceManager::new();
        disabled.apply_scan(list);
        assert_eq!(
            disabled.state().aps,
            None,
            "a build with no link has no page to draw a list on"
        );

        let mut enabled = DeviceManager::new().with_network(true);
        enabled.apply_scan(list);
        assert_eq!(enabled.state().aps, Some(list));
    }

    /// The two states a page words differently: never looked (a build with its own
    /// credentials) against looked and found nothing.
    #[test]
    fn an_empty_scan_is_a_reading_not_an_absence() {
        let mut manager = DeviceManager::new().with_network(true);
        assert_eq!(manager.state().aps, None, "nothing has scanned yet");

        manager.apply_scan(ApList::empty());
        assert_eq!(
            manager.state().aps,
            Some(ApList::empty()),
            "a scan that found nothing is kept as an answer"
        );
    }

    #[test]
    fn a_tap_starts_and_stops_a_capture() {
        let mut manager = on_audio_page();
        tap_in_place(&mut manager);
        assert_eq!(manager.state().audio.phase, AudioPhase::Recording);

        manager.apply_capture(loud(8_000));
        tap_in_place(&mut manager);
        assert_eq!(manager.state().audio.phase, AudioPhase::Stopped);
        assert_eq!(
            manager.state().audio.captured.envelope.committed(),
            1,
            "the poll on record is latched for the panel"
        );
    }

    #[test]
    fn a_stopped_capture_keeps_what_it_caught() {
        let mut manager = on_audio_page();
        tap_in_place(&mut manager);
        manager.apply_capture(loud(8_000));
        tap_in_place(&mut manager);
        let stopped = manager.state().audio.captured;

        let polled_on = loud(20_000);
        manager.apply_capture(polled_on);
        assert_eq!(
            manager.state().audio.captured,
            stopped,
            "the driver keeps polling but a stopped capture is frozen"
        );
        assert_eq!(
            manager.state().audio.live,
            polled_on,
            "live is the driver's latest poll, so resuming picks the scope up"
        );
    }

    #[test]
    fn a_tap_records_without_leaving_the_audio_page() {
        let mut manager = on_audio_page();
        apply(&mut manager, tap());
        assert_eq!(manager.state().page, DisplayPage::Audio, "the page holds");
        assert_eq!(manager.state().audio.phase, AudioPhase::Recording);
    }

    #[test]
    fn a_tap_off_the_audio_page_is_a_color_move_and_never_records() {
        let mut manager = on_audio_page();
        let before = manager.state().lights[0];

        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(
            manager.state().page,
            DisplayPage::Ambient,
            "walked off audio"
        );

        apply(&mut manager, tap());
        assert_eq!(
            manager.state().audio.phase,
            AudioPhase::Idle,
            "never records"
        );
        assert_ne!(
            manager.state().lights[0],
            before,
            "the tap moved the color instead"
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
            brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
        };
        manager.apply_business(set_light(0, solid));
        assert_eq!(manager.light_state(), solid);
        assert_eq!(
            manager.state().lights[0],
            solid,
            "the snapshot mirrors the applied target"
        );
        // Out-of-range instances clamp to the last installed surface.
        manager.apply_business(set_light(7, LightState::Off));
        assert_eq!(manager.light_state_at(1), LightState::Off);
        // An invalid intent is a no-op.
        manager.apply_business(BusinessIntent::Invalid);
        assert_eq!(manager.light_state(), solid, "an invalid intent is a no-op");
    }

    #[test]
    fn a_link_report_lands_whole_and_an_unwired_board_reads_off() {
        // A board that never wired a radio reports `Off` without asking anything, which
        // is what lets a consumer read the link rather than the board's wiring.
        let mut manager = DeviceManager::new();
        assert_eq!(manager.state().link.phase, LinkPhase::Off);

        // The whole report, access point and address included: the panel draws both,
        // and re-deriving either here would be a second opinion about what the radio
        // said.
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
        manager.apply_business(BusinessIntent::LinkReport(status));
        assert_eq!(manager.state().link, status);
    }
}

mod playback {
    use super::*;
    use crate::drivers::playback::Sound;

    /// Every capability is on so a case can also assert the cycle, and the
    /// taps are looped rather than counted so the helper does not have to be
    /// rewritten when a page is added.
    fn on_speaker() -> DeviceManager {
        let mut manager = landed(
            DeviceManager::with_motion(true, MotionCapabilities::EMPTY)
                .with_audio(true)
                .with_playback(true),
        );
        for _ in 0..=3 {
            if manager.state().page == DisplayPage::Speaker {
                break;
            }
            apply_page(&mut manager, triple_click());
        }
        assert_eq!(manager.state().page, DisplayPage::Speaker);
        manager
    }

    /// The Speaker page's play verb, from the panel or the button.
    fn play(manager: &mut DeviceManager) {
        let before = manager.playback_state();
        apply_page(manager, tap());
        if before.phase == PlaybackPhase::Playing {
            assert_eq!(
                manager.playback_state().sound,
                before.sound,
                "a dropped tap must not walk the catalogue"
            );
        } else {
            assert_ne!(
                manager.playback_state().sound,
                before.sound,
                "a heard tap advances to the next sound"
            );
        }
    }

    fn mute(manager: &mut DeviceManager) {
        let before = manager.playback_state().muted;
        apply_page(manager, long_press());
        assert_ne!(
            manager.playback_state().muted,
            before,
            "the latch flips whichever way it stood"
        );
    }

    #[test]
    fn the_speaker_page_only_appears_on_a_board_that_has_one() {
        assert_eq!(
            DisplayPage::Ambient.next_page(true, false, true, false, false),
            DisplayPage::Attitude
        );
        assert_eq!(
            DisplayPage::Audio.next_page(true, true, true, false, false),
            DisplayPage::Speaker
        );
        assert_eq!(
            DisplayPage::Audio.next_page(true, true, false, false, false),
            DisplayPage::Ambient,
            "a board without a speaker skips the page"
        );
        assert_eq!(
            DisplayPage::Ambient.next_page(false, false, true, false, false),
            DisplayPage::Speaker
        );
        // Sitting on the Speaker page with the speaker gone falls back to
        // Ambient rather than stranding the cycle.
        assert_eq!(
            DisplayPage::Speaker.next_page(true, true, false, false, false),
            DisplayPage::Ambient
        );
    }

    #[test]
    fn a_full_lap_walks_every_page_and_returns_home() {
        let mut manager = on_speaker();
        let pages = [
            DisplayPage::Ambient,
            DisplayPage::Attitude,
            DisplayPage::Audio,
        ];
        for expected in pages {
            manager.apply_business(BusinessIntent::NextPage);
            assert_eq!(manager.state().page, expected);
        }
        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(manager.state().page, DisplayPage::Speaker, "and back round");
    }

    #[test]
    fn a_tap_plays_the_next_sound_and_a_replay_is_counted_not_heard() {
        let mut manager = on_speaker();
        let boot = manager.playback_state();
        assert_eq!(boot.phase, PlaybackPhase::Idle);
        assert_eq!(boot.sound, Sound::Tap);
        assert!(!boot.muted, "a boot that is silent reads as dead");

        // Walked off the catalogue rather than written out, so adding a sound
        // cannot leave this asserting a walk that no longer happens.
        let second = Sound::Tap.next();
        play(&mut manager);
        let played = manager.playback_state();
        assert_eq!(played.phase, PlaybackPhase::Playing);
        assert_eq!(played.sound, second, "the tap advanced the catalogue");
        assert_eq!(played.plays, 1);
        assert_eq!(played.dropped, 0);

        // A second tap over a sounding one is tallied, not heard: the sound
        // does not restart, and the count is what tells the user the tap
        // was heard.
        play(&mut manager);
        let dropped = manager.playback_state();
        assert_eq!(
            dropped.phase,
            PlaybackPhase::Playing,
            "still the same sound"
        );
        assert_eq!(dropped.sound, second, "and not the next one");
        assert_eq!(dropped.plays, 1, "so no second sound started");
        assert_eq!(dropped.dropped, 1);

        // The driver's own report hands the page back, and only then is the
        // next tap heard.
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Idle);
        play(&mut manager);
        let second_play = manager.playback_state();
        assert_eq!(
            second_play.sound,
            second.next(),
            "the catalogue kept walking",
        );
        assert_eq!(second_play.plays, 2);
        assert_eq!(
            second_play.dropped, 1,
            "the dropped tally is kept across plays"
        );
    }

    #[test]
    fn a_finish_arriving_with_nothing_playing_is_ignored() {
        // The driver owns the only way into Playing, so a report that
        // arrives late — or twice — must not knock the page out of step with
        // it and let the next tap be dropped.
        let mut manager = on_speaker();
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Idle);
        play(&mut manager);
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        assert_eq!(
            manager.playback_state().phase,
            PlaybackPhase::Idle,
            "a second finish has nothing left to finish"
        );
    }

    #[test]
    fn the_mute_latch_stands_until_it_is_flipped_back() {
        let mut manager = on_speaker();
        mute(&mut manager);
        assert!(manager.playback_state().muted);
        // Muting does not stop a sound that is already sounding, and it does
        // not gate the next one: the latch is the output's, not the page's.
        play(&mut manager);
        let muted = manager.playback_state();
        assert!(muted.muted, "still latched while playing");
        assert_eq!(muted.phase, PlaybackPhase::Playing, "a play still starts");
        mute(&mut manager);
        assert!(
            !manager.playback_state().muted,
            "and one press brings it back"
        );
    }

    #[test]
    fn muting_twice_does_not_cancel_itself_out() {
        // Two presses are two flips, not a double-count: the point of a latch
        // is that a second press is a deliberate unmute.
        let mut manager = on_speaker();
        mute(&mut manager);
        mute(&mut manager);
        assert!(!manager.playback_state().muted);
    }

    #[test]
    fn a_board_without_a_speaker_plays_nothing() {
        let mut manager = landed(DeviceManager::new());
        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(manager.state().page, DisplayPage::Ambient, "no page either");
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Next));
        assert_eq!(manager.playback_state(), PlaybackState::boot());
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::SetMuted(true)));
        assert!(
            !manager.playback_state().muted,
            "an unwired board's latch is nobody's business"
        );
    }

    #[test]
    fn the_tallies_saturate_rather_than_wrap() {
        // A counter that wrapped would report a long session as having
        // played nothing, and the panel would read `0` while sounds came out.
        let mut manager = on_speaker();
        manager.state.playback.plays = u16::MAX;
        manager.state.playback.dropped = u16::MAX;
        play(&mut manager);
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        let saturated = manager.playback_state();
        assert_eq!(saturated.plays, u16::MAX);
        assert_eq!(saturated.dropped, u16::MAX);
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
            contacts: 1,
        };
        manager.apply_operation(op(InputEvent::Touch(first)));
        assert_eq!(manager.touch_state(), Some(first));
        assert_eq!(
            manager.state().press_count,
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
            contacts: 1,
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
        assert_eq!(manager.state().touch_frames, 0);
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 10,
            y: 20,
        })));
        assert_eq!(
            manager.state().touch_frames,
            0,
            "a classifier press pulse is not a raw frame"
        );
        manager.apply_operation(touch_event(TouchStatus::Down));
        manager.apply_operation(touch_event(TouchStatus::Contact));
        assert_eq!(
            manager.state().touch_frames,
            2,
            "every stored snapshot ticks the heartbeat once"
        );
        manager.apply_operation(tap());
        assert_eq!(
            manager.state().touch_frames,
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
        assert_eq!(state.tap_count, 3);
        assert_eq!(
            state.last_gesture_origin,
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
        assert_eq!(manager.state().long_press_count, 1);
        assert_eq!(
            manager.state().press_count,
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
            manager.state().tap_count,
            0,
            "255 taps wrap to 0, keeping the diagnostic digit bounded"
        );
    }

    #[test]
    fn press_pulse_tallies_upserts_and_wraps() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(press(0, 12, 34));
        manager.apply_operation(press(0, 12, 34));
        assert_eq!(manager.state().press_count, 2);
        assert_eq!(
            manager.state().touch_points,
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
            wraps.state().press_count,
            0,
            "255 presses wrap to 0, keeping the diagnostic digit bounded"
        );
    }

    #[test]
    fn ghost_tallies_and_saturates() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        assert_eq!(manager.state().ghost_count, 2);
        for _ in 0..(u8::MAX as u32).saturating_add(1) {
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
        }
        assert_eq!(
            manager.state().ghost_count,
            u8::MAX,
            "the ghost tally saturates instead of wrapping, preserving the record"
        );
    }

    #[test]
    fn swipe_tallies_records_travel_and_wraps() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(swipe());
        let state = manager.state();
        assert_eq!(state.swipe_count, 1);
        assert_eq!(state.last_swipe, Some((SwipeDirection::Down, 73)));
        assert_eq!(state.last_gesture_origin, Some((80, 30)));
        assert_eq!(state.last_gesture_end, Some((80, 60)));
        assert_eq!(state.touch_held_ms, 42);
        assert_eq!(
            state.lights[0],
            LightState::default(),
            "the swipe op tallies; the light move is the translated business intent"
        );
        assert_eq!(
            state.touch_points,
            [None, None],
            "resolution frees the live slot"
        );
        assert_eq!(state.live_dir[0], 0, "resolution clears the movement arrow");
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
            wraps.state().swipe_count,
            0,
            "255 swipes wrap to 0, keeping the diagnostic digit bounded"
        );
    }

    #[test]
    fn chip_gesture_stores_verbatim_without_touching_light() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(op(InputEvent::ChipGesture(0x10)));
        assert_eq!(manager.state().chip_gesture_id, 0x10);
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
        assert_eq!(manager.state().tap_count, 1);
        assert_eq!(manager.state().ghost_count, 1);
        assert_eq!(
            manager.state().press_count,
            1,
            "only a classifier press pulse earns a press tally, never a raw frame"
        );
        let target = LightState::Solid {
            color: Rgb(1, 2, 3),
            brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
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
            manager.state().tap_count,
            1,
            "the business move never inflates the operation tally"
        );
    }

    #[test]
    fn off_tap_full_path_records_diagnostics_and_keeps_off() {
        // A tap on `Off` has no color step, so it resolves `Invalid` — the
        // light must not move while the physical input still records.
        let mut manager = DeviceManager::new();
        manager.apply_business(set_light(0, LightState::Off));
        let op = tap();
        manager.apply_operation(op);
        let business = translate(&op, &manager.state());
        manager.apply_business(business);
        assert_eq!(business, BusinessIntent::Invalid);
        assert_eq!(manager.light_state(), LightState::Off);
        assert_eq!(manager.state().tap_count, 1);
        assert_eq!(manager.state().last_gesture_origin, Some((1, 2)));
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
        assert_eq!(manager.state().swipe_count, 1);
        assert_eq!(manager.state().last_swipe, Some((SwipeDirection::Down, 73)));
    }
}

mod trackers {
    use super::*;

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
            contacts: 1,
        })));
        manager.apply_operation(op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint {
                id: 1,
                x: 150,
                y: 40,
                status: TouchStatus::Contact,
            }; MAX_TOUCH_POINTS],
            len: 1,
            contacts: 1,
        })));
        assert_eq!(
            manager.state().touch_points,
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
            manager.state().two_finger_runs,
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
            contacts: 1,
        })));
        assert_eq!(
            manager.state().touch_points,
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
            contacts: 1,
        })));
        assert_eq!(manager.state().touch_points, [None, None]);
        assert_eq!(
            manager.state().two_finger_runs,
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
            manager.state().live_dir,
            [0, 0],
            "a fresh landing reads as still until the finger moves"
        );
        manager.apply_operation(touch_at(0, 100, 150, TouchStatus::Contact));
        assert_eq!(
            manager.state().live_dir[0],
            SwipeDirection::Up.code(),
            "climbing the panel is an up-move in framebuffer space"
        );
        manager.apply_operation(touch_at(0, 60, 150, TouchStatus::Contact));
        assert_eq!(manager.state().live_dir[0], SwipeDirection::Left.code());
        manager.apply_operation(touch_at(0, 61, 151, TouchStatus::Contact));
        assert_eq!(
            manager.state().live_dir[0],
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
            manager.state().live_dir,
            [0, SwipeDirection::Left.code()],
            "each slot carries only its own finger's axis"
        );
        manager.apply_operation(touch_at(1, 0, 0, TouchStatus::Release));
        assert_eq!(
            manager.state().live_dir,
            [0, 0],
            "releasing a finger frees its slot back to still"
        );
    }

    #[test]
    fn simultaneous_presses_upsert_slots_and_count_runs() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(press(0, 10, 20));
        assert_eq!(
            manager.state().touch_points,
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
            manager.state().two_finger_runs,
            1,
            "two live slots count one two-finger run"
        );
        manager.apply_operation(press(0, 11, 21));
        assert_eq!(
            manager.state().two_finger_runs,
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
                contacts: 2,
            })));
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: up_snapshot,
                len: 2,
                contacts: 0,
            })));
        }
        assert_eq!(
            manager.state().two_finger_runs,
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
            manager.state().touch_points,
            [None, None],
            "each resolved gesture frees only its own slot"
        );
        assert_eq!(
            manager.state().finger,
            [
                FingerLast {
                    kind: FINGER_SWIPE,
                    dir: SwipeDirection::Up.code(),
                    value: 77,
                },
                FingerLast {
                    kind: FINGER_TAP,
                    dir: 0,
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
            manager.state().live_dir[0],
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
        assert_eq!(state.swipe_count, 1, "the diagonal still tallies");
        assert_eq!(state.last_swipe, Some((SwipeDirection::DownRight, 28)));
        assert_eq!(
            state.touch_points[0], None,
            "resolution frees the live slot"
        );
        assert_eq!(state.live_dir[0], 0, "resolution clears the movement arrow");
    }

    #[test]
    fn last_gesture_points_follow_resolved_gestures() {
        let mut manager = DeviceManager::new();
        manager.apply_operation(swipe());
        assert_eq!(
            manager.state().last_gesture_origin,
            Some((80, 30)),
            "a swipe records its origin"
        );
        assert_eq!(manager.state().last_gesture_end, Some((80, 60)));

        manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 11,
            y: 12,
            held_ms: 0,
            at_ms: 0,
        })));
        assert_eq!(
            manager.state().last_gesture_origin,
            Some((11, 12)),
            "a tap stamps its own point as the origin"
        );
        assert_eq!(
            manager.state().last_gesture_end,
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
        assert_eq!(manager.state().last_gesture_origin, Some((13, 14)));
        assert_eq!(
            manager.state().last_gesture_end,
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
            manager.state().last_gesture_origin,
            Some((21, 22)),
            "a slide shows where it started"
        );
        assert_eq!(
            manager.state().last_gesture_end,
            Some((23, 24)),
            "a slide shows where it ended"
        );
    }
}

fn touch_at(id: u8, x: u16, y: u16, status: TouchStatus) -> OperationIntent {
    op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint { id, x, y, status }; MAX_TOUCH_POINTS],
        len: 1,
        contacts: 1,
    }))
}

mod network_page {
    use super::*;
    use crate::keyboard::{self, Key, Layer};

    /// A network whose BSSID follows its name, because that is the field a rescan does not
    /// change: two rows of one list are two networks here as they are on a panel, and a test
    /// that gave them one BSSID would be testing a list of one network twice.
    fn ap_named(ssid: &[u8], rssi: i8, secured: bool) -> AccessPoint {
        let first = ssid.first().copied().unwrap_or(0);
        AccessPoint {
            ssid: Ssid::from_octets(ssid),
            bssid: [first, 2, 3, 4, 5, ssid.len() as u8],
            channel: 6,
            rssi,
            secured,
        }
    }

    fn ap(ssid: &[u8]) -> AccessPoint {
        ap_named(ssid, -55, true)
    }

    /// A network with no passphrase, which the panel connects to on the tap that picks it.
    fn open(ssid: &[u8]) -> AccessPoint {
        ap_named(ssid, -55, false)
    }

    /// The store's own answer for a device that has saved these names, as the network task
    /// publishes it: the list the page is showing, marked against a store that holds them.
    ///
    /// A mark per row rather than the names themselves, which is why this takes the list the
    /// page already has: what the store answers is a question about *those* rows.
    fn marked(manager: &mut DeviceManager, names: &[&str]) {
        let mut networks = SavedNetworks::empty();
        for name in names {
            let credentials =
                Credentials::from_text(name, "latte").expect("a credential the radio takes");
            networks.remember(&credentials);
        }
        let Some(mut list) = manager.state().aps else {
            return;
        };
        list.mark_saved(networks.mask(&list));
        manager.apply_scan(list);
    }

    /// A build that wired a radio, sitting on the page with two networks in range — the
    /// state the product spends its first minutes in.
    fn on_network_page() -> DeviceManager {
        let mut manager = landed(DeviceManager::new().with_network(true));
        manager.apply_scan(ApList::from_ranked(&[ap(b"neighbour"), ap(b"cafe")]));
        // A board with no other capability cycles straight to the link's page.
        manager.apply_business(BusinessIntent::NextPage);
        assert_eq!(manager.state().page, DisplayPage::Network);
        manager
    }

    /// One tap, translated against the state the way the render loop does it.
    fn tap_on_at(manager: &mut DeviceManager, x: u16, y: u16, at_ms: u32) {
        let operation = op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x,
            y,
            held_ms: 40,
            at_ms,
        }));
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    fn tap_on(manager: &mut DeviceManager, x: u16, y: u16) {
        tap_on_at(manager, x, y, 40);
    }

    /// Tap the middle of the band the list's `band`-th visible row is drawn in.
    fn tap_band(manager: &mut DeviceManager, band: usize) {
        let y =
            (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16;
        tap_on(manager, 30, y);
    }

    fn key_at(manager: &mut DeviceManager, key: Key, at_ms: u32) {
        manager.apply_business(BusinessIntent::Key { key, at_ms });
    }

    fn key(manager: &mut DeviceManager, key: Key) {
        key_at(manager, key, 40);
    }

    /// What the page handed the link, as the credential the radio would join with: every test
    /// here is about a passphrase, and the typed shape is the one that carries one. A name
    /// arriving where a passphrase was expected is a page that asked the store instead, which is
    /// a different test's subject and never this one's intended answer.
    fn handed_over(manager: &mut DeviceManager) -> Option<Credentials> {
        match manager.take_join() {
            Some(JoinRequest::Typed(credentials)) => Some(credentials),
            Some(JoinRequest::Held(_)) => {
                panic!("the page handed over a name where a passphrase was expected")
            }
            None => None,
        }
    }

    /// Type one letter of the phone layer the way a finger would: the key that carries it,
    /// tapped until the letter comes round, each tap inside the multi-tap window.
    ///
    /// `at_ms` is where the first tap of it lands. A caller spelling a word has to move the
    /// clock forward between letters, because two letters off one key in a row are the same
    /// character being cycled — which is what a phone keypad does and what a tester has to wait
    /// out.
    fn type_letter(manager: &mut DeviceManager, wanted: u8, at_ms: u32) {
        for digit in 0..keyboard::CHARS {
            let candidates = Layer::Abc.candidates(digit);
            let Some(taps) = candidates.iter().position(|candidate| *candidate == wanted) else {
                continue;
            };
            let face = Layer::Abc.face(digit);
            for tap in 0..=taps {
                key_at(manager, Key::Char(face), at_ms + 100 * tap as u32);
            }
            return;
        }
        panic!("{wanted:?} is not a key of the phone layer");
    }

    #[test]
    fn the_address_the_radio_reports_is_the_one_the_page_draws() {
        // One report at boot and nothing after it: the panel's header and the setup page's own
        // line are the same number, and neither asks the radio again.
        let mut manager = on_network_page();
        assert_eq!(manager.state().mac, None, "nothing has reported one yet");
        let mac = [0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef];
        manager.apply_business(BusinessIntent::Mac(mac));
        assert_eq!(manager.state().mac, Some(mac));
    }

    #[test]
    fn a_script_types_its_own_text_through_the_keyboard() {
        // The whole typing chain without a finger: each of the script's cells reads back as a
        // key through the same `key_of` the panel draws with, and the entry those keys fill is
        // the text the radio would be handed. This is what a bench that plays a script at the
        // panel's coordinates is standing on.
        for text in [b"pass".as_slice(), b"aZ9.# 0_", b"00", b"z9", b"AaA", b" "] {
            let mut entry = Entry::default();
            for tap in keyboard::script_for(text).expect("a text the keyboard carries") {
                let key = keyboard::key_of(entry.layer(), tap.cell).expect("a cell of the grid");
                entry.edit(key, tap.at_ms);
            }
            assert_eq!(entry.as_bytes(), text, "{text:?} did not come back");
        }
    }

    #[test]
    fn a_tap_on_a_band_opens_the_keyboard_on_the_network_shown_in_it() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"cafe")),
            "the keyboard goes with the network the band was showing"
        );
        assert_eq!(
            manager.state().setup.entry,
            Some(Entry::default()),
            "and starts empty"
        );
        assert_eq!(
            manager.state().setup.entry.map(|entry| entry.layer()),
            Some(Layer::Abc),
            "on the layer a network's name is spelled with"
        );
    }

    #[test]
    fn a_band_the_list_has_nothing_in_is_not_a_selection() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 3);
        assert_eq!(
            manager.state().setup.entry,
            None,
            "a band past the last network is the page's own background"
        );
    }

    #[test]
    fn the_list_slides_a_row_at_a_time_and_a_band_means_the_row_it_shows() {
        // Eight rows through a window of three: the band is the page's row rather than the
        // scan's, so the same finger position is a different network as the list slides.
        let mut manager = landed(DeviceManager::new().with_network(true));
        let aps: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
        manager.apply_scan(ApList::from_ranked(&aps));
        manager.apply_business(BusinessIntent::NextPage);

        tap_band(&mut manager, 0);
        assert_eq!(manager.state().setup.selected, Some(ap(b"a")));
        manager.apply_business(BusinessIntent::Key {
            key: Key::Cancel,
            at_ms: 1,
        });

        assert_eq!(manager.state().setup.list_top, 0);
        manager.apply_business(BusinessIntent::Scroll { rows: 1 });
        assert_eq!(
            manager.state().setup.list_top,
            1,
            "a slide forward moves the window one row"
        );
        tap_band(&mut manager, 0);
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"b")),
            "and the first band now holds the second network"
        );
        // Forward to the end: the last window of rows, so the end of the list shows the rows it
        // has rather than a window of empty bands.
        for _ in 0..(8 - keyboard::LIST_ROWS) {
            manager.apply_business(BusinessIntent::Scroll { rows: 1 });
        }
        assert_eq!(
            manager.state().setup.list_top,
            (8 - keyboard::LIST_ROWS) as u8,
            "eight rows through a three-row window stop five rows down"
        );
        manager.apply_business(BusinessIntent::Scroll { rows: 1 });
        assert_eq!(
            manager.state().setup.list_top,
            (8 - keyboard::LIST_ROWS) as u8,
            "and the end of the list is as far as it goes"
        );
        manager.apply_business(BusinessIntent::Scroll { rows: -1 });
        assert_eq!(
            manager.state().setup.list_top,
            (8 - keyboard::LIST_ROWS - 1) as u8
        );
        for _ in 0..8 {
            manager.apply_business(BusinessIntent::Scroll { rows: -1 });
        }
        assert_eq!(
            manager.state().setup.list_top,
            0,
            "and the top of it is as far back"
        );
    }

    #[test]
    fn a_pick_is_read_only_while_there_is_a_link_to_take_down() {
        // The page's own rule: a row is a pick only while nothing is on the air. A list that
        // still took one under a link would be naming the next network while standing on the
        // current one, and the strip is the way off.
        for phase in [LinkPhase::Connecting, LinkPhase::Connected] {
            let mut manager = on_network_page();
            link_at(&mut manager, phase, b"neighbour");
            tap_band(&mut manager, 1);
            assert_eq!(
                manager.state().setup.selected,
                None,
                "{phase:?} is a link, so the band is not a pick"
            );
            assert!(!manager.state().setup.keyboard, "{phase:?}: no keyboard");
            assert_eq!(manager.state().setup.entry, None, "{phase:?}: no draft");
            assert_eq!(
                handed_over(&mut manager),
                None,
                "{phase:?}: and nothing reaches the radio"
            );
        }
        // The same band under a phase that is not a link is the pick it always was.
        for phase in [
            LinkPhase::Off,
            LinkPhase::Scanning,
            LinkPhase::Failed(NetworkError::Credentials),
        ] {
            let mut manager = on_network_page();
            link_at(&mut manager, phase, b"neighbour");
            tap_band(&mut manager, 1);
            assert_eq!(
                manager.state().setup.selected,
                Some(ap(b"cafe")),
                "{phase:?} is not a link, so the band picks the row it shows"
            );
            assert!(
                manager.state().setup.keyboard,
                "{phase:?}: and opens the keyboard"
            );
        }
    }

    #[test]
    fn a_pick_of_an_open_row_is_read_only_under_a_link_too() {
        // An open row has no keyboard to open: the tap is the whole of the pick, and the
        // credential it builds is the one that reaches the radio with no screen in between.
        // So it is the pick the gate has to cover as well as the one that types.
        let mut manager = DeviceManager::new().with_network(true);
        manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
        manager.apply_business(BusinessIntent::NextPage);
        link_at(&mut manager, LinkPhase::Connected, b"neighbour");

        tap_band(&mut manager, 0);
        assert_eq!(
            manager.state().setup.selected,
            None,
            "the row is not a pick"
        );
        assert_eq!(
            handed_over(&mut manager),
            None,
            "and nothing reaches the radio"
        );
    }

    #[test]
    fn a_hold_on_the_list_under_the_join_box_asks_nothing() {
        // The box is over the list, so the rows under it are the box's paper: a forget asked of a
        // row nobody can see would be a memory erased off a screen the finger was not looking at.
        // The strip is the one part that stays offered under the box, which is what this costs
        // the list.
        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        link_at(&mut manager, LinkPhase::Connecting, b"cafe");

        hold_band(&mut manager, 1);
        assert_eq!(
            manager.state().setup.confirm,
            None,
            "a hold under the join box asks nothing"
        );
        assert_eq!(manager.take_forget(), None, "and nothing reaches the store");
    }

    #[test]
    fn a_slide_under_the_join_box_does_not_move_the_list() {
        // The list's other gesture. A window that moved under a box nobody can see through would
        // be a page that jumped the moment the box came off it.
        let mut manager = eight_rows();
        link_at(&mut manager, LinkPhase::Connecting, b"a");
        slide_list(&mut manager);
        assert_eq!(
            manager.state().setup.list_top,
            0,
            "the window stayed where the box was drawn over it"
        );

        // The same gesture with nothing over the list is the slide it always was.
        let mut manager = eight_rows();
        slide_list(&mut manager);
        assert_eq!(manager.state().setup.list_top, 1);
    }

    /// A build sitting on the page with eight networks in range, which is a list with more rows
    /// than the window holds.
    fn eight_rows() -> DeviceManager {
        let mut manager = landed(DeviceManager::new().with_network(true));
        let aps: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
        manager.apply_scan(ApList::from_ranked(&aps));
        manager.apply_business(BusinessIntent::NextPage);
        manager
    }

    /// Slide the list forward one row, through the same translate the render loop runs a swipe
    /// through.
    fn slide_list(manager: &mut DeviceManager) {
        let operation = op(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Up,
            x: 30,
            y: (keyboard::LIST_TOP + keyboard::LIST_ROW_H * 2) as u16,
            end_x: 30,
            end_y: keyboard::LIST_TOP as u16,
            held_ms: 40,
            distance_px: (keyboard::LIST_ROW_H * 2) as u16,
        }));
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    #[test]
    fn a_hold_on_a_saved_row_still_asks_while_the_link_is_up() {
        // The store is not the link's: what a hold asks about is the copy on the part, so it
        // is read whatever the radio is doing — and it is the one thing the list still
        // answers under a link.
        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        link_up(&mut manager, b"neighbour");

        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().setup.confirm,
            None,
            "a tap is not a question"
        );

        hold_band(&mut manager, 1);
        assert_eq!(
            manager.state().setup.confirm,
            Some(Confirm::Forget(ap(b"cafe"))),
            "a hold still is"
        );
    }

    #[test]
    fn a_draft_picked_before_the_radio_associated_is_still_open_under_it() {
        // The gate is on the pick, not on the keyboard: a draft opened while the list was
        // free stays open when a link comes up under it, and what changed is the strip's own
        // verb. That is the window the credential plane still has a panel source in.
        let mut manager = on_network_page();
        tap_band(&mut manager, 1);
        assert!(
            manager.state().setup.keyboard,
            "the pick opened the keyboard"
        );

        link_at(&mut manager, LinkPhase::Connecting, b"neighbour");
        assert!(
            manager.state().setup.keyboard,
            "and the association does not close it"
        );
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"cafe")),
            "the draft is still the row it was opened on"
        );
    }

    /// Tap the action strip, which is one target at every column.
    fn tap_strip(manager: &mut DeviceManager) {
        tap_on(
            manager,
            160,
            (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
        );
    }

    /// The strip's other cell: the way back, drawn only while the device keeps a network.
    fn tap_strip_left(manager: &mut DeviceManager) {
        tap_on(
            manager,
            40,
            (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
        );
    }

    /// Hold the middle of the band the list's `band`-th visible row is drawn in, through the
    /// same translate the render loop runs a classified long press through.
    fn hold_band(manager: &mut DeviceManager, band: usize) {
        let y =
            (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16;
        let operation = op(InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x: 30,
            y,
            held_ms: u16::from(FINGER_LONG_PRESS) + 20,
        }));
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    /// Tap the middle of one of a confirmation's two answers.
    fn tap_answer(manager: &mut DeviceManager, choice: keyboard::DialogChoice) {
        let box_ = keyboard::dialog_answer_box(choice);
        tap_on(
            manager,
            (box_.x + box_.w / 2) as u16,
            (box_.y + box_.h / 2) as u16,
        );
    }

    fn link_up(manager: &mut DeviceManager, ssid: &[u8]) {
        link_at(manager, LinkPhase::Connected, ssid);
    }

    /// The link in `phase` with `ssid` named, as the network task publishes it.
    fn link_at(manager: &mut DeviceManager, phase: LinkPhase, ssid: &[u8]) {
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase,
            ap: Some(ap(ssid)),
            address: None,
            ..LinkStatus::default()
        }));
    }

    #[test]
    fn the_action_strip_leaves_while_a_link_is_up_and_looks_around_while_it_is_not() {
        // One control, and the link decides which verb it carries: the phases a link is up or
        // coming up in are the ones the radio will not scan in, so there is never a choice.
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");

        tap_strip(&mut manager);
        assert_eq!(
            manager.state().setup.confirm,
            Some(Confirm::Leave),
            "a link that is up is one to take down, and taking it down is asked about"
        );
        assert!(!manager.take_leave(), "asked about, not done");
        assert!(
            !manager.take_scan_now(),
            "and the strip was not asking to look around"
        );

        // The same control with no link on it is the survey's own request, and it is not asked
        // about: a scan changes nothing but what is drawn.
        let mut manager = on_network_page();
        tap_strip(&mut manager);
        assert!(
            manager.take_scan_now(),
            "with no link there is something to look around for"
        );
        assert!(
            manager.state().setup.rescan_wait,
            "and the strip waits for the round it asked for"
        );
        assert_eq!(manager.state().setup.confirm, None, "nothing to confirm");
        assert!(
            !manager.take_scan_now(),
            "a request is handed over once, like every other command"
        );
    }

    #[test]
    fn a_link_that_is_coming_up_is_already_one_to_leave() {
        // The phase is the whole of the answer, so an attempt the radio has not accepted yet
        // is on the strip's side of it as much as one that associated and is waiting for its
        // address. Asked with and without an access point, because the network task publishes
        // both: the attempt goes up the moment it has a credential in hand — before the radio
        // has named anything, since the association is where the name comes from — and the
        // access point is filled in when the radio answers. A rule that asked about the
        // association report instead of the phase would answer `SCAN` through the whole
        // association, which is the half-minute a user is most likely to change their mind in.
        for named in [None, Some(ap(b"neighbour"))] {
            for phase in [LinkPhase::Connecting, LinkPhase::Connected] {
                let mut manager = on_network_page();
                manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
                    phase,
                    ap: named,
                    address: None,
                    ..LinkStatus::default()
                }));
                tap_strip(&mut manager);
                assert_eq!(
                    manager.state().setup.confirm,
                    Some(Confirm::Leave),
                    "{phase:?} is a link to leave, named {named:?}"
                );
            }
        }
        for phase in [
            LinkPhase::Off,
            LinkPhase::Scanning,
            LinkPhase::Failed(NetworkError::Credentials),
        ] {
            let mut manager = on_network_page();
            link_at(&mut manager, phase, b"neighbour");
            tap_strip(&mut manager);
            assert!(
                manager.take_scan_now(),
                "{phase:?} is not a link, so the strip looks around instead"
            );
        }
    }

    /// The hardware button's long press, through the same translate the render loop runs it
    /// through, so the test reads the rule the product does and not a copy of it.
    fn long_press_the_button(manager: &mut DeviceManager) {
        let operation = op(InputEvent::Button(ButtonEvent::LongPress));
        manager.apply_operation(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_business(business);
    }

    #[test]
    fn a_splash_long_press_asks_for_setup_mode_on_every_board() {
        // The splash is the whole of the setup window, and the request is the same one on both
        // fronts: the board that can draw the mode draws it when the page ends, and the board
        // that cannot still has the mode to enter. Nothing about a link is involved, because a
        // device on this page has not read a credential yet.
        for panel in [Panel::Wired, Panel::Absent] {
            let mut manager = DeviceManager::new().with_network(true).with_panel(panel);
            long_press_the_button(&mut manager);
            assert!(manager.setup_wanted(), "{panel:?}: the request is latched");
            assert_eq!(
                manager.state().setup.confirm,
                None,
                "{panel:?}: the splash carries no question"
            );
            assert!(
                !manager.take_leave(),
                "{panel:?}: the request is not a leave"
            );
            assert_eq!(
                manager.state().page,
                DisplayPage::Splash,
                "{panel:?}: the page keeps its window"
            );
        }
    }

    #[test]
    fn the_splash_lands_on_the_page_that_offers_setup_when_it_was_asked_for() {
        // The one thing the request does: the landing is the mode's page rather than the one
        // the build named, so a person who held the button sees the list the phone is on.
        let mut manager = DeviceManager::new()
            .with_network(true)
            .with_camera(true)
            .with_boot_page(DisplayPage::Camera);
        long_press_the_button(&mut manager);
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(manager.state().page, DisplayPage::Network);
    }

    #[test]
    fn the_splash_lands_on_the_build_page_when_nothing_was_asked() {
        // And without a request the build's page is the landing, which is the whole reason the
        // splash is not a page in the cycle: it is a window, not a place.
        let manager = landed(
            DeviceManager::new()
                .with_network(true)
                .with_camera(true)
                .with_boot_page(DisplayPage::Camera),
        );
        assert_eq!(manager.state().page, DisplayPage::Camera);
        assert!(!manager.setup_wanted());
    }

    #[test]
    fn a_board_with_no_radio_lands_on_its_page_even_after_the_hold() {
        // There is nothing a page could set up, so the request is not a request: the hold is
        // inert on a build that wired no network.
        let mut manager = DeviceManager::new();
        long_press_the_button(&mut manager);
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn a_long_press_after_the_splash_is_the_pages_own_verb() {
        // The splash is the whole of what makes the setup request. A press made once the device
        // is up and running is the page's own verb — even on a board with no screen, whose
        // button is the only control surface it has.
        let mut manager = landed(
            DeviceManager::new()
                .with_network(true)
                .with_panel(Panel::Absent),
        );
        link_up(&mut manager, b"neighbour");
        let before = manager.state().lights[0];

        long_press_the_button(&mut manager);
        assert_eq!(
            manager.state().setup.confirm,
            None,
            "the press never raises the question the strip raises"
        );
        assert!(!manager.take_leave(), "and it takes nothing down");
        assert_ne!(
            manager.state().lights[0],
            before,
            "the press is still the page's own verb"
        );
    }

    #[test]
    fn a_strip_leave_on_a_board_with_no_screen_is_done_rather_than_asked_about() {
        // The other way into the same cell: a build that reaches it without a long press — a
        // probe, or a board that grows a button later — still must not leave a question up that
        // nothing on that board can answer.
        let mut manager = on_network_page().with_panel(Panel::Absent);
        link_up(&mut manager, b"neighbour");

        tap_strip(&mut manager);
        assert_eq!(manager.state().setup.confirm, None);
        assert!(manager.take_leave(), "the strip's verb is the leave itself");
    }

    #[test]
    fn leaving_happens_when_the_question_is_answered_and_not_when_it_is_left_alone() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");

        // `KEEP`: the question goes and the link stays.
        tap_strip(&mut manager);
        tap_answer(&mut manager, keyboard::DialogChoice::Keep);
        assert_eq!(manager.state().setup.confirm, None);
        assert!(!manager.take_leave(), "the safe answer takes nothing down");
        assert_eq!(
            manager.state().link.phase,
            LinkPhase::Connected,
            "and the link is still the one it was"
        );

        // The action button: the same question, answered the other way.
        tap_strip(&mut manager);
        tap_answer(&mut manager, keyboard::DialogChoice::Act);
        assert_eq!(manager.state().setup.confirm, None, "the question is done");
        assert!(manager.take_leave(), "and the request is on its way");
    }

    #[test]
    fn a_question_about_the_link_does_not_outlive_the_link() {
        // The box is drawn and answered a moment apart, and a link that dropped in between is
        // not one to take down: the question goes with the link it was about.
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");
        tap_strip(&mut manager);
        assert!(manager.state().setup.confirm.is_some());

        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::NotFound),
            ap: None,
            address: None,
            ..LinkStatus::default()
        }));
        assert_eq!(
            manager.state().setup.confirm,
            None,
            "a link that is gone takes its own question with it"
        );
        assert!(!manager.take_leave());
    }

    #[test]
    fn a_survey_round_ends_the_strips_wait() {
        // The cue exists so a tap that changes nothing visible still answers: a report about
        // the link is what a round of the survey publishes, so it is what clears it.
        let mut manager = on_network_page();
        tap_strip(&mut manager);
        assert!(manager.state().setup.rescan_wait);
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus::default()));
        assert!(
            !manager.state().setup.rescan_wait,
            "the round came back, so the strip stops waiting"
        );
    }

    #[test]
    fn a_hold_on_a_saved_row_asks_to_forget_it() {
        // The second verb for a row: a tap joins the network, a hold asks about the copy this
        // device keeps of it — and the store is what says whether it keeps one.
        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        hold_band(&mut manager, 1);
        assert_eq!(
            manager.state().setup.confirm,
            Some(Confirm::Forget(ap(b"cafe"))),
            "the band held is the network the box asks about"
        );
        assert!(
            !manager.state().setup.keyboard,
            "a hold is not a tap: nothing is picked and nothing is typed"
        );
        assert_eq!(manager.state().setup.selected, None);

        tap_answer(&mut manager, keyboard::DialogChoice::Act);
        assert_eq!(
            manager.take_forget(),
            Some(Ssid::from_octets(b"cafe")),
            "the answer hands the name to the task that holds the store"
        );
        assert_eq!(manager.take_forget(), None, "once");
    }

    #[test]
    fn a_hold_on_a_row_this_device_has_not_saved_asks_nothing() {
        // There is nothing to forget, so there is no question: a box that asked anyway would
        // be offering to erase a memory the device does not have.
        let mut manager = on_network_page();
        hold_band(&mut manager, 0);
        assert_eq!(manager.state().setup.confirm, None);
        assert_eq!(manager.take_forget(), None);

        // And the mark is the store's own answer rather than the page's guess: a name it
        // reports is one the hold asks about, and one it stops reporting is one it does not.
        marked(&mut manager, &["neighbour"]);
        hold_band(&mut manager, 0);
        assert!(manager.state().setup.confirm.is_some());
        manager.apply_business(BusinessIntent::Dismiss);
        marked(&mut manager, &[]);
        hold_band(&mut manager, 0);
        assert_eq!(
            manager.state().setup.confirm,
            None,
            "a name the store no longer holds is not one to ask about"
        );
    }

    #[test]
    fn the_stores_marks_ride_the_snapshot_the_panel_draws_from() {
        // The mark is drawn by a sink that holds only the snapshot, so the marks have to be in
        // it — and they are marks rather than names: what joins the networks stays with the
        // radio, and the page only ever asks about the rows in front of it.
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour", "cafe"]);
        let snapshot = manager.state();
        let list = snapshot.aps.expect("the page has a list");
        assert!(list.is_saved(0), "the row the store holds is marked");
        assert!(list.is_saved(1), "and so is the other one");
        assert!(
            !list.is_saved(2),
            "a row the store does not hold is not marked, and neither is a row that is not there"
        );

        marked(&mut manager, &["cafe"]);
        let list = manager.state().aps.expect("the page has a list");
        assert!(!list.is_saved(0), "a mark the store dropped is gone");
        assert!(list.is_saved(1));
    }

    #[test]
    fn the_same_row_is_a_request_to_join_while_the_link_is_not_on_it() {
        // The row is the link's own only while the link is *up*: a device that has an access
        // point recorded from an attempt that failed still has to be able to join it.
        let mut manager = on_network_page();
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ap: Some(ap(b"neighbour")),
            address: None,
            ..LinkStatus::default()
        }));

        tap_band(&mut manager, 0);
        assert!(!manager.take_leave());
        assert!(
            manager
                .state()
                .setup
                .selected
                .is_some_and(|picked| picked.is_same_network(&ap(b"neighbour"))),
            "the row is a selection, and the passphrase can be fixed from there"
        );
    }

    #[test]
    fn the_row_the_link_is_on_is_neither_a_leave_nor_a_pick() {
        // Leaving is the strip's, not the row's: one row meaning its own opposite was a tap
        // with two meanings and nothing on the panel to tell which one was coming. And under a
        // link the row means nothing at all — the strip is the page's one way off it, so the
        // row that is up is no more a pick than any other row is.
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");

        tap_band(&mut manager, 0);
        assert!(
            !manager.take_leave(),
            "a tap on the link's own row is not a leave"
        );
        assert!(
            !manager.state().setup.keyboard,
            "and it is not a pick while the link is up"
        );
        assert_eq!(
            manager.state().setup.selected,
            None,
            "nothing on the page moved"
        );
    }

    #[test]
    fn a_scan_that_shrank_pulls_the_list_back_to_its_own_end() {
        let mut manager = landed(DeviceManager::new().with_network(true));
        let eight: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
        manager.apply_scan(ApList::from_ranked(&eight));
        manager.apply_business(BusinessIntent::NextPage);
        manager.apply_business(BusinessIntent::Scroll { rows: 1 });
        assert_eq!(manager.state().setup.list_top, 1);

        manager.apply_scan(ApList::from_ranked(&[ap(b"a"), ap(b"b")]));
        assert_eq!(
            manager.state().setup.list_top,
            0,
            "a list shorter than its own window cannot be scrolled past its end"
        );
        // Both of its rows are still reachable afterwards.
        tap_band(&mut manager, 1);
        assert_eq!(manager.state().setup.selected, Some(ap(b"b")));
    }

    #[test]
    fn connect_hands_over_what_was_typed_for_the_row_that_was_picked() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        // `b` is the phone layer's second key tapped twice: the first tap types `a` and the
        // second cycles it, because two letters off one key in a row are one character.
        type_letter(&mut manager, b'b', 0);
        type_letter(&mut manager, b'd', 1_000);
        key(&mut manager, Key::Connect);

        let credential = handed_over(&mut manager).expect("connect hands one over");
        assert_eq!(credential.ssid, Ssid::from_octets(b"neighbour"));
        assert_eq!(credential.password.as_bytes(), b"bd");
        assert!(
            !manager.state().setup.keyboard,
            "the keyboard closes on connect: the radio says whether it worked"
        );
        assert_eq!(
            manager.state().setup.entry.map(|entry| entry.len()),
            Some(2),
            "and the draft stays: a refusal comes back to the text it refused"
        );
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"neighbour")),
            "the page keeps showing which network is being tried"
        );
        assert_eq!(
            handed_over(&mut manager),
            None,
            "a credential is a command, not a latch: it is handed over once"
        );
    }

    #[test]
    fn an_open_network_is_handed_over_without_a_keyboard() {
        // The panel's own answer to "open": there is nothing to type, so the tap that picks the
        // row is the whole handover — and the page goes back to the list, where the phase and
        // the address have somewhere to be read.
        let mut manager = on_network_page();
        manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
        tap_band(&mut manager, 0);
        assert!(
            !manager.state().setup.keyboard,
            "no keyboard for a network that asks for nothing"
        );
        assert_eq!(
            manager.state().setup.selected,
            Some(open(b"cafe")),
            "but the page still says which network is being tried"
        );
        let credential = handed_over(&mut manager).expect("the pick hands one over");
        assert_eq!(credential.ssid, Ssid::from_octets(b"cafe"));
        assert!(
            credential.password.as_bytes().is_empty(),
            "no passphrase rather than a guess at one"
        );
    }

    #[test]
    fn a_tap_on_a_saved_row_asks_for_the_copy_the_device_keeps() {
        // The row is one the device can join on its own, so the tap says *which* network and
        // nothing else: no keyboard, because there is nothing to type, and the passphrase never
        // leaves the store.
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        tap_band(&mut manager, 0);
        assert_eq!(
            manager.take_join(),
            Some(JoinRequest::Held(Ssid::from_octets(b"neighbour"))),
            "the tap names the row for the store rather than for the keyboard"
        );
        assert!(!manager.state().setup.keyboard, "and nothing is asked for");
        assert_eq!(manager.state().setup.entry, None);
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"neighbour")),
            "with the row still saying which network is being tried"
        );
    }

    #[test]
    fn a_saved_row_that_is_open_is_still_handed_over_without_a_keyboard() {
        // An open network's copy is an empty passphrase, which the page can hand over itself:
        // the tap is the whole handover either way, and the row's mark decides nothing here.
        let mut manager = on_network_page();
        manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
        marked(&mut manager, &["cafe"]);
        tap_band(&mut manager, 0);
        assert!(!manager.state().setup.keyboard);
        let credential = handed_over(&mut manager).expect("the pick hands one over");
        assert!(credential.password.is_empty());
    }

    #[test]
    fn a_row_the_store_has_no_copy_of_puts_the_page_back_on_the_keyboard() {
        // The mark is a round old and the store is the truth, so the tap that skipped the
        // keyboard can end on one after all: the radio says there is no copy, and the page asks
        // for the passphrase — over an empty draft, because the tap that asked for the copy
        // opened none.
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        tap_band(&mut manager, 0);
        manager.apply_business(BusinessIntent::NoCopy(Ssid::from_octets(b"neighbour")));
        assert!(manager.state().setup.keyboard, "the page has to ask");
        assert_eq!(
            manager.state().setup.entry,
            Some(Entry::default()),
            "over an empty draft, which is what the keys are drawn from"
        );
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"neighbour")),
            "on the row the finger picked"
        );
    }

    #[test]
    fn a_copy_the_store_has_that_was_refused_comes_back_as_a_keyboard() {
        // The store's copy can be refused like any other, and the page it leaves has to be one
        // the user can type on: the row is picked with no draft under it — the tap that asked
        // for the copy opened none — and a keyboard over nothing is a page with no keys drawn
        // on it at all. So the refusal starts the draft the user edits.
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        tap_band(&mut manager, 0);
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ap: Some(ap(b"neighbour")),
            address: None,
            ..LinkStatus::default()
        }));
        assert!(manager.state().setup.keyboard, "the refusal asks for a fix");
        assert_eq!(manager.state().setup.entry, Some(Entry::default()));
        assert_eq!(
            manager.state().setup.refused,
            Some(NetworkError::Credentials),
            "and the page says what went wrong"
        );
    }

    #[test]
    fn an_answer_about_another_row_is_not_this_rows_question() {
        // A tap on a different row since the ask is a newer question, whose own answer is still
        // on its way. Raising the keyboard over the older name would hand whatever the user
        // types to a network they did not pick — and the page the answer arrives on is one where
        // that shows: the newer row was asked for by name too, so it has no draft to rise over.
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour", "cafe"]);
        tap_band(&mut manager, 0);
        manager.take_join();
        tap_band(&mut manager, 1);
        manager.apply_business(BusinessIntent::NoCopy(Ssid::from_octets(b"neighbour")));
        assert!(
            !manager.state().setup.keyboard,
            "the answer is about a row that is not the picked one"
        );
        assert_eq!(manager.state().setup.entry, None);
        assert_eq!(
            manager.state().setup.selected,
            Some(ap(b"cafe")),
            "which is still the row the finger is on"
        );
        assert_eq!(
            manager.take_join(),
            Some(JoinRequest::Held(Ssid::from_octets(b"cafe"))),
            "and the ask the newer row made is the one still in hand"
        );
    }

    #[test]
    fn a_second_tap_of_the_same_row_keeps_the_draft_through_a_rescan() {
        // A rescan rewrites every row it lists, so "the same network" has to be read off the
        // network and not off the row — otherwise the tap that follows a refusal would wipe the
        // passphrase that refusal was about.
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        key(&mut manager, Key::Connect);
        manager.apply_scan(ApList::from_ranked(&[
            ap_named(b"neighbour", -70, true),
            ap(b"cafe"),
        ]));
        tap_band(&mut manager, 0);
        assert!(
            manager.state().setup.keyboard,
            "the same row opens the entry again over the draft"
        );
        assert_eq!(
            manager
                .state()
                .setup
                .entry
                .map(|entry| entry.as_bytes().to_vec()),
            Some(b"b".to_vec()),
            "the draft belongs to the network, not to the reading of it"
        );
        // A row for a *different* network starts an empty draft: the text belonged to the
        // first one. The keyboard has to go down first, because while it is up the whole panel
        // is keys.
        key(&mut manager, Key::Cancel);
        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().setup.entry.map(|entry| entry.len()),
            Some(0)
        );
        assert_eq!(manager.state().setup.selected, Some(ap(b"cafe")));
    }

    #[test]
    fn the_row_a_link_was_left_from_opens_a_new_draft() {
        // What worked dropped the draft and kept the row it was for, and leaving the link
        // touches neither. So the row of the network this device just had is still the picked
        // one, and tapping it again asks to join it again — over a *new* draft, because there
        // is none left to keep. Keeping the row and skipping the draft is a keyboard raised
        // over nothing: the keys are drawn from the draft and the strip is hidden with them,
        // so the page draws nothing between the header and the bottom edge, and no control on
        // it can be reached. This is the state a panel showed as black.
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        key(&mut manager, Key::Connect);
        link_up(&mut manager, b"neighbour");
        assert!(
            manager.state().setup.selected.is_some() && manager.state().setup.entry.is_none(),
            "the link kept the row and dropped the draft"
        );

        tap_strip(&mut manager);
        tap_answer(&mut manager, keyboard::DialogChoice::Act);
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus::default()));
        assert!(!manager.state().link.is_leavable(), "the link is down");

        tap_band(&mut manager, 0);
        assert!(
            manager.state().setup.keyboard,
            "the row the link was on is a request to join it again"
        );
        assert_eq!(
            manager.state().setup.entry.map(|entry| entry.len()),
            Some(0),
            "and the entry under the keys is a new one, not the draft the link dropped"
        );
    }

    #[test]
    fn a_refused_passphrase_comes_back_to_the_keyboard_over_the_draft() {
        // The likeliest reason an association is refused is a character in the passphrase, so
        // the screen that can fix it comes back up — and the refusal stays written for as long
        // as it takes, because the survey's own phase reports would otherwise bury it.
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        key(&mut manager, Key::Connect);
        assert!(
            !manager.state().setup.keyboard,
            "the keyboard is down while the radio tries"
        );
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ..LinkStatus::default()
        }));
        assert_eq!(
            manager.state().setup.refused,
            Some(NetworkError::Credentials)
        );
        assert!(
            manager.state().setup.keyboard,
            "the keyboard comes back to fix the passphrase"
        );
        assert_eq!(
            manager
                .state()
                .setup
                .entry
                .map(|entry| entry.as_bytes().to_vec()),
            Some(b"b".to_vec()),
            "over the draft that was refused, not over an empty field"
        );
        // What worked is not kept: the radio holds the network, and the passphrase is not left
        // in the state for no reader.
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Connected,
            ..LinkStatus::default()
        }));
        assert_eq!(manager.state().setup.entry, None);
        assert!(
            !manager.state().setup.keyboard,
            "and a link that came up has nothing left to type"
        );
    }

    #[test]
    fn the_reveal_key_shows_the_draft_without_touching_it() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        assert!(!manager.state().setup.revealed);
        key(&mut manager, Key::Reveal);
        assert!(manager.state().setup.revealed);
        assert_eq!(
            manager
                .state()
                .setup
                .entry
                .map(|entry| entry.as_bytes().to_vec()),
            Some(b"b".to_vec()),
            "showing the draft is not typing on it"
        );
        key(&mut manager, Key::Cancel);
        assert!(
            !manager.state().setup.revealed,
            "and leaving the entry puts the stars back"
        );
    }

    #[test]
    fn a_draft_spelling_a_word_reaches_the_radio_byte_for_byte() {
        // The keyboard's own end-to-end claim, driven through the geometry rather than
        // through the draft's API: what a finger types is what the radio is handed. `pass`
        // is the word this is for — two letters off the `pqrs` key in a row, which is the one
        // place a multi-tap keyboard has to be waited out rather than tapped faster.
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        for (n, byte) in b"pass".iter().enumerate() {
            type_letter(&mut manager, *byte, 2_000 * n as u32);
        }
        key(&mut manager, Key::Connect);
        let credential = handed_over(&mut manager).expect("connect hands one over");
        assert_eq!(credential.password.as_bytes(), b"pass");
    }

    #[test]
    fn cancel_throws_the_draft_away_without_asking_the_radio() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        key(&mut manager, Key::Char(b'2'));
        key(&mut manager, Key::Cancel);
        assert_eq!(manager.state().setup.entry, None);
        assert_eq!(manager.state().setup.selected, None);
        assert_eq!(
            handed_over(&mut manager),
            None,
            "cancelling sends nothing to the radio"
        );
    }

    #[test]
    fn a_key_with_no_keyboard_up_types_nothing() {
        // A queued tap can arrive after the entry closed — the same finger that pressed
        // connect. It must not type into the next entry, which is not open yet.
        let mut manager = on_network_page();
        let before = manager.state();
        key(&mut manager, Key::Char(b'2'));
        key(&mut manager, Key::Connect);
        assert_eq!(manager.state(), before);
        assert_eq!(handed_over(&mut manager), None);
    }

    #[test]
    fn a_repeat_of_one_key_cycles_it_and_any_other_key_starts_a_character() {
        let mut entry = Entry::default();
        let two = Layer::Abc.face(1);
        entry.edit(Key::Char(two), 0);
        assert_eq!(
            entry.as_bytes(),
            b"a",
            "the first tap is the first candidate"
        );
        entry.edit(Key::Char(two), 100);
        entry.edit(Key::Char(two), 200);
        assert_eq!(
            entry.as_bytes(),
            b"c",
            "each tap inside the window moves the character on rather than adding one"
        );
        entry.edit(Key::Char(two), 300);
        assert_eq!(entry.as_bytes(), b"a", "and the run comes round again");
        entry.edit(Key::Char(Layer::Abc.face(2)), 400);
        assert_eq!(
            entry.as_bytes(),
            b"ad",
            "another key ends the run and starts a character of its own"
        );
        entry.edit(Key::Char(Layer::Abc.face(2)), 400 + MULTITAP_WINDOW_MS + 1);
        assert_eq!(
            entry.as_bytes(),
            b"add",
            "and so does the same key once the window has closed"
        );
    }

    #[test]
    fn backspace_ends_the_run_so_the_next_tap_starts_a_character() {
        // Cycling replaces the character on the panel, so a character that is no longer there
        // must not be cycled: the run ends with the character.
        let mut entry = Entry::default();
        let two = Layer::Abc.face(1);
        entry.edit(Key::Char(two), 0);
        entry.edit(Key::Char(two), 100);
        entry.edit(Key::Backspace, 200);
        entry.edit(Key::Char(two), 300);
        assert_eq!(entry.as_bytes(), b"a");
        assert_eq!(entry.cycling().map(|c| c.index), Some(0));
    }

    #[test]
    fn the_digit_and_symbol_layers_type_one_character_a_key() {
        // One character a key, so a repeat is a second character rather than a cycle: a
        // passphrase with `00` in it is four taps and not two.
        let mut entry = Entry::default();
        entry.edit(Key::Layer, 0);
        assert_eq!(entry.layer(), Layer::Digits);
        let zero = Layer::Digits.face(9);
        entry.edit(Key::Char(zero), 0);
        entry.edit(Key::Char(zero), 100);
        assert_eq!(entry.as_bytes(), b"00");
        entry.edit(Key::Layer, 200);
        assert_eq!(entry.layer(), Layer::Symbols);
        let at = Layer::Symbols.face(8);
        entry.edit(Key::Char(at), 300);
        assert_eq!(entry.as_bytes(), b"00@");
    }

    #[test]
    fn shift_capitalises_one_letter_and_a_second_tap_locks_it() {
        let mut entry = Entry::default();
        let (two, three) = (Layer::Abc.face(1), Layer::Abc.face(2));
        entry.edit(Key::Shift, 0);
        entry.edit(Key::Char(two), 100);
        entry.edit(Key::Char(three), 200);
        assert_eq!(
            entry.as_bytes(),
            b"Ad",
            "one shot: the capital is the letter after the tap and nothing else"
        );
        // A second tap inside the window locks it, and it stays locked across other keys.
        entry.edit(Key::Shift, 300);
        entry.edit(Key::Shift, 400);
        entry.edit(Key::Char(two), 500);
        entry.edit(Key::Char(three), 600);
        assert_eq!(entry.as_bytes(), b"AdAD");
        // One more tap takes the lock off again, so the letter after it is lower case.
        entry.edit(Key::Shift, 700);
        entry.edit(Key::Char(two), 800);
        assert_eq!(entry.as_bytes(), b"AdADa");
        // Punctuation has no case to capitalise, so a one-shot shift is not spent on it.
        entry.edit(Key::Shift, 900);
        entry.edit(Key::Char(Layer::Abc.face(0)), 1_000);
        entry.edit(Key::Char(two), 1_100);
        assert_eq!(entry.as_bytes(), b"AdADa.A");
        // A letter three taps down a key is still that run's character, so the shift reaches it:
        // `Z` is the shift and then `wxyz` tapped through, which a shift spent on the *first*
        // tap of the run could not type at all.
        entry.edit(Key::Shift, 3_000);
        let nine = Layer::Abc.face(8);
        for tap in 0..4 {
            entry.edit(Key::Char(nine), 3_100 + 100 * tap);
        }
        assert_eq!(entry.as_bytes(), b"AdADa.AZ");
        // And the shift is spent by it: the next letter is lower case again.
        entry.edit(Key::Char(two), 4_000);
        assert_eq!(entry.as_bytes(), b"AdADa.AZa");
    }

    #[test]
    fn the_draft_stops_at_the_length_the_radio_takes() {
        // A passphrase one byte past what the driver accepts is refused rather than
        // truncated there, which reads as a wrong password — so the keyboard stops.
        let mut entry = Entry::default();
        let (two, three) = (Layer::Abc.face(1), Layer::Abc.face(2));
        for n in 0..(PASSWORD_BYTES + 8) {
            // Alternating keys, because a repeat of one would cycle rather than add.
            let face = if n % 2 == 0 { two } else { three };
            entry.edit(Key::Char(face), 100 * n as u32);
        }
        assert_eq!(entry.len(), PASSWORD_BYTES - 1);
        entry.edit(Key::Backspace, 0);
        entry.edit(Key::Char(two), 0);
        assert_eq!(entry.len(), PASSWORD_BYTES - 1);
        assert_eq!(entry.as_bytes().last(), Some(&b'a'));
        // And a key tapped at the cap does not leave the run open behind it, which would
        // make the next tap of it erase the character it could not replace.
        assert_eq!(entry.cycling().map(|c| c.index), Some(0));
    }

    #[test]
    fn the_panel_is_told_which_candidate_a_key_is_holding() {
        // The keyboard's own feedback: the key being cycled says what the next tap of it
        // would type, which is how a passphrase gets typed without reading it back.
        let mut entry = Entry::default();
        assert_eq!(entry.cycling(), None, "nothing is being cycled yet");
        let two = Layer::Abc.face(1);
        entry.edit(Key::Char(two), 0);
        let cycling = entry
            .cycling()
            .expect("the key just tapped is the one cycling");
        assert_eq!(cycling.face, two);
        assert_eq!(cycling.candidates, b"abc");
        assert_eq!(cycling.index, 0);
        entry.edit(Key::Char(two), 100);
        assert_eq!(entry.cycling().map(|c| c.index), Some(1));
        entry.edit(Key::Connect, 200);
        assert_eq!(entry.cycling(), None, "leaving the entry ends the run");
    }

    #[test]
    fn an_empty_passphrase_is_still_a_credential_the_radio_can_refuse() {
        // An open network has none, and the panel has no way to say "open" yet: what it
        // hands over is what was typed, empty included.
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        key(&mut manager, Key::Connect);
        let credential = handed_over(&mut manager).expect("an empty one is still one");
        assert!(credential.password.is_empty());
    }

    #[test]
    fn a_refusal_is_kept_until_another_attempt_starts() {
        // The survey republishes the phase every round, so a page that only read the live
        // phase would print `Off` about a second after a wrong passphrase was typed.
        let mut manager = on_network_page();
        let failed = LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Driver),
            ..LinkStatus::default()
        };
        manager.apply_business(BusinessIntent::LinkReport(failed));
        assert_eq!(
            manager.state().setup.phase(LinkPhase::Off),
            LinkPhase::Failed(NetworkError::Driver),
            "the round's own phase does not bury the refusal"
        );
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Scanning,
            ..LinkStatus::default()
        }));
        assert_eq!(
            manager.state().setup.phase(LinkPhase::Scanning),
            LinkPhase::Failed(NetworkError::Driver),
            "nor does the survey that follows it"
        );
        // Another attempt supersedes it, whichever way it goes.
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Connecting,
            ..LinkStatus::default()
        }));
        assert_eq!(
            manager.state().setup.phase(LinkPhase::Connecting),
            LinkPhase::Connecting
        );
    }

    #[test]
    fn the_page_draws_the_row_being_typed_for_before_the_link_is_on_it() {
        // What the keyboard's own header needs: the name comes from the row the user
        // picked, because the link does not know it until the association succeeds.
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        assert_eq!(manager.state().link.ap, None, "nothing is on the air yet");
        assert_eq!(
            manager.state().setup.selected.map(|ap| ap.ssid),
            Some(Ssid::from_octets(b"neighbour"))
        );
    }
    fn setup_mode(manager: &mut DeviceManager, cancellable: bool) {
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            setup: true,
            setup_cancellable: cancellable,
            front: crate::drivers::network::Front::AccessPoint,
            setup_password: Some(crate::drivers::network::SetupPass::from_array(
                *b"k3mq9t7w2x",
            )),
            ..LinkStatus::default()
        }));
    }

    #[test]
    fn the_strip_trades_a_link_for_setup_mode_and_closes_it_again() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");
        tap_strip(&mut manager);
        tap_answer(&mut manager, keyboard::DialogChoice::Act);
        assert!(manager.take_leave(), "the question is asked, and answered");

        // And the mode the radio reports once the access point is up has the other verbs: the
        // way back on the left, and the front switch on the right.
        setup_mode(&mut manager, true);
        tap_strip_left(&mut manager);
        assert!(
            manager.take_setup_cancel(),
            "a device that keeps a network has a way back to it"
        );
        assert!(
            !manager.take_setup_cancel(),
            "a command is handed over once, like every other"
        );
        // The other cell is the front switch, and it is one request rather than a direction:
        // the radio publishes which end it went to.
        tap_strip(&mut manager);
        assert!(manager.take_switch_front());
        assert!(!manager.take_switch_front());
    }

    #[test]
    fn an_offline_page_keeps_its_ap_cell_but_a_hold_does_not_raise_one() {
        // Nothing is saved, so the page is the list and offers `SCAN | AP`. `AP` is a tap: the one
        // explicit way into setup from a running device.
        let mut manager = landed(DeviceManager::new().with_network(true));
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            browsing: true,
            ..LinkStatus::default()
        }));
        manager.apply_business(BusinessIntent::SwitchFront);
        assert!(manager.take_switch_front(), "the cell is still the way in");

        // A hold is not: setup opens only from the splash, so a press on a running device is the
        // page's own verb rather than an access point nobody asked for.
        let operation = op(InputEvent::Button(ButtonEvent::LongPress));
        let business = translate(&operation, &manager.state());
        assert_ne!(
            business,
            BusinessIntent::SwitchFront,
            "a hold outside setup mode does not switch the front"
        );
    }

    #[test]
    fn a_board_with_no_panel_has_no_front_to_switch_to() {
        // The switch exists for a panel whose touch stopped answering: it is how setup mode is
        // reached without a working page. A board with no panel has no second front to offer, and
        // answering the hold would drop the access point a phone is using.
        let mut manager = DeviceManager::new()
            .with_network(true)
            .with_panel(Panel::Absent);
        setup_mode(&mut manager, false);
        manager.apply_business(BusinessIntent::SwitchFront);
        assert!(
            !manager.take_switch_front(),
            "a lamp-only board has nowhere to switch to"
        );
    }

    #[test]
    fn a_front_switch_that_outlives_setup_mode_is_dropped() {
        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus::default()));
        manager.apply_business(BusinessIntent::SwitchFront);
        assert!(
            !manager.take_switch_front(),
            "a queued switch cannot ask for another front once the mode is gone"
        );
    }

    #[test]
    fn a_device_with_nothing_saved_has_only_the_front_switch_on_the_access_points_front() {
        // Nothing to go back to: cancelling would take the access point down and raise it
        // again, which a phone sees as the page disappearing. So the left cell is not drawn
        // on this front, and the one box the switch fills is both halves of the strip.
        let mut manager = on_network_page();
        setup_mode(&mut manager, false);
        tap_strip_left(&mut manager);
        assert!(!manager.take_setup_cancel());
        assert!(
            manager.take_switch_front(),
            "the strip's one action is the other front end"
        );
    }

    #[test]
    fn the_panels_front_keeps_a_cell_for_looking_around_when_there_is_no_way_back() {
        // The panel's own front has a live list under it, so its left half is the strip's
        // "look around" rather than a cell that does nothing — the same verb the strip carries
        // outside the mode. The right half is the switch, whatever the device keeps.
        let mut manager = on_network_page();
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            setup: true,
            front: crate::drivers::network::Front::Panel,
            ..LinkStatus::default()
        }));
        tap_strip_left(&mut manager);
        assert!(!manager.take_setup_cancel());
        assert!(
            manager.take_scan_now(),
            "a device with no way back still asks the radio to look around"
        );
        assert!(manager.state().setup.rescan_wait, "and the strip says so");
        assert!(!manager.take_switch_front());

        tap_strip(&mut manager);
        assert!(manager.take_switch_front(), "the right half is the switch");
        assert!(!manager.take_scan_now());
    }

    #[test]
    fn the_box_belongs_to_the_access_points_front_and_not_to_a_boot() {
        // The box is the mode's face rather than an announcement: it is up while the phone's
        // front is, and the panel's own front is a list with nothing over it.
        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        assert!(manager.state().link.shows_setup_box());

        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            setup: true,
            front: crate::drivers::network::Front::Panel,
            ..LinkStatus::default()
        }));
        assert!(
            !manager.state().link.shows_setup_box(),
            "the list is what the panel's own front shows"
        );

        // And a refusal raises the access point again under the same front: the box is the
        // mode, so it comes back with it rather than being announced once.
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus::default()));
        setup_mode(&mut manager, true);
        assert!(manager.state().link.shows_setup_box());
    }

    #[test]
    fn the_setup_box_s_other_answer_asks_for_the_access_point_to_be_open() {
        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        tap_answer(&mut manager, keyboard::DialogChoice::Keep);
        assert_eq!(manager.take_setup_open(), Some(true));
        assert_eq!(manager.take_setup_open(), None);

        // And it is not reachable outside the box: a panel's own front has no open access
        // point to ask for.
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            setup: true,
            front: crate::drivers::network::Front::Panel,
            ..LinkStatus::default()
        }));
        manager.apply_business(BusinessIntent::SetOpenAccessPoint(true));
        assert_eq!(manager.take_setup_open(), None);
    }

    #[test]
    fn the_radio_s_three_moments_are_heard() {
        let mut manager = on_network_page().with_playback(true);

        setup_mode(&mut manager, true);
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Playing);
        assert_eq!(
            manager.playback_state().sound,
            Sound::Rise,
            "entering setup mode"
        );

        // The driver's word ends a sound; the state only starts them.
        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        link_up(&mut manager, b"neighbour");
        assert_eq!(
            manager.playback_state().sound,
            Sound::Tap,
            "the network answered"
        );

        manager.apply_business(BusinessIntent::Playback(PlaybackAction::Finished));
        link_at(
            &mut manager,
            LinkPhase::Failed(NetworkError::Credentials),
            b"neighbour",
        );
        assert_eq!(
            manager.playback_state().sound,
            Sound::Error,
            "the network refused"
        );
    }
}

/// The page's moves, walked exhaustively rather than along the paths a test happened to pick.
///
/// The tests above are examples: one path somebody thought of, with the state it expected beside
/// it. That is the wrong shape for a move whose *precondition* an earlier move took away — such a
/// rule can be green for every state its own test walks into while being wrong for a state none of
/// them does. So this walks the reachable states instead: a breadth-first sweep over every move the
/// page has, from the state a booted device is in, with [`broken`]'s invariants checked on every
/// state a move lands on. The moves that reached a broken state are printed with it, so a failure
/// here is a reproduction rather than a verdict.
///
/// Two limits of the alphabet decide what the sweep's answer is worth. Every report it sends is one
/// the radio really makes — `Connected` carries an address, `Connecting` may or may not have a name
/// yet, `Off` and `Scanning` carry neither — because a sweep free to invent a report would find
/// states the device cannot be in and call them reachable. And states are compared through [`key`]:
/// a field left out that a move reads would merge two states the page tells apart.
mod reachable {
    use super::*;
    use crate::keyboard::{self, DialogChoice, Key};
    use alloc::collections::{BTreeSet, VecDeque};
    use alloc::string::String;

    /// Where a list band is tapped and held: inside the row, well clear of the panel's edge.
    const LIST_X: u16 = 30;

    /// How many states the sweep may reach before it is called off. Asserted rather than assumed:
    /// the page settles at under half of it, so reaching it means a move was added that the page's
    /// own state cannot really be in.
    const SWEEP_CAP: usize = 20_000;

    /// The sweep's key for a state: the fields a move branches on, written out as bytes.
    ///
    /// The fields only the *drawing* reads — the reveal, the last refusal, the strip's wait — are
    /// left out, and that costs nothing: [`broken`] is checked on the state every move lands on
    /// whether or not a state like it has been seen. What the key is for is the frontier.
    ///
    /// Bytes rather than a hash, because a hash's collision would merge two states and drop the
    /// moves leading out of one of them — a silent hole in the one thing this test is for.
    fn key(state: &DeviceState) -> Vec<u8> {
        let mut key = Vec::with_capacity(64);
        key.push(state.page as u8);
        key.push(u8::from(state.link.is_leavable()));
        encode_ap(&mut key, state.link.ap);
        key.push(u8::from(state.link.address.is_some()));
        match state.aps {
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
        encode_ap(&mut key, state.setup.selected);
        key.push(u8::from(state.setup.entry.is_some()));
        key.push(u8::from(state.setup.keyboard));
        key.push(state.setup.list_top);
        match state.setup.confirm {
            None => key.push(0),
            Some(Confirm::Leave) => key.push(1),
            Some(Confirm::Forget(ap)) => {
                key.push(2);
                encode_ap(&mut key, Some(ap));
            }
        }
        key.push(u8::from(state.leave));
        key.push(u8::from(state.scan_now));
        match state.join {
            None => key.push(0),
            // The two shapes of the same command are two states here: one is joined with and
            // the other is looked up, and the moves below do different things with each.
            Some(JoinRequest::Typed(_)) => key.push(1),
            Some(JoinRequest::Held(_)) => key.push(2),
        }
        key.push(u8::from(state.forget.is_some()));
        key
    }

    /// A network as bytes: the BSSID, which is the field that names it, and whether it asks for
    /// a passphrase, which is the field a pick branches on.
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

    /// The invariants the page's own drawing and hit rules rest on, read off a state some move
    /// reached: `None` when they all hold.
    ///
    /// Each one is the negation of a state that draws nothing, offers nothing, or offers
    /// something that is not there — the failures this page has no way to be loud about, because
    /// what breaks is what the panel puts on the glass.
    fn broken(state: &DeviceState) -> Option<&'static str> {
        if state.setup.keyboard && state.setup.entry.is_none() {
            return Some(
                "the keyboard is up with no draft under it. The keys are drawn from the draft \
                     and the strip is hidden with them, so the page draws nothing between the \
                     header and the bottom edge, and no control on it can be reached",
            );
        }
        if state.setup.entry.is_some() && state.setup.selected.is_none() {
            return Some(
                "a draft is kept for no row, so nothing names the network it would be handed \
                     to",
            );
        }
        if state.setup.confirm == Some(Confirm::Leave) && !state.link.is_leavable() {
            return Some(
                "the page is asking about leaving a link that is not up, so the box has no \
                     answer left to take",
            );
        }
        if state.setup.rescan_wait && state.link.is_leavable() {
            return Some(
                "the strip is waiting for a survey round under a link it would offer to leave",
            );
        }
        None
    }

    /// How the sweep reached `at`, with `this` as the move that was applied to it.
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

    /// One classified event, applied the way the render loop applies one: the operation first,
    /// then the business intent `translate` makes of it against the state that operation left.
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

    /// A network whose BSSID is its own, so the rows of a list stay distinct states.
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
        manager.apply_business(BusinessIntent::LinkReport(status));
    }

    fn scan_of(manager: &mut DeviceManager, rows: &[AccessPoint]) {
        manager.apply_scan(ApList::from_ranked(rows));
    }

    // The page's own moves.
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
    /// The input task's clock ending the splash: a report rather than a gesture, and the only
    /// way off the page.
    fn splash_over(manager: &mut DeviceManager) {
        manager.apply_business(BusinessIntent::SplashOver);
    }
    fn click(manager: &mut DeviceManager) {
        applied(manager, InputEvent::Button(ButtonEvent::Click));
    }
    fn hold_the_button(manager: &mut DeviceManager) {
        applied(manager, InputEvent::Button(ButtonEvent::LongPress));
    }

    // The radio's own reports, each one a shape the radio really makes.
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

    // The survey's own answers.
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

    // The render loop's own half: one latch handed over, and the radio's answer to it.
    fn the_leave_is_carried_out(manager: &mut DeviceManager) {
        if manager.take_leave() {
            nothing_on_the_air(manager);
        }
    }
    // A typed credential is joined with, and the radio's first report about the attempt is the
    // phase it starts in: an association with no name yet, because the driver has not said which
    // access point answered.
    fn the_typed_join_is_carried_out(manager: &mut DeviceManager) {
        if let Some(JoinRequest::Typed(_)) = manager.take_join() {
            attempt_without_a_name(manager);
        }
    }
    // And the two answers a name has: the copy is on the part, so the link goes after it and
    // reports the association, or there is none and the page is told to ask. Both take the same
    // command, so the sweep forks on the answer rather than on the ask.
    fn the_held_join_is_carried_out(manager: &mut DeviceManager) {
        if let Some(JoinRequest::Held(_)) = manager.take_join() {
            associated(manager);
        }
    }
    fn the_missing_copy_is_reported(manager: &mut DeviceManager) {
        if let Some(JoinRequest::Held(ssid)) = manager.take_join() {
            manager.apply_business(BusinessIntent::NoCopy(ssid));
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
            // The store's own erase, as the network task answers it: the same rows, marked
            // against a store that no longer holds the name.
            let mut list = manager.state().aps.unwrap_or_else(ApList::empty);
            list.mark_saved(0);
            manager.apply_scan(list);
        }
    }

    /// One move of the page, and the words a failure prints it as.
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
        let start = DeviceManager::new().with_network(true);
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

        // What the sweep reached, which is what makes it a sweep of this page rather than of
        // nothing. Every state named here is one the moves above have to be able to get to, and
        // one that stops being reached is an alphabet that lost a move — a floor on the count
        // would say the same thing less exactly, and would break on any legitimate cut.
        let reached = |wanted: fn(&DeviceState) -> bool| {
            states.iter().any(|manager| wanted(&manager.state()))
        };
        assert!(
            reached(|state| state.setup.keyboard && state.setup.entry.is_some()),
            "the keyboard never came up over a draft"
        );
        assert!(
            reached(|state| state.setup.selected.is_some() && state.setup.entry.is_none()),
            "a row was never left picked with no draft: the state a link that came up leaves \
                 behind, and the one the pick after it reads"
        );
        assert!(
            reached(|state| state.setup.confirm == Some(Confirm::Leave)),
            "the strip never asked about leaving"
        );
        assert!(
            reached(|state| matches!(state.setup.confirm, Some(Confirm::Forget(_)))),
            "a hold never asked about a saved row"
        );
        assert!(
            reached(|state| state.leave),
            "a leave was never waiting for the network task"
        );
        assert!(
            reached(|state| state.scan_now),
            "the strip never asked to look around"
        );
        assert!(
            reached(|state| matches!(state.join, Some(JoinRequest::Typed(_)))),
            "a credential was never handed up"
        );
        assert!(
            reached(|state| matches!(state.join, Some(JoinRequest::Held(_)))),
            "a saved row was never asked for by name"
        );
        assert!(
            reached(|state| state.link.is_leavable()),
            "the link was never up"
        );
        assert!(
            reached(|state| state.page != DisplayPage::Network),
            "the page cycle was never walked"
        );
    }
}
