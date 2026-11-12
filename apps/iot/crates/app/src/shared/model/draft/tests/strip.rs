//! The action strip and the link question: leaving, and which command each verb answers with.

use super::*;

#[test]
fn the_action_strip_leaves_while_a_link_is_up_and_looks_around_while_it_is_not() {
    let mut manager = on_network_page();
    link_up(&mut manager, b"neighbour");

    let asked = tap_strip(&mut manager);
    assert_eq!(
        manager.state().network_page.confirm,
        Some(Confirm::Leave),
        "a link that is up is one to take down, and taking it down is asked about"
    );
    assert_eq!(asked, None, "asked about, not done");

    let mut manager = on_network_page();
    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::ScanNow)),
        "with no link there is something to look around for"
    );
    assert!(
        manager.state().network_page.rescan_wait,
        "and the strip waits for the round it asked for"
    );
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "nothing to confirm"
    );
}

#[test]
fn a_link_that_is_coming_up_is_already_one_to_leave() {
    for named in [None, Some(ap(b"neighbour"))] {
        for phase in [LinkPhase::Connecting, LinkPhase::Connected] {
            let mut manager = on_network_page();
            manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
                phase,
                ap: named,
                address: None,
                ..LinkStatus::default()
            })));
            tap_strip(&mut manager);
            assert_eq!(
                manager.state().network_page.confirm,
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
        assert_eq!(
            tap_strip(&mut manager),
            Some(Command::Network(NetworkCommand::ScanNow)),
            "{phase:?} is not a link, so the strip looks around instead"
        );
    }
}

#[test]
fn a_strip_leave_on_a_board_with_no_screen_is_done_rather_than_asked_about() {
    let mut manager = booted(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
        ..Default::default()
    });
    link_up(&mut manager, b"neighbour");

    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::Leave)),
        "the strip's verb is the leave itself"
    );
    assert_eq!(manager.state().network_page.confirm, None);
}

#[test]
fn leaving_happens_when_the_question_is_answered_and_not_when_it_is_left_alone() {
    let mut manager = on_network_page();
    link_up(&mut manager, b"neighbour");

    tap_strip(&mut manager);
    let asked = tap_answer(&mut manager, layout::DialogChoice::Keep);
    assert_eq!(manager.state().network_page.confirm, None);
    assert_eq!(asked, None, "the safe answer takes nothing down");
    assert_eq!(
        manager.state().network.link.phase,
        LinkPhase::Connected,
        "and the link is still the one it was"
    );

    tap_strip(&mut manager);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Act),
        Some(Command::Network(NetworkCommand::Leave)),
        "and the request is on its way"
    );
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "the question is done"
    );
}

#[test]
fn a_question_about_the_link_does_not_outlive_the_link() {
    let mut manager = on_network_page();
    link_up(&mut manager, b"neighbour");
    tap_strip(&mut manager);
    assert!(manager.state().network_page.confirm.is_some());

    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        phase: LinkPhase::Failed(NetworkError::NotFound),
        ap: None,
        address: None,
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "a link that is gone takes its own question with it"
    );
    assert_eq!(
        manager.apply_business(BusinessIntent::NetworkPage(NetworkPageIntent::Leave)),
        None
    );
}

#[test]
fn a_scan_round_ends_the_strips_wait() {
    let mut manager = on_network_page();
    tap_strip(&mut manager);
    assert!(manager.state().network_page.rescan_wait);
    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(
        LinkStatus::default(),
    )));
    assert!(
        !manager.state().network_page.rescan_wait,
        "the round came back, so the strip stops waiting"
    );
}

#[test]
fn each_panel_request_answers_with_its_own_command_on_the_spot() {
    let mut manager = on_network_page();
    assert_eq!(
        manager.apply_business(BusinessIntent::NetworkPage(NetworkPageIntent::Scroll {
            rows: 1
        })),
        None,
        "an intent that asks the radio for nothing answers with nothing"
    );

    let mut manager = on_network_page();
    marked(&mut manager, &["neighbour"]);
    assert_eq!(
        tap_band(&mut manager, 0),
        Some(Command::Network(NetworkCommand::Join(JoinRequest::Held(
            Ssid::from_octets(b"neighbour")
        )))),
        "a saved row is the credential the task is handed"
    );

    let mut manager = on_network_page();
    link_up(&mut manager, b"neighbour");
    tap_strip(&mut manager);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Act),
        Some(Command::Network(NetworkCommand::Leave)),
        "the answered question is the leave"
    );

    let mut manager = on_network_page();
    setup_mode(&mut manager, true);
    assert_eq!(
        tap_strip_left(&mut manager),
        Some(Command::Network(NetworkCommand::LeaveSetup {
            from: SetupHost::AccessPoint
        }))
    );
    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::SwitchHost {
            from: SetupHost::AccessPoint
        }))
    );

    let mut manager = on_network_page();
    setup_mode(&mut manager, true);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Keep),
        Some(Command::Network(NetworkCommand::SetupOpen {
            from: SetupHost::AccessPoint,
            open: true
        }))
    );

    let mut manager = on_network_page();
    assert_eq!(
        tap_strip(&mut manager),
        Some(Command::Network(NetworkCommand::ScanNow)),
        "with no link the strip is the scan"
    );

    let mut manager = on_network_page();
    marked(&mut manager, &["cafe"]);
    hold_band(&mut manager, 1);
    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Act),
        Some(Command::Network(NetworkCommand::Forget(Ssid::from_octets(
            b"cafe"
        ))))
    );
}
