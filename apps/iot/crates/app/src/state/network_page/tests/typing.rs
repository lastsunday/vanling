//! The draft: what a key does to it, and when it reaches the radio.

use super::*;

#[test]
fn the_reveal_key_shows_the_draft_without_touching_it() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    type_letter(&mut manager, b'b', 0);
    assert!(!manager.state().network_page.revealed);
    key(&mut manager, Key::Reveal);
    assert!(manager.state().network_page.revealed);
    assert_eq!(
        manager
            .state()
            .network_page
            .entry
            .map(|entry| entry.as_bytes().to_vec()),
        Some(b"b".to_vec()),
        "showing the draft is not typing on it"
    );
    key(&mut manager, Key::Cancel);
    assert!(
        !manager.state().network_page.revealed,
        "and leaving the entry puts the stars back"
    );
}

#[test]
fn a_draft_spelling_a_word_reaches_the_radio_byte_for_byte() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    for (n, byte) in b"pass".iter().enumerate() {
        type_letter(&mut manager, *byte, 2_000 * n as u32);
    }
    let handed = key(&mut manager, Key::Connect);
    let credential = handed_over(handed).expect("connect hands one over");
    assert_eq!(credential.password.as_bytes(), b"pass");
}

#[test]
fn cancel_throws_the_draft_away_without_asking_the_radio() {
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    key(&mut manager, Key::Char(b'2'));
    let handed = key(&mut manager, Key::Cancel);
    assert_eq!(manager.state().network_page.entry, None);
    assert_eq!(manager.state().network_page.selected, None);
    assert_eq!(
        handed_over(handed),
        None,
        "cancelling sends nothing to the radio"
    );
}

#[test]
fn a_key_with_no_keyboard_up_types_nothing() {
    let mut manager = on_network_page();
    let before = manager.state();
    assert_eq!(key(&mut manager, Key::Char(b'2')), None);
    assert_eq!(key(&mut manager, Key::Connect), None);
    assert_eq!(manager.state(), before);
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
    entry.edit(Key::Shift, 300);
    entry.edit(Key::Shift, 400);
    entry.edit(Key::Char(two), 500);
    entry.edit(Key::Char(three), 600);
    assert_eq!(entry.as_bytes(), b"AdAD");
    entry.edit(Key::Shift, 700);
    entry.edit(Key::Char(two), 800);
    assert_eq!(entry.as_bytes(), b"AdADa");
    entry.edit(Key::Shift, 900);
    entry.edit(Key::Char(Layer::Abc.face(0)), 1_000);
    entry.edit(Key::Char(two), 1_100);
    assert_eq!(entry.as_bytes(), b"AdADa.A");
    entry.edit(Key::Shift, 3_000);
    let nine = Layer::Abc.face(8);
    for tap in 0..4 {
        entry.edit(Key::Char(nine), 3_100 + 100 * tap);
    }
    assert_eq!(entry.as_bytes(), b"AdADa.AZ");
    entry.edit(Key::Char(two), 4_000);
    assert_eq!(entry.as_bytes(), b"AdADa.AZa");
}

#[test]
fn the_draft_stops_at_the_length_the_radio_takes() {
    let mut entry = Entry::default();
    let (two, three) = (Layer::Abc.face(1), Layer::Abc.face(2));
    for n in 0..(PASSWORD_BYTES + 8) {
        let face = if n % 2 == 0 { two } else { three };
        entry.edit(Key::Char(face), 100 * n as u32);
    }
    assert_eq!(entry.len(), PASSWORD_BYTES - 1);
    entry.edit(Key::Backspace, 0);
    entry.edit(Key::Char(two), 0);
    assert_eq!(entry.len(), PASSWORD_BYTES - 1);
    assert_eq!(entry.as_bytes().last(), Some(&b'a'));
    assert_eq!(entry.cycling().map(|c| c.index), Some(0));
}

#[test]
fn the_panel_is_told_which_candidate_a_key_is_holding() {
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
    let mut manager = on_network_page();
    tap_band(&mut manager, 0);
    let handed = key(&mut manager, Key::Connect);
    let credential = handed_over(handed).expect("an empty one is still one");
    assert!(credential.password.is_empty());
}

#[test]
fn a_refusal_is_kept_until_another_attempt_starts() {
    let mut manager = on_network_page();
    let failed = LinkStatus {
        phase: LinkPhase::Failed(NetworkError::Driver),
        ..LinkStatus::default()
    };
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(failed)));
    assert_eq!(
        manager.state().network_page.phase(LinkPhase::Off),
        LinkPhase::Failed(NetworkError::Driver),
        "the round's own phase does not bury the refusal"
    );
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase: LinkPhase::Scanning,
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.phase(LinkPhase::Scanning),
        LinkPhase::Failed(NetworkError::Driver),
        "nor does the scan that follows it"
    );
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase: LinkPhase::Connecting,
        ..LinkStatus::default()
    })));
    assert_eq!(
        manager.state().network_page.phase(LinkPhase::Connecting),
        LinkPhase::Connecting
    );
}
