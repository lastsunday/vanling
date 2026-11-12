//! Picking a row: the question it asks, the copy it may already have, and what is handed over.

use super::*;

#[test]
fn a_tap_on_a_band_opens_the_keyboard_on_the_network_shown_in_it() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 1);
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"cafe")),
        "the keyboard goes with the network the band was showing"
    );
    assert_eq!(
        manager.state().network_page.entry,
        Some(Entry::default()),
        "and starts empty"
    );
    assert_eq!(
        manager
            .state()
            .network_page
            .entry
            .map(|entry| entry.layer()),
        Some(Layer::Abc),
        "on the layer a network's name is spelled with"
    );
}

#[test]
fn a_pick_is_read_only_while_there_is_a_link_to_take_down() {
    for phase in [LinkPhase::Connecting, LinkPhase::Connected] {
        let mut manager = on_network_page();
        link_at(&mut manager, phase, b"neighbour");
        let asked = tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.selected,
            None,
            "{phase:?} is a link, so the band is not a pick"
        );
        assert!(
            !manager.state().network_page.keyboard,
            "{phase:?}: no keyboard"
        );
        assert_eq!(
            manager.state().network_page.entry,
            None,
            "{phase:?}: no draft"
        );
        assert_eq!(
            handed_over(asked),
            None,
            "{phase:?}: and nothing reaches the radio"
        );
    }
    for phase in [
        LinkPhase::Off,
        LinkPhase::Scanning,
        LinkPhase::Failed(NetworkError::Credentials),
    ] {
        let mut manager = on_network_page();
        link_at(&mut manager, phase, b"neighbour");
        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"cafe")),
            "{phase:?} is not a link, so the band picks the row it shows"
        );
        assert!(
            manager.state().network_page.keyboard,
            "{phase:?}: and opens the keyboard"
        );
    }
}

#[test]
fn a_pick_of_an_open_row_is_read_only_under_a_link_too() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    });
    manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
    manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
    link_at(&mut manager, LinkPhase::Connected, b"neighbour");

    let asked = tap_band(&mut manager, 0);
    assert_eq!(
        manager.state().network_page.selected,
        None,
        "the row is not a pick"
    );
    assert_eq!(handed_over(asked), None, "and nothing reaches the radio");
}

#[test]
fn a_hold_on_a_saved_row_still_asks_while_the_link_is_up() {
    let mut manager = on_network_page();
    marked(&mut manager, &["cafe"]);
    link_up(&mut manager, b"neighbour");

    tap_band(&mut manager, 1);
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "a tap is not a question"
    );

    hold_band(&mut manager, 1);
    assert_eq!(
        manager.state().network_page.confirm,
        Some(Confirm::Forget(ap(b"cafe"))),
        "a hold still is"
    );
}

#[test]
fn a_draft_picked_before_the_radio_associated_is_still_open_under_it() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 1);
    assert!(
        manager.state().network_page.keyboard,
        "the pick opened the keyboard"
    );

    link_at(&mut manager, LinkPhase::Connecting, b"neighbour");
    assert!(
        manager.state().network_page.keyboard,
        "and the association does not close it"
    );
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"cafe")),
        "the draft is still the row it was opened on"
    );
}

#[test]
fn a_hold_on_a_saved_row_asks_to_forget_it() {
    let mut manager = on_network_page();
    marked(&mut manager, &["cafe"]);
    hold_band(&mut manager, 1);
    assert_eq!(
        manager.state().network_page.confirm,
        Some(Confirm::Forget(ap(b"cafe"))),
        "the band held is the network the box asks about"
    );
    assert!(
        !manager.state().network_page.keyboard,
        "a hold is not a tap: nothing is picked and nothing is typed"
    );
    assert_eq!(manager.state().network_page.selected, None);

    assert_eq!(
        tap_answer(&mut manager, layout::DialogChoice::Act),
        Some(Command::Network(NetworkCommand::Forget(Ssid::from_octets(
            b"cafe"
        )))),
        "the answer hands the name to the task that holds the store"
    );
    assert_eq!(manager.state().network_page.confirm, None, "once");
}

#[test]
fn a_hold_on_a_row_this_device_has_not_saved_asks_nothing() {
    let mut manager = on_network_page();
    assert_eq!(hold_band(&mut manager, 0), None);
    assert_eq!(manager.state().network_page.confirm, None);

    marked(&mut manager, &["neighbour"]);
    hold_band(&mut manager, 0);
    assert!(manager.state().network_page.confirm.is_some());
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Dismiss));
    marked(&mut manager, &[]);
    hold_band(&mut manager, 0);
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "a name the store no longer holds is not one to ask about"
    );
}

