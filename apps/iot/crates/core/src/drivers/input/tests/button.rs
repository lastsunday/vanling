//! The scanner and what a press becomes.

use super::*;

struct FakeButton<'a> {
    pressed: &'a Pressed,
}

impl Button for FakeButton<'_> {
    fn is_pressed(&self) -> bool {
        self.pressed.get()
    }
}

fn hold_and_release(hold_ms: u64) -> Option<ButtonEvent> {
    let pressed = Pressed::new(false);
    let btn = FakeButton { pressed: &pressed };
    let mut scanner = ButtonScanner::new(btn);

    pressed.set(true);
    for ms in 0..=30 {
        scanner.poll(ms * 10);
    }

    pressed.set(false);
    let mut outcome = None;
    for ms in 0..=20 {
        if let Some(InputEvent::Button(e)) = scanner.poll(hold_ms + ms * 10) {
            outcome = Some(e);
        }
    }
    outcome
}

#[test]
fn debounce_confirms_a_click_only_after_n_consecutive_samples() {
    let pressed = Pressed::new(false);
    let mut scanner = ButtonScanner::new(FakeButton { pressed: &pressed });
    for ms in [0, 10, 20] {
        pressed.set(true);
        assert!(scanner.poll(ms).is_none());
    }
    pressed.set(true);
    assert!(scanner.poll(30).is_none());
    pressed.set(false);
    for ms in [40, 50] {
        assert!(scanner.poll(ms).is_none());
    }
    assert_eq!(
        scanner.poll(60),
        Some(InputEvent::Button(ButtonEvent::Click))
    );
    let pressed = Pressed::new(false);
    let mut scanner = ButtonScanner::new(FakeButton { pressed: &pressed });
    pressed.set(true);
    assert!(scanner.poll(0).is_none());
    assert!(scanner.poll(10).is_none());
    pressed.set(false);
    assert!(scanner.poll(20).is_none());
    assert!(scanner.poll(30).is_none());
    assert!(!scanner.pressed, "should still be released");
}

#[test]
fn long_press_threshold_splits_hold_duration_on_release() {
    // LONG_PRESS_MS = 600 is the inclusive boundary: a 600 ms hold is
    // a long-press, a 599 ms hold is still a click.
    assert_eq!(hold_and_release(599), Some(ButtonEvent::Click));
    assert_eq!(hold_and_release(600), Some(ButtonEvent::LongPress));
    assert_eq!(hold_and_release(610), Some(ButtonEvent::LongPress));
    assert_eq!(hold_and_release(800), Some(ButtonEvent::LongPress));
    assert_eq!(LONG_PRESS_MS, 600);
}

#[test]
fn a_held_button_stays_held_through_the_release_debounce() {
    let pressed = Pressed::new(false);
    let mut scanner = ButtonScanner::new(FakeButton { pressed: &pressed });
    assert!(!scanner.held(), "nothing has been pressed");
    pressed.set(true);
    for sample in 0..3 {
        assert!(scanner.poll(1_000 + sample * 10).is_none());
    }
    assert!(scanner.held(), "down, and nothing emitted yet");
    pressed.set(false);
    assert!(
        scanner.held(),
        "the pin is up but the lift is still being debounced"
    );
    for sample in 0..3 {
        if scanner.poll(1_800 + sample * 10).is_some() {
            break;
        }
    }
    assert!(!scanner.held(), "the press resolved, so the hold is over");
}

#[test]
fn button_cadence_and_windows_align_on_the_base_grid() {
    assert_eq!(BUTTON_SCAN_MS, 10);
    const _: () = assert!(BUTTON_SCAN_MS.is_multiple_of(INPUT_BASE_MS));
    const _: () = assert!(DOUBLE_CLICK_WINDOW_MS < LONG_PRESS_MS);
    const _: () = assert!(DOUBLE_CLICK_WINDOW_MS.is_multiple_of(INPUT_BASE_MS));
}

#[test]
fn non_click_events_pass_through_immediately() {
    let mut passthrough = PassThrough;
    let click = InputEvent::Button(ButtonEvent::Click);
    assert_eq!(passthrough.feed(click, 100), Some(click));
    let mut agg = DoubleClickAggregator::new();
    let long = InputEvent::Button(ButtonEvent::LongPress);
    assert_eq!(
        agg.feed(long, 100),
        Some(long),
        "a long-press flushes and forwards"
    );
    assert_eq!(agg.tick(1000), None);
}

