//! The panel's own configuration path, replayed by a build that has no finger.
//!
//! Everything a development machine *cannot* reach is on the glass: the row a tap picks, the key a tap
//! types, and the tap that hands a passphrase over. A bench build — one named with `IOT_PANEL_BENCH` —
//! therefore types, at boot, the passphrase that same build was given (`IOT_WIFI_PSK`), at the cells
//! [`iot_core::keyboard`] draws it in, through the same ring the touch driver writes to.
//!
//! The taps carry the *script's* clock ([`iot_core::keyboard::Tap::at_ms`]) rather than a wall clock,
//! and one is published per pass of the render loop: a passphrase costs a few seconds of bench time
//! instead of the minute a finger's own timing needs. The render loop drains what has been published
//! before it steps this, so each tap is seen before the next is decided.
//!
//! Compiled out of every other build: [`BENCH`](crate::panel::BENCH) is a `const`, so the call that steps this disappears
//! along with everything it would reach. What it walks after the association is the page's *own*
//! controls — the row under a link, the action strip, the question a confirmation asks, and the row a
//! finger holds to forget a saved network: the paths that end on planes only a task can see.

use alloc::vec::Vec;

use iot_core::drivers::input::{FINGER_LONG_PRESS, GestureEvent, InputEvent, SwipeDirection};
use iot_core::drivers::network::{ApList, LinkPhase, Ssid};
use iot_core::intent::{Intent, recognize};
use iot_core::keyboard::{self, DialogChoice, Key, LIST_ROWS};
use iot_core::page::Part;
use iot_core::state::{Confirm, DeviceManager, DeviceState, DisplayPage};

use crate::input::IntentBus;

/// Whether this build replays a credential through the panel rather than being handed one.
///
/// The other half of this is in [`crate::network`]: a bench build's credentials are typed rather
/// than compiled in, so the product has to come up unconfigured for the keyboard to be the thing
/// that names a network.
pub const BENCH: bool = option_env!("IOT_PANEL_BENCH").is_some();

/// The column a list tap lands in.
///
/// A band answers for its whole width, and the row it holds is read from the tap's row alone — so
/// the bench aims at the panel's left edge rather than at the name, which is where a finger
/// aiming at a row goes anyway.
const BAND_X: u16 = 4;

/// One thing the bench does to the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    /// A tap at a point.
    At { x: u16, y: u16, at_ms: u32 },
    /// A finger that stays where it landed, which is the page's second verb for a row.
    Hold { x: u16, y: u16 },
    /// A slide up the list, which is the gesture that shows the next row of it.
    NextPage,
}

/// One act of the page's own controls, walked once the bench's credential is on the air.
///
/// The things a finger would do to the strip, to a confirmation and to a row it holds, in the order
/// that leaves each one something to read against the next. The taps go through the same ring a
/// finger's do and end on planes no test on the development machine can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// Tap a row while the link is up, which must pick nothing: under a link the row is a reading
    /// and the strip is the only way to take it down.
    TapRowUnderLink,
    /// Tap the action strip, which with a link up asks about leaving it.
    AskToLeave,
    /// Answer that question with `KEEP`: the link stays up and nothing was taken down.
    KeepTheLink,
    /// Tap the strip again, which asks about leaving once more.
    LeaveAsk,
    /// Answer that question with the action, which takes the link down.
    LeaveGo,
    /// Tap the row the link was just taken off — which is still the picked one, because what a link
    /// that came up dropped was the draft and not the row the draft was for. What this act is for is
    /// the arm where a keyboard rises over a draft that is not there: the keys are drawn *from* the
    /// draft and the strip is hidden with them, so that state draws nothing below the header and has
    /// nothing left to reach.
    ///
    /// Which is the arm the walk reaches once the forget above has taken the row's mark off: a row
    /// this device no longer has a passphrase for is a row to type for again.
    ReopenTheRowJustLeft,
    /// Tap the row the link was just taken off, which this device *does* hold the passphrase for: the
    /// link comes back up from the copy on the part, with no keyboard and nothing typed.
    ///
    /// The passphrase never goes near the panel for a row the store can answer for, so what this act
    /// reads back is that the page asked nothing — the strip under the list and no keys on it — and
    /// that the row it tapped is still the marked one that made it that kind of row.
    RejoinTheRowJustLeft,
    /// Tap the keyboard's own `CANCEL`, which is how the act above gets off the entry and leaves the
    /// list in front of the walk's remaining acts.
    DropTheKeyboard,
    /// Hold the row this device has saved, which asks about forgetting it.
    AskToForget,
    /// Answer that question with the action: the saved mark goes.
    Forget,
    /// Tap the strip with no link up, which asks the radio to look around.
    Rescan,
    /// Everything the page's own controls do has been walked.
    Done,
}

/// How many passes an act may take before the bench calls it a failure.
///
/// Generous: the acts that wait on the radio are a disconnect and a store write, and the acts that
/// wait on nothing but the page take one pass. What a bench must not do is wait for ever — a walk
/// that stopped without saying so would read as one that never ran.
const ACT_PASSES: u16 = 120;