#[test]
fn the_same_row_is_a_request_to_join_while_the_link_is_not_on_it() {
    let mut manager = on_network_page();
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase: LinkPhase::Failed(NetworkError::Credentials),
        ap: Some(ap(b"neighbour")),
        address: None,
        ..LinkStatus::default()
    })));

    let asked = tap_band(&mut manager, 0);
    assert_ne!(asked, Some(Command::Network(NetworkCommand::Leave)));
    assert!(
        manager
            .state()
            .network_page
            .selected
            .is_some_and(|picked| picked.is_same_network(&ap(b"neighbour"))),
        "the row is a selection, and the passphrase can be fixed from there"
    );
}

#[test]
fn the_row_the_link_is_on_is_neither_a_leave_nor_a_pick() {
    let mut manager = on_network_page();
    link_up(&mut manager, b"neighbour");

    assert_eq!(
        tap_band(&mut manager, 0),
        None,
        "a tap on the link's own row is not a leave"
    );
    assert!(
        !manager.state().network_page.keyboard,
        "and it is not a pick while the link is up"
    );
    assert_eq!(
        manager.state().network_page.selected,
        None,
        "nothing on the page moved"
    );
}

#[test]
fn connect_hands_over_what_was_typed_for_the_row_that_was_picked() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    type_letter(&mut manager, b'b', 0);
    type_letter(&mut manager, b'd', 1_000);
    let handed = key(&mut manager, Key::Connect);

    let credential = handed_over(handed).expect("connect hands one over");
    assert_eq!(credential.ssid, Ssid::from_octets(b"neighbour"));
    assert_eq!(credential.password.as_bytes(), b"bd");
    assert!(
        !manager.state().network_page.keyboard,
        "the keyboard closes on connect: the radio says whether it worked"
    );
    assert_eq!(
        manager.state().network_page.entry.map(|entry| entry.len()),
        Some(2),
        "and the draft stays: a refusal comes back to the text it refused"
    );
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"neighbour")),
        "the page keeps showing which network is being tried"
    );
    assert_eq!(
        handed_over(key(&mut manager, Key::Connect)),
        None,
        "a credential is a command, not a latch: the keyboard is closed on the first"
    );
}

#[test]
fn an_open_network_is_handed_over_without_a_keyboard() {
    let mut manager = on_network_page();
    manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
    let handed = tap_band(&mut manager, 0);
    assert!(
        !manager.state().network_page.keyboard,
        "no keyboard for a network that asks for nothing"
    );
    assert_eq!(
        manager.state().network_page.selected,
        Some(open(b"cafe")),
        "but the page still says which network is being tried"
    );
    let credential = handed_over(handed).expect("the pick hands one over");
    assert_eq!(credential.ssid, Ssid::from_octets(b"cafe"));
    assert!(
        credential.password.as_bytes().is_empty(),
        "no passphrase rather than a guess at one"
    );
}

#[test]
fn a_tap_on_a_saved_row_asks_for_the_copy_the_device_keeps() {
    let mut manager = on_network_page();
    marked(&mut manager, &["neighbour"]);
    assert_eq!(
        tap_band(&mut manager, 0),
        Some(Command::Network(NetworkCommand::Join(JoinRequest::Held(
            Ssid::from_octets(b"neighbour")
        )))),
        "the tap names the row for the store rather than for the keyboard"
    );
    assert!(
        !manager.state().network_page.keyboard,
        "and nothing is asked for"
    );
    assert_eq!(manager.state().network_page.entry, None);
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"neighbour")),
        "with the row still saying which network is being tried"
    );
}

#[test]
fn a_saved_row_that_is_open_is_still_handed_over_without_a_keyboard() {
    let mut manager = on_network_page();
    manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
    marked(&mut manager, &["cafe"]);
    let handed = tap_band(&mut manager, 0);
    assert!(!manager.state().network_page.keyboard);
    let credential = handed_over(handed).expect("the pick hands one over");
    assert!(credential.password.is_empty());
}

#[test]
fn a_row_the_store_has_no_copy_of_puts_the_page_back_on_the_keyboard() {
    let mut manager = on_network_page();
    marked(&mut manager, &["neighbour"]);
    tap_band(&mut manager, 0);
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::NoCopy(
        Ssid::from_octets(b"neighbour"),
    )));
    assert!(manager.state().network_page.keyboard, "the page has to ask");
    assert_eq!(
        manager.state().network_page.entry,
        Some(Entry::default()),
        "over an empty draft, which is what the keys are drawn from"
    );
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"neighbour")),
        "on the row the finger picked"
    );
}