#[test]
fn double_click_buffers_pairs_and_flushes() {
    let click = InputEvent::Button(ButtonEvent::Click);
    let mut agg = DoubleClickAggregator::new();
    assert_eq!(agg.feed(click, 100), None, "first click buffers");
    assert_eq!(
        agg.feed(click, 100 + DOUBLE_CLICK_WINDOW_MS),
        None,
        "the second click waits for a possible third"
    );
    assert_eq!(
        agg.tick(100 + 2 * DOUBLE_CLICK_WINDOW_MS + 1),
        Some(InputEvent::Button(ButtonEvent::DoubleClick))
    );

    let mut agg = DoubleClickAggregator::new();
    assert_eq!(agg.feed(click, 100), None);
    assert_eq!(
        agg.tick(100 + DOUBLE_CLICK_WINDOW_MS),
        None,
        "window still open at the boundary"
    );
    assert_eq!(
        agg.tick(100 + DOUBLE_CLICK_WINDOW_MS + 1),
        Some(InputEvent::Button(ButtonEvent::Click)),
        "the tick past the window releases the lone click"
    );

    let mut agg = DoubleClickAggregator::new();
    assert_eq!(agg.feed(click, 100), None);
    assert_eq!(
        agg.feed(click, 100 + DOUBLE_CLICK_WINDOW_MS + 1),
        Some(InputEvent::Button(ButtonEvent::Click)),
        "stale pending click released"
    );
    assert_eq!(
        agg.tick(100 + 2 * (DOUBLE_CLICK_WINDOW_MS + 1)),
        Some(InputEvent::Button(ButtonEvent::Click)),
        "the new buffer expires as a lone click"
    );

    let mut agg = DoubleClickAggregator::new();
    let long = InputEvent::Button(ButtonEvent::LongPress);
    assert_eq!(agg.feed(click, 100), None);
    assert_eq!(
        agg.feed(long, 100 + DOUBLE_CLICK_WINDOW_MS - 1),
        Some(InputEvent::Button(ButtonEvent::Click))
    );
    assert_eq!(agg.tick(100 + 2 * DOUBLE_CLICK_WINDOW_MS), Some(long));
}

#[test]
fn triple_click_uses_the_same_window_and_restarts_afterwards() {
    let click = InputEvent::Button(ButtonEvent::Click);
    let mut agg = DoubleClickAggregator::with_window_ms(DOUBLE_CLICK_WINDOW_MS);
    assert_eq!(agg.feed(click, 0), None);
    assert_eq!(agg.feed(click, 400), None);
    assert_eq!(
        agg.feed(click, 800),
        Some(InputEvent::Button(ButtonEvent::TripleClick))
    );
    assert_eq!(agg.feed(click, 1_200), None);
    assert_eq!(agg.feed(click, 1_600), None);
    assert_eq!(
        agg.feed(click, 2_000),
        Some(InputEvent::Button(ButtonEvent::TripleClick))
    );
}

#[test]
fn poll_entry_anchors_the_first_sample_on_the_cadence_grid() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        BUTTON_SCAN_MS,
    );
    assert_eq!(entry.next_at_ms(), BUTTON_SCAN_MS);
    entry.advance_past(BUTTON_SCAN_MS);
    assert_eq!(entry.next_at_ms(), 2 * BUTTON_SCAN_MS);
    entry.advance_past(2 * BUTTON_SCAN_MS - 1);
    assert_eq!(entry.next_at_ms(), 2 * BUTTON_SCAN_MS);
    entry.advance_past(25);
    assert_eq!(entry.next_at_ms(), 3 * BUTTON_SCAN_MS);
}

#[test]
fn a_deactivated_source_is_due_immediately_once_it_comes_back() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        BUTTON_SCAN_MS,
    );
    entry.advance_past(1_000);
    assert!(entry.is_active());

    entry.set_active(false);
    assert!(!entry.is_active());

    entry.set_active(true);
    assert_eq!(entry.next_at_ms(), 0);
    entry.advance_past(1_000);
    assert!(entry.next_at_ms() > 1_000, "and it is due, not parked");
}

#[test]
fn a_source_that_resumes_after_an_hour_lands_on_the_next_slot_once() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        BUTTON_SCAN_MS,
    );
    entry.set_active(false);
    entry.set_active(true);
    entry.advance_past(3_600_000);
    let next = entry.next_at_ms();
    assert!(next > 3_600_000, "due, not parked at the old schedule");
    assert!(
        next - BUTTON_SCAN_MS <= 3_600_000,
        "and on the first slot past now, not several cadences late"
    );
}