/// The bench's own hand.
pub struct PanelBench {
    /// Every tap of the run, in order, or `None` until the list says where the network is.
    script: Option<Vec<Touch>>,
    next: usize,
    /// Set once the radio has answered, so the report is made once.
    answered: bool,
    /// What the draft held when the key that hands it over was pressed.
    typed: Option<usize>,
    /// Set once a scan has come back without the network this build named, so that is said once
    /// rather than once a round — a bench whose row is missing would otherwise say nothing at all,
    /// and nothing at all is the one reading that cannot be told from a bench that never started.
    missing_reported: bool,
    /// Set once a build given neither a network nor a passphrase has said so, for the same reason.
    absent_reported: bool,
    /// Set once the keys have been read as the page: the strip is not drawn with them. Latched
    /// because a draft lasts hundreds of passes and the reading is the same one every time.
    typed_parts_read: bool,
    /// Set once an attempt in flight has been read as a box over the list.
    joining_read: bool,
    /// Set once a keyboard has been read as standing over no draft at all.
    ///
    /// The one reading the typing reading above cannot make: that one asks where the page's parts
    /// are while a finger types, and a keyboard with nothing under it answers it the way a healthy
    /// one does. Latched for the same reason, and reported as an error because it is a page with
    /// nothing to draw rather than a page drawn differently.
    draftless_read: bool,
    /// Whether the saved row has taken the link back up yet, which is what tells the second leave
    /// from the first: the strip is the way off a link, and the walk leaves twice — once off the
    /// build's own network and once off the one the saved row brought back.
    rejoined: bool,
    /// The act of the walk now in progress.
    act: Act,
    /// Whether that act's own touch is already out.
    sent: bool,
    /// What the row-under-a-link act's read-back is against: whether a pick is showing, read before
    /// that act's own tap went out. The typing that came before already picked a row, so what is
    /// left in the page is not this act's to read — see the act itself.
    picked: Option<(bool, bool)>,
    /// How many passes the act has taken, which is what stops a walk that cannot finish.
    passes: u16,
}

impl PanelBench {
    pub const fn new() -> Self {
        Self {
            script: None,
            next: 0,
            answered: false,
            typed: None,
            missing_reported: false,
            absent_reported: false,
            typed_parts_read: false,
            joining_read: false,
            draftless_read: false,
            rejoined: false,
            act: Act::TapRowUnderLink,
            sent: false,
            picked: None,
            passes: 0,
        }
    }

    /// Advance the bench by at most one tap.
    ///
    /// Called once a pass, after the loop has drained the ring: a tap published here is applied by
    /// the next pass, and the one after that is decided against the state it produced.
    pub fn step(&mut self, manager: &DeviceManager, bus: &IntentBus) {
        if !BENCH {
            return;
        }
        let state = manager.state();
        self.read_parts(&state);
        if self.script.is_none() {
            // The name decides the row and the passphrase decides the keys, and a bench build is
            // given both at build time. One given neither has nothing to replay, and saying so is
            // the one reading that tells it from a bench that never ran.
            let Some((ssid, psk)) = ssid().zip(psk()) else {
                if state.page == DisplayPage::Network && state.aps.is_some() {
                    self.absent("this build was named no network and no passphrase to replay");
                }
                return;
            };
            let Some(script) = self.script(&state, ssid, psk) else {
                return;
            };
            self.script = Some(script);
        }
        let script = self.script.as_ref().expect("built just above");
        if let Some(touch) = script.get(self.next) {
            if self.next + 1 == script.len() {
                // The draft as it stands while the key that hands it over is pressed. Read *here*
                // because it is the last pass that can: the state drops the draft the moment the
                // radio has the network, and what the keyboard typed would be gone by the time the
                // radio answered.
                self.typed = Some(state.setup.entry.map_or(0, |entry| entry.len()));
            }
            self.publish(bus, *touch);
            self.next += 1;
            return;
        }
        if self.answered {
            // The credential is on the air, so the bench's next acts are the page's own controls:
            // the strip, the questions it asks, and the row a finger holds. Nothing but a bench can
            // reach those paths, which is why they are walked here.
            self.walk(manager, bus);
            return;
        }
        // Every tap is out, so the radio's answer is the bench's result.
        match state.link.phase {
            LinkPhase::Connected => {
                // The passphrase itself is never logged; its length is what says the multi-tap, the
                // shift and the layers came out right. The radio associating on it is what says the
                // characters themselves were right — nothing checks the draft against the passphrase
                // beyond this, because a passphrase that reaches the network task is the one the
                // keyboard typed.
                let typed = self.typed.unwrap_or(0);
                let wanted = psk().map_or(0, |psk| psk.len());
                self.answered = true;
                if typed == wanted {
                    log::info!(
                        "[BENCH] the panel's own credential is on the air: {typed} characters typed, \
                         {} taps, and the radio has an address",
                        script.len()
                    );
                } else {
                    log::error!(
                        "[BENCH] the radio is up, but the draft was {typed} characters of {wanted} \
                         when OK was pressed: the radio accepted something the keyboard did not type"
                    );
                }
            }
            LinkPhase::Failed(kind) => {
                self.answered = true;
                log::error!(
                    "[BENCH] the radio refused what the panel typed: {kind:?} after {} taps",
                    script.len()
                );
            }
            // The radio has not been asked yet, or is being asked now.
            LinkPhase::Off | LinkPhase::Scanning | LinkPhase::Connecting => {}
        }
    }

