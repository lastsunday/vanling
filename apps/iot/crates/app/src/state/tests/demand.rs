use super::*;
use iot_core::demand::{ALL_KEYS, Claim, DemandKey, DemandKind, Mode, Priority};

fn fully_wired() -> DeviceManager {
    fully_wired_on(Page::Ambient)
}

fn fully_wired_on(boot_page: Page) -> DeviceManager {
    DeviceManager::boot(Boot {
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
        Page::Attitude
            .demand(ALL_KEYS)
            .claim(DemandKey::MOTION)
            .priority,
        Priority::Primary,
        "the page that draws the motion sample may wait nothing for it"
    );
    assert_eq!(
        Page::Audio
            .demand(ALL_KEYS)
            .claim(DemandKey::AUDIO)
            .priority,
        Priority::Primary,
        "and the same for the page that draws the envelope"
    );

    for page in CYCLE {
        if matches!(page, Page::Attitude | Page::Camera) {
            continue;
        }
        assert_eq!(
            page.demand(ALL_KEYS).claim(DemandKey::MOTION).priority,
            Priority::Secondary,
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
    let panel_only = DeviceManager::boot(Boot {
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
    let claimed = Page::Camera.demand(ALL_KEYS);

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
        if page == Page::Audio {
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
    let twin = DeviceManager::boot(Boot {
        demand_keys: alloc::vec![
            DemandKey::instance(DemandKind::Audio, 0),
            DemandKey::instance(DemandKind::Audio, 1),
        ],
        ..Default::default()
    });
    let mut demand = *twin.demand();
    demand.set(
        DemandKey::instance(DemandKind::Audio, 0),
        Claim::new(Mode::Continuous, 0, Priority::Primary),
    );
    demand.set(
        DemandKey::instance(DemandKind::Audio, 1),
        Claim::new(Mode::Continuous, 5_000, Priority::Secondary),
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
    let before = *manager.demand();

    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));

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
    by_page.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
    assert_eq!(
        by_page.demand(),
        &DeviceManager::proposed(&by_page.state, &by_page.demand_keys, &by_page.override_)
    );

    for manager in [
        DeviceManager::boot(Boot {
            backed: Backed {
                camera: true,
                ..Backed::NONE
            },
            ..Default::default()
        }),
        DeviceManager::boot(Boot {
            backed: Backed {
                audio: true,
                ..Backed::NONE
            },
            ..Default::default()
        }),
        DeviceManager::boot(Boot {
            backed: Backed {
                playback: true,
                ..Backed::NONE
            },
            ..Default::default()
        }),
        DeviceManager::boot(Boot {
            backed: Backed {
                motion: Some(MotionCapabilities::EMPTY),
                ..Backed::NONE
            },
            boot_page: Page::Attitude,
            ..Default::default()
        }),
    ] {
        assert_eq!(
            manager.demand(),
            &DeviceManager::proposed(&manager.state, &manager.demand_keys, &manager.override_)
        );
    }
}
