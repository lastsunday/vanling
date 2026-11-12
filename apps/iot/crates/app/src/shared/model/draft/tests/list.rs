//! The scan list: which band is which row, and how far it scrolls.

use super::*;

#[test]
fn the_address_the_radio_reports_is_the_one_the_page_draws() {
    let mut manager = on_network_page();
    assert_eq!(
        manager.state().network.mac,
        None,
        "nothing has reported one yet"
    );
    let mac = [0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef];
    manager.apply_action(ActionIntent::Link(LinkIntent::Mac(mac)));
    assert_eq!(manager.state().network.mac, Some(mac));
}

#[test]
fn a_band_the_list_has_nothing_in_is_not_a_selection() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 3);
    assert_eq!(
        manager.state().network_page.entry,
        None,
        "a band past the last network is the page's own background"
    );
}

#[test]
fn the_list_slides_a_row_at_a_time_and_a_band_means_the_row_it_shows() {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    }));
    let aps: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
    manager.apply_scan(ApList::from_ranked(&aps));
    manager.apply_action(ActionIntent::Page(PageIntent::NextPage));

    tap_band(&mut manager, 0);
    assert_eq!(manager.state().network_page.selected, Some(ap(b"a")));
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Key {
        key: Key::Cancel,
        at_ms: 1,
    }));

    assert_eq!(manager.state().network_page.list_top, 0);
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Scroll {
        rows: 1,
    }));
    assert_eq!(
        manager.state().network_page.list_top,
        1,
        "a slide forward moves the window one row"
    );
    tap_band(&mut manager, 0);
    assert_eq!(
        manager.state().network_page.selected,
        Some(ap(b"b")),
        "and the first band now holds the second network"
    );
    for _ in 0..(8 - LIST_ROWS) {
        manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Scroll {
            rows: 1,
        }));
    }
    assert_eq!(
        manager.state().network_page.list_top,
        (8 - LIST_ROWS) as u8,
        "eight rows through a three-row window stop five rows down"
    );
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Scroll {
        rows: 1,
    }));
    assert_eq!(
        manager.state().network_page.list_top,
        (8 - LIST_ROWS) as u8,
        "and the end of the list is as far as it goes"
    );
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Scroll {
        rows: -1,
    }));
    assert_eq!(
        manager.state().network_page.list_top,
        (8 - LIST_ROWS - 1) as u8
    );
    for _ in 0..8 {
        manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Scroll {
            rows: -1,
        }));
    }
    assert_eq!(
        manager.state().network_page.list_top,
        0,
        "and the top of it is as far back"
    );
}

#[test]
fn a_hold_on_the_list_under_the_join_box_asks_nothing() {
    let mut manager = on_network_page();
    marked(&mut manager, &["cafe"]);
    link_at(&mut manager, LinkPhase::Connecting, b"cafe");

    let asked = hold_band(&mut manager, 1);
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "a hold under the join box asks nothing"
    );
    assert_eq!(asked, None, "and nothing reaches the store");
}

#[test]
fn a_slide_under_the_join_box_does_not_move_the_list() {
    let mut manager = eight_rows();
    link_at(&mut manager, LinkPhase::Connecting, b"a");
    slide_list(&mut manager);
    assert_eq!(
        manager.state().network_page.list_top,
        0,
        "the window stayed where the box was drawn over it"
    );

    let mut manager = eight_rows();
    slide_list(&mut manager);
    assert_eq!(manager.state().network_page.list_top, 1);
}

#[test]
fn a_scan_that_shrank_pulls_the_list_back_to_its_own_end() {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    }));
    let eight: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
    manager.apply_scan(ApList::from_ranked(&eight));
    manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
    manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::Scroll {
        rows: 1,
    }));
    assert_eq!(manager.state().network_page.list_top, 1);

    manager.apply_scan(ApList::from_ranked(&[ap(b"a"), ap(b"b")]));
    assert_eq!(
        manager.state().network_page.list_top,
        0,
        "a list shorter than its own window cannot be scrolled past its end"
    );
    tap_band(&mut manager, 1);
    assert_eq!(manager.state().network_page.selected, Some(ap(b"b")));
}