    /// Read the page's own parts off the state, without a finger on the panel.
    ///
    /// What these ask is what the page draws, and the page's parts are data — [`Part::showing`] is the
    /// same answer the renderer walks, so a reading here is the drawing without the pixels.
    ///
    /// Two of them are about a part that must *not* be there, and both are the panel's own defects
    /// rather than the radio's: the **strip while a finger is typing** (the keys and the strip are the
    /// same band of the panel, so a strip drawn with the keys stamps its own word across the row that
    /// ends an entry) and the **list while the radio is joining** (what is over it then is the box that
    /// says the attempt is in flight, because an association can take half a minute). The strip stays
    /// offered under that box: the box asks nothing, and `LEAVE` is the page's one way off the attempt.
    ///
    /// One more is about a keyboard standing over no draft at all: the keys are drawn *from* the draft,
    /// so with none they are nothing — and with the strip hidden they are all the page has below its
    /// header. That state has been on the glass, and this log said nothing about it.
    ///
    /// Latched, one reading per state: an entry lasts hundreds of passes and an attempt thousands, and
    /// every one of them would say the same thing.
    fn read_parts(&mut self, state: &DeviceState) {
        if state.page != DisplayPage::Network {
            return;
        }
        let showing = |part: Part| part.showing(state.setup, state.link);
        if state.setup.keyboard && state.setup.entry.is_none() && !self.draftless_read {
            self.draftless_read = true;
            log::error!(
                "[BENCH] the keyboard is up with no draft under it: the keys are drawn from the \
                 draft and the strip is hidden with them, so the page has nothing to put below its \
                 header and no control left on it"
            );
        }
        if state.setup.keyboard && !self.typed_parts_read {
            self.typed_parts_read = true;
            if showing(Part::Content) && !showing(Part::Controls) && !showing(Part::Overlay) {
                log::info!("[BENCH] a finger is typing, and the strip is not drawn with the keys");
            } else {
                log::error!(
                    "[BENCH] the page under the keyboard is drawn wrong: content {}, strip {}, \
                     box {}",
                    showing(Part::Content),
                    showing(Part::Controls),
                    showing(Part::Overlay),
                );
            }
        }
        if state.link.phase == LinkPhase::Connecting && !self.joining_read {
            self.joining_read = true;
            if showing(Part::Overlay) && !showing(Part::Content) && showing(Part::Controls) {
                log::info!(
                    "[BENCH] an attempt is in flight: the box is over the list and the strip is \
                     still offered"
                );
            } else {
                log::error!(
                    "[BENCH] the radio is joining and the page is not saying so: box {}, list \
                     hidden {}, strip {}",
                    showing(Part::Overlay),
                    !showing(Part::Content),
                    showing(Part::Controls),
                );
            }
        }
    }

    /// The whole run, or `None` while the page has no list to pick from yet.
    ///
    /// The two arguments are what the build was named: the network decides which row is picked and
    /// the passphrase decides which cells are tapped. Taken as arguments rather than read here so a
    /// host test can hand them in — a build compiled without them is the one case that cannot.
    fn script(&mut self, state: &DeviceState, ssid: &str, psk: &str) -> Option<Vec<Touch>> {
        // Nothing to type at until the page the keyboard is on is showing, and nothing to pick
        // while an entry is already open over it.
        if state.page != DisplayPage::Network || state.setup.keyboard {
            return None;
        }
        // Not scanned yet: the survey is a few hundred milliseconds behind boot, and a bench that
        // reported the network missing before the first round came back would be reporting its own
        // impatience.
        let list = state.aps?;
        let wanted = Ssid::from_octets(ssid.as_bytes());
        let Some(index) = list.iter().position(|ap| ap.ssid == wanted) else {
            // Said once and at warn, not error: the first round often lists a neighbour and not
            // the network this bench wants (the board has done exactly that), and a run that starts
            // on the second round is a run that worked.
            if !list.is_empty() && !self.missing_reported {
                self.missing_reported = true;
                log::warn!(
                    "[BENCH] the network this build was named is not among the {} listed yet; \
                     a later round may carry it",
                    list.len()
                );
            }
            return None;
        };
        if !list.get(index).is_some_and(|ap| ap.secured) {
            log::error!(
                "[BENCH] the network this build was named asks for no passphrase, so the panel path \
                 this bench exists for is not being exercised"
            );
            self.answered = true;
            return None;
        }
        let script = keyboard::script_for(psk.as_bytes())?;
        let mut touches = Vec::with_capacity(script.len() + 2);
        // Rows of the list above the one the network is on, and the band the row is on once the
        // window has been slid there. The window moves a row a slide, so the bench reads the list
        // the way a finger does — including that the tap that picks the row is read against the top
        // the slides left behind, which is the arithmetic that says which row a tap lands on.
        let slides = index
            .saturating_sub(LIST_ROWS - 1)
            .min(list.len().saturating_sub(LIST_ROWS));
        let band = index - slides;
        for _ in 0..slides {
            touches.push(Touch::NextPage);
        }
        touches.push(Touch::At {
            x: BAND_X,
            y: band_center(band),
            at_ms: 0,
        });
        for tap in &script {
            let rect = keyboard::key_box(tap.cell).expect("a script only taps cells the grid has");
            touches.push(Touch::At {
                x: (rect.x + rect.w / 2) as u16,
                y: (rect.y + rect.h / 2) as u16,
                at_ms: tap.at_ms,
            });
        }
        let ok = keyboard::key_box(keyboard::cell_of(Key::Connect).expect("the grid has connect"))
            .expect("a cell of the grid");
        touches.push(Touch::At {
            x: (ok.x + ok.w / 2) as u16,
            y: (ok.y + ok.h / 2) as u16,
            at_ms: script.last().map_or(0, |tap| tap.at_ms) + 100,
        });
        log::info!(
            "[BENCH] replaying a credential through the panel: {index} is band {band} after \
             {slides} slides, {} taps in all",
            touches.len()
        );
        Some(touches)
    }