#[test]
fn a_zero_cadence_moves_the_schedule_instead_of_dividing_by_zero() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        0,
    );
    entry.advance_past(1_000);
    assert!(
        entry.next_at_ms() > 1_000,
        "the next tick decides, rather than this one spinning"
    );
}

#[test]
fn deactivating_a_source_that_is_already_off_changes_nothing() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        BUTTON_SCAN_MS,
    );
    entry.advance_past(50);
    let due = entry.next_at_ms();

    entry.set_active(true);
    assert_eq!(entry.next_at_ms(), due);
}

fn button_entry() -> PollEntry {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        BUTTON_SCAN_MS,
    )
}

#[test]
fn a_latency_budget_stretches_the_cadence_and_keeps_the_source_due_now() {
    let mut entry = button_entry();
    let board = entry.board_cadence_ms();
    entry.advance_past(1_000);
    let due = entry.next_at_ms();
    assert!(due > 1_000, "due on the original grid");

    entry.set_cadence_ms(30_000);

    assert_eq!(entry.cadence_ms(), 30_000);
    assert_eq!(
        entry.board_cadence_ms(),
        board,
        "the floor the board wired is not the demand's to move"
    );
    assert_eq!(entry.next_at_ms(), due);
    entry.advance_past(due);
    assert!(
        entry.next_at_ms() >= due + 30_000,
        "and the one after it waits a whole stretched cadence"
    );
}

#[test]
fn a_budget_can_never_shorten_a_cadence_the_board_wired() {
    let mut entry = button_entry();
    let board = entry.board_cadence_ms();
    entry.set_cadence_ms(board - 1);
    entry.set_cadence_ms(0);
    assert_eq!(entry.cadence_ms(), board);
}

#[test]
fn a_cheaper_budget_can_put_a_stretched_cadence_back() {
    let mut entry = button_entry();
    let board = entry.board_cadence_ms();
    entry.set_cadence_ms(30_000);
    entry.advance_past(60_000);

    entry.set_cadence_ms(board);

    assert_eq!(entry.cadence_ms(), board);
    entry.advance_past(60_000);
    assert!(
        entry.next_at_ms() <= 60_000 + board,
        "back on the grid the board wired"
    );
}

#[test]
fn a_resumed_source_is_not_postponed_by_a_longer_cadence() {
    let mut entry = button_entry();
    entry.advance_past(600_000);
    entry.set_active(false);
    entry.set_active(true);
    entry.set_cadence_ms(60_000);

    assert_eq!(entry.next_at_ms(), 0);
}

#[test]
fn poll_entry_wires_scanner_into_the_double_click() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::new(
        0,
        DemandKey::INTERACTION,
        Box::new(ButtonScanner::new(FakeButton { pressed })),
        Box::new(PassThrough),
        BUTTON_SCAN_MS,
    );
    for ms in [0, 10, 20] {
        pressed.set(true);
        assert!(entry.poll(ms).is_none());
    }
    pressed.set(false);
    assert!(entry.poll(40).is_none());
    assert!(entry.poll(50).is_none());
    assert_eq!(entry.poll(60), Some(InputEvent::Button(ButtonEvent::Click)));
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let mut entry = PollEntry::button(0, FakeButton { pressed });
    for ms in [0, 10, 20] {
        pressed.set(true);
        assert!(entry.poll(ms).is_none());
    }
    pressed.set(false);
    assert!(entry.poll(30).is_none());
    assert!(entry.poll(40).is_none());
    assert!(entry.poll(50).is_none());
    for ms in [60, 70, 80] {
        pressed.set(true);
        assert!(entry.poll(ms).is_none());
    }
    pressed.set(false);
    assert!(entry.poll(90).is_none());
    assert!(entry.poll(100).is_none());
    assert!(entry.poll(110).is_none());
    assert_eq!(
        entry.poll(511),
        Some(InputEvent::Button(ButtonEvent::DoubleClick))
    );
}

#[test]
fn a_board_button_carries_the_interaction_wiring() {
    let pressed: &'static Pressed = Box::leak(Box::new(Pressed::new(false)));
    let entry = PollEntry::button(4, FakeButton { pressed });
    assert_eq!(entry.source_id(), 4);
    assert_eq!(entry.key(), DemandKey::INTERACTION);
    assert_eq!(entry.cadence_ms(), BUTTON_SCAN_MS);
    assert_eq!(entry.board_cadence_ms(), BUTTON_SCAN_MS);
    assert_eq!(entry.next_at_ms(), BUTTON_SCAN_MS);
    assert_eq!(entry.name(), "button");
}