#[test]
fn a_copy_the_store_has_that_was_refused_comes_back_as_a_keyboard() {
    let mut manager = on_network_page();
    marked(&mut manager, &["neighbour"]);
    tap_band(&mut manager, 0);
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase: LinkPhase::Failed(NetworkError::Credentials),
        ap: Some(ap(b"neighbour")),
        address: None,
        ..LinkStatus::default()
    })));
    assert!(
        manager.state().network_page.keyboard,
        "the refusal asks for a fix"
    );
    assert_eq!(manager.state().network_page.entry, Some(Entry::default()));
    assert_eq!(
        manager.state().network_page.refused,
        Some(NetworkError::Credentials),
        "and the page says what went wrong"
    );
}

#[test]
fn an_answer_about_another_row_is_not_this_rows_question() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::NoCopy(
        Ssid::from_octets(b"cafe"),
    )));
    assert!(
        manager.state().network_page.keyboard,
        "the answer is about a row that is not the picked one"
    );
    assert_eq!(
        manager.state().network_page.entry,
        Some(Entry::default()),
        "so the draft for the picked row stays open, and stays empty"
    );
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"neighbour")),
        "which is still the row the finger is on"
    );
}

#[test]
fn a_row_tap_while_the_join_is_pending_is_not_a_second_join() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    type_letter(&mut manager, b'b', 0);
    key(&mut manager, Key::Connect);
    manager.apply_scan(ApList::from_ranked(&[
        ap_named(b"neighbour", -70, true),
        ap(b"cafe"),
    ]));
    assert_eq!(
        tap_band(&mut manager, 0),
        None,
        "the list is behind the join box: a row tap is not a new join"
    );
    link_at(
        &mut manager,
        LinkPhase::Failed(NetworkError::Credentials),
        b"neighbour",
    );
    assert!(
        manager.state().network_page.keyboard,
        "the refusal opens the entry again over the draft"
    );
    assert_eq!(
        manager
            .state()
            .network_page
            .entry
            .map(|entry| entry.as_bytes().to_vec()),
        Some(b"b".to_vec()),
        "the draft belongs to the network, not to the reading of it"
    );
    key(&mut manager, Key::Cancel);
    tap_band(&mut manager, 1);
    assert_eq!(
        manager.state().network_page.entry.map(|entry| entry.len()),
        Some(0)
    );
    assert_eq!(manager.state().network_page.selected, Some(ap(b"cafe")));
}

#[test]
fn the_row_a_link_was_left_from_opens_a_new_draft() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    type_letter(&mut manager, b'b', 0);
    key(&mut manager, Key::Connect);
    link_up(&mut manager, b"neighbour");
    assert!(
        manager.state().network_page.selected.is_some()
            && manager.state().network_page.entry.is_none(),
        "the link kept the row and dropped the draft"
    );

    tap_strip(&mut manager);
    tap_answer(&mut manager, layout::DialogChoice::Act);
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(
        LinkStatus::default(),
    )));
    assert!(
        !manager.state().network.link.is_leavable(),
        "the link is down"
    );

    tap_band(&mut manager, 0);
    assert!(
        manager.state().network_page.keyboard,
        "the row the link was on is a request to join it again"
    );
    assert_eq!(
        manager.state().network_page.entry.map(|entry| entry.len()),
        Some(0),
        "and the entry under the keys is a new one, not the draft the link dropped"
    );
}

#[test]
fn a_refused_passphrase_comes_back_to_the_keyboard_over_the_draft() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    type_letter(&mut manager, b'b', 0);
    key(&mut manager, Key::Connect);
    assert!(
        !manager.state().network_page.keyboard,
        "the keyboard is down while the radio tries"
    );
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase: LinkPhase::Failed(NetworkError::Credentials),
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.refused,
        Some(NetworkError::Credentials)
    );
    assert!(
        manager.state().network_page.keyboard,
        "the keyboard comes back to fix the passphrase"
    );
    assert_eq!(
        manager
            .state()
            .network_page
            .entry
            .map(|entry| entry.as_bytes().to_vec()),
        Some(b"b".to_vec()),
        "over the draft that was refused, not over an empty field"
    );
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase: LinkPhase::Connected,
        ..LinkStatus::default()
    })));
    assert_eq!(manager.state().network_page.entry, None);
    assert!(
        !manager.state().network_page.keyboard,
        "and a link that came up has nothing left to type"
    );
}

#[test]
fn the_page_draws_the_row_being_typed_for_before_the_link_is_on_it() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    assert_eq!(
        manager.state().network.link.ap,
        None,
        "nothing is on the air yet"
    );
    assert_eq!(
        manager.state().network_page.selected.map(|ap| ap.ssid),
        Some(Ssid::from_octets(b"neighbour"))
    );
}
