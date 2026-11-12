//! Setup mode, the front switch, and the marks the panel draws from.

use super::*;

#[test]
fn the_stores_marks_ride_the_snapshot_the_panel_draws_from() {
    let mut manager = on_network_page();
    marked(&mut manager, &["neighbour", "cafe"]);
    let snapshot = manager.state();
    let list = snapshot.network.aps.expect("the page has a list");
    assert!(list.is_saved(0), "the row the store holds is marked");
    assert!(list.is_saved(1), "and so is the other one");
    assert!(
        !list.is_saved(2),
        "a row the store does not hold is not marked, and neither is a row that is not there"
    );

    marked(&mut manager, &["cafe"]);
    let list = manager.state().network.aps.expect("the page has a list");
    assert!(!list.is_saved(0), "a mark the store dropped is gone");
    assert!(list.is_saved(1));
}

#[test]
fn the_strip_trades_a_link_for_setup_mode_and_closes_it_again() {
    let mut manager = on_network_page();
    link_up(&mut manager, b"neighbour");
    tap_strip(&mut manager);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Act),
        Some(Command::Network(NetworkCommand::Leave)),
        "the question is asked, and answered"
    );

    setup_mode(&mut manager, true);
    assert_eq!(
        tap_strip_left(&mut manager),
        Some(Command::Network(NetworkCommand::LeaveSetup {
            from: SetupHost::AccessPoint
        })),
        "a device that keeps a network has a way back to it"
    );
    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::AccessPoint
        }))
    );
}

#[test]
fn an_offline_page_keeps_its_ap_cell_but_a_hold_does_not_raise_one() {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    }));
    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Browsing,
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.apply_business(BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost)),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::Panel
        })),
        "the cell is still the way in"
    );

    let operation = op(InputEvent::Button(ButtonEvent::LongPress));
    let business = translate(&operation, &manager.state());
    assert_ne!(
        business,
        BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost),
        "a hold outside setup mode does not switch the host"
    );
}

#[test]
fn a_board_with_no_display_has_no_front_to_switch_to() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
        ..Default::default()
    });
    setup_mode(&mut manager, false);
    assert_eq!(
        manager.apply_business(BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost)),
        None,
        "a lamp-only board has nowhere to switch to"
    );
}

#[test]
fn a_front_switch_that_outlives_setup_mode_is_dropped() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, true);
    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(
        LinkStatus::default(),
    )));
    assert_eq!(
        manager.apply_business(BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost)),
        None,
        "a switch cannot ask for another host once the mode is gone"
    );
}

#[test]
fn a_device_with_nothing_saved_has_only_the_front_switch_on_the_access_points_front() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, false);
    assert_eq!(
        tap_strip_left(&mut manager),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::AccessPoint
        })),
        "the strip's one action is the other host"
    );
}

#[test]
fn the_panels_front_keeps_a_cell_for_looking_around_when_there_is_no_way_back() {
    let mut manager = on_network_page();
    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::Panel,
            cancellable: false,
        },
        ..LinkStatus::default()
    })));
    assert_eq!(
        tap_strip_left(&mut manager),
        Some(Command::Network(NetworkCommand::ScanNow)),
        "a device with no way back still asks the radio to look around"
    );
    assert!(
        manager.state().network_page.rescan_wait,
        "and the strip says so"
    );

    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::Panel
        })),
        "the right half is the switch"
    );
}

#[test]
fn the_box_belongs_to_the_access_points_front_and_not_to_a_boot() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, true);
    assert!(manager.state().network.link.shows_setup_box());

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::Panel,
            cancellable: false,
        },
        ..LinkStatus::default()
    })));
    assert!(
        !manager.state().network.link.shows_setup_box(),
        "the list is what the panel's own host shows"
    );

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(
        LinkStatus::default(),
    )));
    setup_mode(&mut manager, true);
    assert!(manager.state().network.link.shows_setup_box());
}

