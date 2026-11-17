//! What the page keeps and puts away as the device moves off it and back.

use super::*;

#[test]
fn leaving_puts_the_edit_away_and_keeps_the_row_it_was_for() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    let editing = manager.network_view();
    assert!(
        editing.keyboard,
        "a secured row with no saved copy asks for one"
    );
    let entry = editing.entry.expect("the keyboard is up over a draft");
    let selected = editing.selected.expect("the draft belongs to a row");

    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
    assert_ne!(manager.state().page(), PageId::Network);
    let away = manager.network_view();
    assert!(
        !away.keyboard && !away.revealed && away.confirm.is_none(),
        "the page out of sight is not still asking for a passphrase"
    );
    assert_eq!(
        away.entry,
        Some(entry),
        "the typed draft is the page's to keep"
    );
    assert_eq!(away.selected, Some(selected));

    for _ in 0..PageId::ALL.len() {
        if manager.state().page() == PageId::Network {
            break;
        }
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
    }
    assert_eq!(manager.state().page(), PageId::Network);
    let back = manager.network_view();
    assert!(
        !back.keyboard,
        "coming back does not reopen the edit by itself"
    );
    assert_eq!(back.entry, Some(entry));
    assert_eq!(back.selected, Some(selected));
}

#[test]
fn a_hidden_page_still_reads_the_link() {
    let mut manager = on_network_page();
    tap_strip(&mut manager);
    assert!(
        manager.network_view().rescan_wait,
        "the strip asked to look around"
    );

    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
    assert_ne!(manager.state().page(), PageId::Network);
    link_at(&mut manager, LinkPhase::Scanning, b"neighbour");
    assert!(
        !manager.network_view().rescan_wait,
        "a page out of sight still hears the round it asked for"
    );
}