    /// Walk the page's own controls, one act a pass, each read back against the state it produced.
    ///
    /// One act a pass for the reason the script is one tap a pass: what the page did with a touch is
    /// read from the state the *next* pass holds, and an act decided against a state two acts old
    /// would be a bench testing its own arithmetic rather than the panel's.
    ///
    /// Every act says what it saw, and the failure of one is logged rather than swallowed: the walk
    /// is the only reading these paths have, so an act that quietly did nothing would be a bench
    /// reporting a pass on a panel that never answered.
    fn walk(&mut self, manager: &DeviceManager, bus: &IntentBus) {
        let state = manager.state();
        self.passes = self.passes.saturating_add(1);
        match self.act {
            Act::TapRowUnderLink => {
                if !self.sent {
                    if !state.link.is_leavable() {
                        return self.unwalkable(&state, "no link is up");
                    }
                    log::info!("[BENCH] tapping band 0 under the link, which must pick nothing");
                    // Read before the tap: the typing that came before picked this row, and the
                    // row it picked is still in `selected` — a link going up clears the draft, not
                    // the row the draft was for. So the act reads its own tap against what was
                    // there, not the page's whole history.
                    self.picked = Some(a_pick_showing(&state));
                    return self.send(bus, row_point(0));
                }
                // The only arm with nothing to wait for: what it reads back is the *absence* of
                // what a pick does, and that absence is there on the pass after the tap is
                // applied.
                if self.picked != Some(a_pick_showing(&state)) {
                    log::error!(
                        "[BENCH] the row under the link took a pick: a link to leave is not a row \
                         to pick"
                    );
                    self.act = Act::Done;
                    return;
                }
                log::info!("[BENCH] the row under the link took no pick, and no keyboard came up");
                self.advance(Act::AskToLeave);
            }
            Act::AskToLeave => {
                if !self.sent {
                    if !state.link.is_leavable() {
                        return self.unwalkable(&state, "no link is up");
                    }
                    log::info!("[BENCH] tapping the strip, which asks to leave the link it is on");
                    return self.send(bus, strip_point());
                }
                if state.setup.confirm == Some(Confirm::Leave) {
                    log::info!("[BENCH] the strip asked about leaving, and nothing is down yet");
                    return self.advance(Act::KeepTheLink);
                }
                self.stop_if_stale(format_args!(
                    "the strip was tapped with a link up and asked nothing"
                ));
            }
            Act::KeepTheLink => {
                if !self.sent {
                    return self.send(bus, answer_point(DialogChoice::Keep));
                }
                if state.setup.confirm.is_none() {
                    if state.link.is_leavable() {
                        log::info!("[BENCH] KEEP left the question undone and the link up");
                    } else {
                        log::error!(
                            "[BENCH] KEEP took the link down, which is what the action is for"
                        );
                    }
                    return self.advance(Act::LeaveAsk);
                }
                self.stop_if_stale(format_args!(
                    "the box's own KEEP did not take the question away, which leaves it saying {:?}",
                    state.setup.confirm,
                ));
            }
            Act::LeaveAsk => {
                if !self.sent {
                    return self.send(bus, strip_point());
                }
                if state.setup.confirm == Some(Confirm::Leave) {
                    log::info!("[BENCH] the strip asked again; the action's own answer is next");
                    return self.advance(Act::LeaveGo);
                }
                self.stop_if_stale(format_args!(
                    "the strip was tapped again with a link up and asked nothing"
                ));
            }
            Act::LeaveGo => {
                if !self.sent {
                    return self.send(bus, answer_point(DialogChoice::Act));
                }
                // The radio answers a moment later: the link is down when the network task has
                // taken the request and dropped the association, and the page says so.
                if !state.link.is_leavable() && state.link.address.is_none() {
                    log::info!("[BENCH] the link is down and the page is back on the list");
                    // The first leave is off the build's network and is followed by the row the link
                    // was on — this device holds that passphrase. The second is off the link that row
                    // brought back and is followed by the row's own mark, which a link would hide: a
                    // hold with a link up asks nothing.
                    return self.advance(if self.rejoined {
                        Act::AskToForget
                    } else {
                        Act::RejoinTheRowJustLeft
                    });
                }
                self.stop_if_stale(format_args!(
                    "the action's own tap did not take the link down (it says {:?} with the \
                     address {:?})",
                    state.link.phase, state.link.address,
                ));
            }
            Act::RejoinTheRowJustLeft => {
                // Still the picked row, and still carrying the store's mark.
                let Some(band) = picked_band(&state) else {
                    return self
                        .unwalkable(&state, "the row the link was on is not on a band showing");
                };
                if !self.sent {
                    log::info!(
                        "[BENCH] tapping band {band}, the row the link was just taken off and this \
                         device has the passphrase for"
                    );
                    return self.send(bus, row_point(band));
                }
                if state.link.is_leavable() {
                    if state.setup.keyboard || state.setup.entry.is_some() {
                        log::error!(
                            "[BENCH] a row this device has the passphrase for opened the keyboard: \
                             the copy on the part is what such a row is joined with, and asking for \
                             one is asking the user to type a secret the device already has"
                        );
                        self.act = Act::Done;
                        return;
                    }
                    if saved_band(&state).is_none() {
                        log::error!(
                            "[BENCH] the row that brought the link back is not one this device has \
                             saved, so the copy it joined with came from nowhere"
                        );
                        self.act = Act::Done;
                        return;
                    }
                    log::info!(
                        "[BENCH] the saved row brought the link back up with the copy on the part, \
                         and the page asked for nothing"
                    );
                    self.rejoined = true;
                    // Taken down again: the row's mark is what the walk still has to read, and a link
                    // covers it.
                    return self.advance(Act::AskToLeave);
                }
                self.stop_if_stale(format_args!(
                    "tapping the saved row did not bring the link up (it says {:?} with the address \
                     {:?})",
                    state.link.phase, state.link.address,
                ));
            }
            Act::ReopenTheRowJustLeft => {
                // Still the picked row — a link drops the draft, not the row it was for — and what
                // this act is for is the entry that opens over it.
                let Some(band) = picked_band(&state) else {
                    return self
                        .unwalkable(&state, "the row the link was on is not on a band showing");
                };
                if !self.sent {
                    log::info!("[BENCH] tapping band {band}, the row the link was just taken off");
                    return self.send(bus, row_point(band));
                }
                match state.setup.entry {
                    None if state.setup.keyboard => {
                        log::error!(
                            "[BENCH] the row the link was left from opened the keyboard over no \
                             draft: the page drew nothing below its header"
                        );
                        self.act = Act::Done;
                    }
                    None => self.stop_if_stale(format_args!(
                        "tapping the row the link was left from opened nothing"
                    )),
                    Some(entry) => {
                        log::info!(
                            "[BENCH] the row the link was left from opens a fresh entry: the \
                             keyboard is up over {} characters, and the draft the link dropped is \
                             not back",
                            entry.len(),
                        );
                        self.advance(Act::DropTheKeyboard);
                    }
                }
            }
            Act::DropTheKeyboard => {
                if !self.sent {
                    return self.send(bus, cancel_point());
                }
                // `CANCEL` is the entry's own way out, and it takes the row with it: the page is back
                // on the list rather than on a row still picked with nothing over it.
                if !state.setup.keyboard && state.setup.selected.is_none() {
                    log::info!("[BENCH] the entry's own cancel puts the page back on the list");
                    return self.advance(Act::Rescan);
                }
                self.stop_if_stale(format_args!(
                    "the keyboard's cancel left the entry where it was: keyboard up {}, row \
                     picked {}",
                    state.setup.keyboard,
                    state.setup.selected.is_some(),
                ));
            }
            Act::AskToForget => {
                let Some(band) = saved_band(&state) else {
                    return self.unwalkable(&state, "no saved row is on one of the bands showing");
                };
                if !self.sent {
                    log::info!("[BENCH] holding band {band}, which asks to forget that network");
                    return self.send(bus, hold_point(band));
                }
                if let Some(Confirm::Forget(ap)) = state.setup.confirm {
                    log::info!(
                        "[BENCH] the hold asked to forget {:?}",
                        ap.ssid.as_display_text().unwrap_or("<unprintable>"),
                    );
                    return self.advance(Act::Forget);
                }
                self.stop_if_stale(format_args!(
                    "the hold asked nothing, which leaves the question {:?}",
                    state.setup.confirm,
                ));
            }
            Act::Forget => {
                if !self.sent {
                    return self.send(bus, answer_point(DialogChoice::Act));
                }
                // The store's own answer, which the network task publishes: the row stops being
                // marked once the erase is written, and not before.
                if saved_band(&state).is_none() && state.setup.confirm.is_none() {
                    log::info!("[BENCH] the saved mark is gone from the row the finger held");
                    // What the mark was hiding is the row itself: it is one to type for again — the
                    // next act's reading.
                    return self.advance(Act::ReopenTheRowJustLeft);
                }
                self.stop_if_stale(format_args!(
                    "the action's own tap left the row the finger held marked ({:#010b} over {} \
                     rows) with the question {:?}",
                    marks(&state),
                    state.aps.map_or(0, |list| list.len()),
                    state.setup.confirm,
                ));
            }
            Act::Rescan => {
                if !self.sent {
                    log::info!("[BENCH] tapping the strip with no link, which asks to look around");
                    return self.send(bus, strip_point());
                }
                // The survey's own round is what clears the wait, and it is the round the strip
                // asked for rather than one that happened to be due: the wait is only ever set by
                // this tap.
                if !state.setup.rescan_wait {
                    log::info!("[BENCH] the strip's own round came back");
                    return self.advance(Act::Done);
                }
                self.stop_if_stale(format_args!(
                    "the strip asked to look around and no round came back"
                ));
            }
            Act::Done => {}
        }
    }