#[test]
fn the_setup_box_s_other_answer_asks_for_the_access_point_to_be_open() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, true);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Keep),
        Some(Command::Network(NetworkCommand::SetupOpen {
            from: SetupHost::AccessPoint,
            open: true
        }))
    );

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::Panel,
            cancellable: false,
        },
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.apply_business(BusinessIntent::NetworkPage(
            NetworkPageIntent::SetOpenAccessPoint(true)
        )),
        None,
        "the box belongs to the access point's front, not to the panel's"
    );
}

#[test]
fn a_front_switch_waits_for_the_other_host_to_report() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, false);
    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::AccessPoint
        }))
    );
    assert_eq!(
        manager.state().network_page.setup_wait,
        Some(SetupWait::Host(SetupHost::Panel)),
        "the page is waiting for the host it asked the radio for"
    );
    assert_eq!(
        tap_strip(&mut manager),
        None,
        "a second tap while the radio is switching asks for nothing new"
    );

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::Panel,
            cancellable: false,
        },
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.setup_wait,
        None,
        "the host the radio reports is the answer the page was waiting for"
    );
    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::Panel
        })),
        "and the switch is offered again"
    );
    assert!(
        manager.state().network_page.setup_box_pending(),
        "asking for the access point puts its box up before it is reported"
    );
    assert_eq!(
        manager.state().network_page.setup_wait,
        Some(SetupWait::Host(SetupHost::AccessPoint))
    );
}

#[test]
fn a_passphrase_switch_waits_for_the_access_point_to_report_it() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, true);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Keep),
        Some(Command::Network(NetworkCommand::SetupOpen {
            from: SetupHost::AccessPoint,
            open: true
        }))
    );
    assert_eq!(
        manager.state().network_page.setup_wait,
        Some(SetupWait::Open(true))
    );
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Keep),
        None,
        "the same answer is already on its way"
    );
    assert_eq!(
        tap_strip(&mut manager),
        None,
        "and the front switch waits with it"
    );

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable: false,
        },
        setup_password: Some(iot_core::drivers::network::SetupPass::from_array(
            *b"k3mq9t7w2x",
        )),
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.setup_wait,
        Some(SetupWait::Open(true)),
        "a report that has not changed the passphrase is not the answer"
    );

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable: false,
        },
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.setup_wait,
        None,
        "the box now says the access point is open, which is what was asked"
    );
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Keep),
        Some(Command::Network(NetworkCommand::SetupOpen {
            from: SetupHost::AccessPoint,
            open: false
        })),
        "and the next answer asks for the passphrase back"
    );
}

#[test]
fn a_setup_change_stops_waiting_when_setup_mode_ends() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, false);
    tap_strip(&mut manager);
    assert!(manager.state().network_page.setup_wait.is_some());

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Browsing,
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.setup_wait,
        None,
        "a report that is not setup mode ends the wait: no setup report can follow it"
    );
}

#[test]
fn a_refused_setup_change_stops_waiting() {
    let mut manager = on_network_page();
    setup_mode(&mut manager, false);
    tap_strip(&mut manager);
    assert_eq!(
        manager.state().network_page.setup_wait,
        Some(SetupWait::Host(SetupHost::Panel))
    );

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable: false,
        },
        phase: LinkPhase::Failed(NetworkError::Driver),
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.setup_wait,
        None,
        "a refusal is an answer: the page must not keep saying WAIT for a change that failed"
    );
}

#[test]
fn the_radio_s_three_moments_are_heard() {
    let mut manager = booted(Boot {
        backed: Backed {
            network: true,
            playback: true,
            ..Backed::NONE
        },
        ..Default::default()
    });

    setup_mode(&mut manager, true);
    assert_eq!(manager.playback_state().phase, PlaybackPhase::Playing);
    assert_eq!(
        manager.playback_state().sound,
        Sound::Rise,
        "entering setup mode"
    );

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