    /// Say what the walk cannot do here, and end it: every act after this one is decided against a
    /// state this one needed.
    fn unwalkable(&mut self, state: &DeviceState, why: &str) {
        log::error!(
            "[BENCH] the page's own controls cannot be walked: {why} (phase {:?})",
            state.link.phase
        );
        self.act = Act::Done;
    }

    /// Say that an act's own touch went out, and start counting the passes it has to land in.
    fn send(&mut self, bus: &IntentBus, touch: Touch) {
        self.sent = true;
        self.passes = 0;
        self.publish(bus, touch);
    }

    /// Move to the next act, with its own touch not yet out.
    fn advance(&mut self, act: Act) {
        self.act = act;
        self.sent = false;
        self.passes = 0;
    }

    /// Stop the walk if the act in progress has run out of passes, saying why once, with what the act
    /// read.
    ///
    /// Every arm ends with this rather than a bare `return`: an act that has run out is a page that is
    /// not answering rather than a bench that is slow, and the one thing a bench must not do is wait
    /// for ever. The reading is part of the message because most acts watch more than one field.
    fn stop_if_stale(&mut self, why: core::fmt::Arguments<'_>) {
        if self.passes < ACT_PASSES {
            return;
        }
        log::error!("[BENCH] {why} in {ACT_PASSES} passes; the walk stops here");
        self.act = Act::Done;
    }

    /// Says, once, why no run was built.
    ///
    /// A bench that reports nothing is a bench that cannot be told from one that never started, so
    /// every way of never starting says which one it was.
    fn absent(&mut self, why: &str) {
        if self.absent_reported {
            return;
        }
        self.absent_reported = true;
        log::error!("[BENCH] {why}");
    }

    fn publish(&self, bus: &IntentBus, touch: Touch) {
        let event = match touch {
            Touch::At { x, y, at_ms } => InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms,
            }),
            // A hold is the classified gesture itself rather than a contact that dwelled: the
            // classifier is the input task's, and what the page reads is the gesture it produces.
            Touch::Hold { x, y } => InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x,
                y,
                held_ms: u16::from(FINGER_LONG_PRESS) + 100,
            }),
            Touch::NextPage => InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Up,
                x: BAND_X + 10,
                y: band_center(LIST_ROWS - 1),
                end_x: BAND_X + 10,
                end_y: 0,
                held_ms: 40,
                distance_px: band_center(LIST_ROWS - 1),
            }),
        };
        // Source 0 is the panel's own gestures, which is what a touch on the glass is: the same
        // operation the input task builds from a real one.
        let _ = bus.try_send(Intent::Operation(recognize(event, 0)));
    }
}

/// The middle of the page's action strip, where a finger aiming at it lands.
fn strip_point() -> Touch {
    Touch::At {
        x: BAND_X,
        y: (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
        at_ms: 0,
    }
}

/// The middle of one of a confirmation's two answers, read off the geometry the panel draws it at.
fn answer_point(choice: DialogChoice) -> Touch {
    let rect = keyboard::dialog_answer_box(choice);
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

/// The middle of the band `band` is drawn on, tapped rather than held.
fn row_point(band: usize) -> Touch {
    Touch::At {
        x: BAND_X,
        y: band_center(band),
        at_ms: 0,
    }
}

/// The middle of the band `band` is drawn on, held rather than tapped.
fn hold_point(band: usize) -> Touch {
    Touch::Hold {
        x: BAND_X,
        y: band_center(band),
    }
}

/// What a pick leaves behind, for the act that has to tell its own tap from one already there.
///
/// The two fields a pick sets: the keyboard it opens and the row it names. Read as a pair because
/// either alone is a state a page can be in without a pick — the draft's own row survives the link
/// going up, and the keyboard is down for a row that is not secured.
fn a_pick_showing(state: &DeviceState) -> (bool, bool) {
    (state.setup.keyboard, state.setup.selected.is_some())
}

/// The band one of the page's showing rows answers `matches` on, if one does.
///
/// The walk's own lookup, and the same shape as the script's: the page's window is what a finger sees,
/// and a row it has scrolled past is one the bench cannot reach either.
fn band_where(state: &DeviceState, matches: impl Fn(&ApList, usize) -> bool) -> Option<usize> {
    let list = state.aps?;
    let top = usize::from(state.setup.list_top);
    (top..top + LIST_ROWS)
        .find(|index| matches(&list, *index))
        .map(|index| index - top)
}

/// The band a row this device has saved is showing on.
fn saved_band(state: &DeviceState) -> Option<usize> {
    band_where(state, |list, index| list.is_saved(index))
}

/// The marks the page's list is carrying, one bit per row, for the acts that read one back.
fn marks(state: &DeviceState) -> u8 {
    let Some(list) = state.aps else {
        return 0;
    };
    (0..list.len()).fold(0, |marks, index| {
        marks | u8::from(list.is_saved(index)) << index
    })
}

/// The band the page's own picked row is showing on, if the window still holds it.
///
/// Read off the row rather than remembered from the script, because the pick outlives the link: the
/// page's own answer for whether a row is that one is the network — the survey rewrites every row a
/// second later, so a name that matched on the reading would stop matching on the next one.
fn picked_band(state: &DeviceState) -> Option<usize> {
    let picked = state.setup.selected?;
    band_where(state, |list, index| {
        list.get(index)
            .is_some_and(|ap| ap.is_same_network(&picked))
    })
}

/// The middle of the keyboard's own `CANCEL`, read off the geometry the panel draws it at.
fn cancel_point() -> Touch {
    let cell = keyboard::cell_of(Key::Cancel).expect("the grid holds cancel");
    let rect = keyboard::key_box(cell).expect("a cell of the grid");
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

impl Default for PanelBench {
    fn default() -> Self {
        Self::new()
    }
}

/// The middle of the list's `band`-th visible row, in panel pixels.
fn band_center(band: usize) -> u16 {
    (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16
}

/// The network this build was named, if it was named one.
fn ssid() -> Option<&'static str> {
    option_env!("IOT_WIFI_SSID")
}

/// The passphrase this build was named, if it was named one.
fn psk() -> Option<&'static str> {
    option_env!("IOT_WIFI_PSK")
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::network::{
        AccessPoint, ApList, Credentials, LinkPhase, LinkStatus, SavedNetworks,
    };
    use iot_core::intent::{BusinessIntent, translate};
    use iot_core::keyboard::Layer;

    /// A listed row: the name is what the bench looks up, so the rest is whatever makes the row a
    /// row.
    fn ap(name: &str) -> AccessPoint {
        AccessPoint {
            ssid: Ssid::from_octets(name.as_bytes()),
            bssid: [7; 6],
            channel: 11,
            rssi: -50,
            secured: true,
        }
    }

    /// A list of these rows, marked as a device that holds `saved` would mark it: the store's own
    /// answer for the rows in front of it, which is what the network task publishes.
    fn marked(names: &[&str], saved: &[&str]) -> ApList {
        let rows: Vec<AccessPoint> = names.iter().map(|name| ap(name)).collect();
        let mut list = ApList::from_ranked(&rows);
        let mut store = SavedNetworks::empty();
        for name in saved {
            store.remember(&Credentials::from_text(name, "ab").expect("a credential"));
        }
        list.mark_saved(store.mask(&list));
        list
    }

    /// A page showing `names`, which is what the bench waits for.
    fn showing(names: &[&str]) -> DeviceState {
        let rows: Vec<AccessPoint> = names.iter().map(|name| ap(name)).collect();
        DeviceState {
            page: DisplayPage::Network,
            aps: Some(ApList::from_ranked(&rows)),
            ..DeviceState::default()
        }
    }

    /// A tap at the middle of the cell a control sits in, as the bench aims at it.
    fn key_center(key: Key) -> Touch {
        let cell = keyboard::cell_of(key).expect("the grid holds every control");
        let rect = keyboard::key_box(cell).expect("a cell of the grid");
        Touch::At {
            x: (rect.x + rect.w / 2) as u16,
            y: (rect.y + rect.h / 2) as u16,
            at_ms: 0,
        }
    }

    /// The point a tap lands on.
    fn point(touch: Touch) -> (u16, u16) {
        match touch {
            Touch::At { x, y, .. } => (x, y),
            Touch::NextPage => panic!("a slide is not a point"),
            Touch::Hold { .. } => panic!("a hold is not a point"),
        }
    }

    #[test]
    fn a_run_taps_the_row_the_named_network_is_on() {
        let state = showing(&["aaa", "bbb", "ccc", "target", "eee"]);
        let mut bench = PanelBench::new();
        let script = bench.script(&state, "target", "ab").expect("a run");

        // The fourth row through a window of three: one slide brings it into view as the last band,
        // and the tap after the slide is read against the top that slide left.
        assert_eq!(script[0], Touch::NextPage);
        assert_eq!(
            script[1],
            Touch::At {
                x: BAND_X,
                y: band_center(2),
                at_ms: 0
            }
        );
        assert_eq!(keyboard::list_band_at(band_center(2)), Some(2));
    }

    #[test]
    fn every_row_a_scan_can_carry_is_reachable_by_the_bench() {
        // The window shows three rows of up to eight and the slide moves one at a time, so a row is
        // reachable from *some* top or the list has rows a finger cannot pick at all. The strongest
        // rows first, which is the order the scan hands over: the eighth row is the one that only
        // the last window — the one aligned to the list's own end — can show.
        let names = [
            "net0", "net1", "net2", "net3", "net4", "net5", "net6", "net7",
        ];
        let rows: Vec<AccessPoint> = names
            .iter()
            .enumerate()
            .map(|(index, name)| AccessPoint {
                ssid: Ssid::from_octets(name.as_bytes()),
                rssi: -50 - index as i8,
                ..ap("taken")
            })
            .collect();
        let state = DeviceState {
            page: DisplayPage::Network,
            aps: Some(ApList::from_ranked(&rows)),
            ..DeviceState::default()
        };

        for (index, name) in names.iter().enumerate() {
            let mut bench = PanelBench::new();
            let script = bench
                .script(&state, name, "ab")
                .unwrap_or_else(|| panic!("row {index} has no run"));
            let slides = script
                .iter()
                .take_while(|touch| matches!(touch, Touch::NextPage))
                .count();
            let (_, y) = point(script[slides]);
            assert_eq!(
                keyboard::list_band_at(y),
                Some(index - slides),
                "row {index} is the band its tap lands on after {slides} slides"
            );
        }
    }

    #[test]
    fn the_row_the_page_keeps_picked_is_the_band_its_own_tap_lands_on() {
        // The act that walks back onto the row the link was taken off reads its band the way the
        // script does, but off the *page*: what the page is holding is the row, and the survey
        // rewrites every row a second later, so the row a name finds now is not the row a reading of
        // it would have found then. Each row here is its own network for that reason.
        let rows: Vec<AccessPoint> = ["aaa", "bbb", "target", "ddd", "eee"]
            .iter()
            .enumerate()
            .map(|(index, name)| AccessPoint {
                bssid: [7, 0, 0, 0, 0, index as u8 + 1],
                ..ap(name)
            })
            .collect();
        let mut state = DeviceState {
            page: DisplayPage::Network,
            aps: Some(ApList::from_ranked(&rows)),
            ..DeviceState::default()
        };
        assert_eq!(picked_band(&state), None, "nothing is picked yet");

        state.setup.selected = Some(rows[2]);
        assert_eq!(picked_band(&state), Some(2), "the row is the band it is on");

        state.setup.list_top = 2;
        assert_eq!(picked_band(&state), Some(0), "with the window slid onto it");

        state.setup.selected = Some(rows[0]);
        assert_eq!(
            picked_band(&state),
            None,
            "and a row the window has scrolled past is one the bench cannot tap either"
        );
    }

    #[test]
    fn the_walk_asks_before_it_leaves_forgets_and_looks_around() {
        // The bench's own acts, driven the way the render loop drives a finger's taps: every touch
        // it publishes goes through the same `translate` and the same state, and the two things the
        // page asks of the radio are answered here as the network task would answer them. What this
        // proves is that each act's read-back is a claim the page really makes — a walk whose acts
        // were decided against the wrong state would end somewhere other than the end of the list.
        let mut manager = DeviceManager::new().with_network(true);
        // The page the bench's own script leaves: nothing is saved yet, so the row is one to type
        // for; the passphrase goes in, and the store's marks follow the association it won — which is
        // the order the board runs in, and the reason the row under the link is a saved one.
        manager.apply_scan(marked(&["target"], &[]));
        manager.apply_business(BusinessIntent::TogglePage);
        manager.apply_business(BusinessIntent::Row(0));
        assert!(
            manager.state().setup.keyboard,
            "the pick is there to begin with"
        );
        manager.apply_scan(marked(&["target"], &["target"]));
        manager.apply_business(BusinessIntent::Link(LinkStatus {
            phase: LinkPhase::Connected,
            ap: Some(ap("target")),
            address: None,
        }));
        assert!(
            manager.state().setup.selected.is_some() && !manager.state().setup.keyboard,
            "the link cleared the draft and kept the row it was for"
        );

        let bus = IntentBus::new();
        let mut bench = PanelBench::new();
        bench.answered = true;
        // One counter each for the commands that must not happen twice — what an act's own reading
        // cannot catch: a KEEP that took the link down would be a second leave, and a second survey
        // request or join is one act that ran twice.
        let (mut leaves, mut forgets, mut rounds, mut joins) = (0, 0, 0, 0);
        // And one for the act that walks back onto the row the link was just taken off: the entry it
        // reads is the one the panel showed as a black page when there was nothing under the keys.
        let mut reopened = 0;
        // Every act the walk was in, in the order it was in them: the counters below say what the
        // page *did*, and this says which questions were asked to make it do it — a walk that skipped
        // an act, or answered one out of order, ends on a page the counters cannot tell from the right
        // one.
        let mut visited: Vec<Act> = Vec::new();
        for _ in 0..ACT_PASSES * 6 {
            bench.walk(&manager, &bus);
            if visited.last() != Some(&bench.act) {
                visited.push(bench.act);
            }
            while let Ok(intent) = bus.try_receive() {
                match intent {
                    Intent::Operation(op) => {
                        manager.apply_operation(op);
                        let business = translate(&op, &manager.state());
                        manager.apply_business(business);
                    }
                    Intent::Business(business) => manager.apply_business(business),
                }
            }
            // The network task's own answers, as the render loop hands them over: a leave takes the
            // link down and reports it, a forget is the store's own erase, a round of the survey
            // publishes the phase the strip's wait ends on — and a row asked for by name is joined
            // with the copy the store holds for it, which is the association the radio reports.
            if manager.take_leave() {
                leaves += 1;
                manager.apply_business(BusinessIntent::Link(LinkStatus::default()));
            }
            if manager.take_forget().is_some() {
                forgets += 1;
                // The store's own erase, as the network task answers it: the same rows, marked
                // again against a store that no longer holds the name.
                manager.apply_scan(marked(&["target"], &[]));
            }
            if let Some(iot_core::drivers::network::JoinRequest::Held(ssid)) = manager.take_join() {
                joins += 1;
                // The copy is on the part, so the link goes after it: what the radio reports next is
                // the association, with the name it is on and no address yet.
                assert_eq!(
                    ssid,
                    Ssid::from_octets(b"target"),
                    "the row the walk tapped"
                );
                manager.apply_business(BusinessIntent::Link(LinkStatus {
                    phase: LinkPhase::Connecting,
                    ap: Some(ap("target")),
                    address: None,
                }));
            }
            if manager.take_scan_now() {
                rounds += 1;
                manager.apply_business(BusinessIntent::Link(LinkStatus::default()));
            }
            if bench.act == Act::ReopenTheRowJustLeft && manager.state().setup.keyboard {
                assert!(
                    manager.state().setup.entry.is_some(),
                    "the keyboard came up over no draft, which is a page with nothing on it"
                );
                reopened += 1;
            }
            if bench.act == Act::Done {
                break;
            }
            // No row takes a pick while a link is up: the first act taps one and the gate refuses it.
            // The acts after the link is down are the ones that open the keyboard, which is why this
            // is gated on the link rather than asserted flat.
            assert!(
                !manager.state().link.is_leavable() || !manager.state().setup.keyboard,
                "a row took a pick while a link was up"
            );
        }

        assert_eq!(bench.act, Act::Done, "the walk reached the end of the page");
        assert_eq!(
            visited,
            alloc::vec![
                Act::TapRowUnderLink,
                Act::AskToLeave,
                Act::KeepTheLink,
                Act::LeaveAsk,
                Act::LeaveGo,
                // The row the link was just taken off, joined with the copy on the part and nothing
                // asked for — and then left again, because the row's mark is under the box the link
                // draws over the list and a hold can only reach the row the page is showing.
                Act::RejoinTheRowJustLeft,
                Act::AskToLeave,
                Act::KeepTheLink,
                Act::LeaveAsk,
                Act::LeaveGo,
                Act::AskToForget,
                Act::Forget,
                // The mark is gone, so the same row is a row to type for: the keyboard comes up over
                // a fresh entry, which is cancelled and leaves the strip's own verb to be read.
                Act::ReopenTheRowJustLeft,
                Act::DropTheKeyboard,
                Act::Rescan,
                Act::Done,
            ],
            "the acts the walk was in, in the order it was in them"
        );
        assert_eq!(
            leaves, 2,
            "KEEP left the link up and the action took it down, twice: the build's own network and \
             the one the saved row brought back"
        );
        assert_eq!(joins, 1, "the saved row was asked for by name, and once");
        assert_eq!(forgets, 1, "the hold's own answer is the one forget");
        assert_eq!(rounds, 1, "and the strip's other verb asked for one round");
        assert!(
            reopened > 0,
            "the row the link was left from never opened an entry for the walk to read"
        );
        let state = manager.state();
        assert!(!state.link.is_leavable(), "the link is down");
        assert!(
            !state.aps.expect("the list is still up").is_saved(0),
            "and the saved mark is gone"
        );
        assert_eq!(state.setup.confirm, None, "with no question left open");
        assert!(!state.setup.rescan_wait, "and the strip is not waiting");
    }

    #[test]
    fn a_run_ends_on_the_key_that_hands_the_passphrase_over() {
        let state = showing(&["target"]);
        let mut bench = PanelBench::new();
        let script = bench.script(&state, "target", "ab").expect("a run");

        let last = script.last().copied().expect("a run has taps");
        assert_eq!(point(last), point(key_center(Key::Connect)));
        assert_eq!(
            keyboard::key_at(Layer::Abc, point(last).0, point(last).1),
            Some(Key::Connect),
            "the last tap is the key that hands the credential over, not a cell beside it"
        );
        // And every earlier tap is on a key rather than on a seam: the bench is a finger at the
        // panel's coordinates, and a tap between two cells would be a tap on neither.
        for tap in &script[..script.len() - 1] {
            let (x, y) = point(*tap);
            assert!(
                keyboard::key_at(Layer::Abc, x, y).is_some(),
                "the tap at ({x}, {y}) is on a key"
            );
        }
    }

    #[test]
    fn a_row_that_is_not_listed_is_reported_rather_than_silently_skipped() {
        let state = showing(&["aaa"]);
        let mut bench = PanelBench::new();
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(bench.missing_reported, "the bench says the row is missing");
        assert!(
            !bench.answered,
            "and it is waiting to be told what the row is"
        );
    }

    #[test]
    fn an_open_network_is_refused_rather_than_typed_into() {
        let mut state = showing(&["target"]);
        state.aps = Some(ApList::from_ranked(&[AccessPoint {
            secured: false,
            ..ap("target")
        }]));
        let mut bench = PanelBench::new();
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(bench.answered, "there is no run to wait for");
    }

    #[test]
    fn no_run_is_built_before_the_list_is_there() {
        let mut state = DeviceState {
            page: DisplayPage::Network,
            ..DeviceState::default()
        };
        let mut bench = PanelBench::new();
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(!bench.missing_reported, "nothing has been looked at yet");
        assert!(!bench.absent_reported, "nothing has been looked at yet");

        // And nothing while the keyboard is already up over the list: taps decided against a list
        // would land on the keyboard's own cells.
        state.aps = Some(ApList::from_ranked(&[ap("target")]));
        state.setup.keyboard = true;
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(!bench.missing_reported);
    }
}
