//! The link, running beside the product's own tasks.
//!
//! One more future on the cooperative executor the product already runs its input, render and
//! playback on, rather than a task of its own: the station's futures are deliberately not `Send`, and
//! one stack is what this has to fit into.
//!
//! Three decisions are here rather than taken from a default:
//!
//! * **The address is the link.** `LinkPhase::Connected` means "has an address", and this is the only
//!   thing that can hand DHCP's answer up to the state — the stack owns the address and the station
//!   never sees it. Every window, not only the first: a lease that lapses is visible to nothing but a
//!   reader that keeps looking.
//! * **A missing address is also the drop.** When the driver reports the link down, embassy-net
//!   resets the DHCP socket and clears the address as the link drops (`Inner::poll`), so an address
//!   that stops coming is the access point leaving. The station remembers what it was asked to do,
//!   and a drop is not a call it makes.
//! * **The loop never gives up.** A refused association, an address that never arrives and an address
//!   that goes away all lead back to associating again after a pause, for as long as the device is on.
//!
//! A build with no credentials does not park: it surveys, and keeps surveying, because it is the one
//! waiting to be set up. What it finds rides [`crate::render::SCAN`] rather than the intent ring — a
//! list is a window, and the ring prices every slot at its widest message. What the panel's keyboard
//! produces comes back the same way, through [`crate::render::JOIN`]: one slot, because a credential
//! is a value only its newest form means anything as.
//!
//! Credentials come from three places and the difference matters. A build input is the bench's, and a
//! device given one has nowhere else to go, so a refusal is retried for ever. The device's own flash
//! holds what the panel has been told before, walked at boot so a device that has been set up once
//! comes up on its own; a refusal there falls back to the panel, because the passphrase may have
//! changed on the access point since. The panel is the third, and what it names is kept on the way
//! past.
//!
//! The panel can name a network with or without a passphrase — a row it typed a passphrase for, or a
//! row whose copy is already in the flash — which is [`JoinRequest`]'s two shapes. The second is this
//! module's own lookup, and what a name the flash does not hold is answered with is
//! [`BusinessIntent::NoCopy`], because a passphrase is something only a person at the panel can
//! supply. What neither shape does is write: the copy a store join uses is the one already on the
//! part, and a build's or a panel's credential is kept where it is joined.

use alloc::boxed::Box;
use core::future::pending;

use embassy_futures::join::join;
use embassy_futures::select::{Either, Either4, Either5, select, select4, select5};
use embassy_net::driver::{Driver, HardwareAddress};
use embassy_net::{Config, Stack, StackResources};
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    Credentials, Ipv4, JoinRequest, LinkStatus, SavedNetworks, Ssid, Station, SurveyHistory,
    survey_pause_after,
};
use iot_core::intent::{BusinessIntent, Intent};

use crate::input::INTENT_BUS;
use crate::render::{FORGET, JOIN, LEAVE, RESCAN};

/// How often the link is read, published and reported. The window is the product's heartbeat
/// on the air: a phase that stops being repeated is the finding, so it is short enough to see
/// a drop inside one bench run.
const WINDOW_MS: u64 = 2_000;

/// How long DHCP gets to produce an address before the link is reported associated and not
/// connected. Generous on purpose: a lease offer is a round trip on a network this knows
/// nothing about.
const ADDRESS_BUDGET_MS: u64 = 30_000;

/// How many windows in a row without an address before the station is told to leave and the
/// whole association starts over. A de-bounce rather than a timeout: the address is cleared the
/// moment the driver reports the link down, and the wait is for a lease that is mid-renewal
/// rather than one that is gone.
const DROP_WINDOWS: u8 = 3;

/// How long to wait after leaving — or after a refusal — before associating again.
const RETRY_MS: u64 = 5_000;

/// Socket slots the stack reserves. One, because DHCP is the only consumer: the product opens
/// no socket on this link yet, and this is what keeps the stack's own buffers the only cost
/// here beyond the driver's.
const SOCKETS: usize = 1;

/// Bring the link up and keep it there, for ever.
///
/// A board that wired no radio hands `None` and the product runs without a link — that is the
/// answer for the C6 with no panel to configure one on as much as for a board whose radio would
/// not start, and neither is a reason for the rest of the device to stop.
///
/// The saved networks come in boxed and stay that way: this future is polled inside the task the
/// product shares, so a list carried by value is a thousand bytes of the main stack's own region
/// per frame that holds it, and the storage is the only copy either way.
pub async fn serve<S, R>(station: Option<Box<S>>, saved: Option<Box<Store<R>>>) -> !
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let Some(mut station) = station else {
        log::warn!("[WIFI] this build wired no radio, so the link stays off");
        park().await
    };

    let device = station
        .take_device()
        .expect("the station hands its interface over on the first ask");
    let seed = seed_from(&device);
    // The stack borrows its resources for as long as it runs and every loop below is inside
    // that borrow, so they have to outlive this frame. Leaked rather than put in a `static`
    // cell: this is the only place that ever asks for them, and one allocation of a few
    // hundred bytes that is never given back is cheaper than a dependency for a cell.
    let resources = Box::leak(Box::new(StackResources::<SOCKETS>::new()));
    let (stack, mut runner) =
        embassy_net::new(device, Config::dhcpv4(Default::default()), resources, seed);
    log::info!("[WIFI] stack up");

    match join(runner.run(), link(&mut *station, &stack, saved)).await {}
}

/// The link's own loop: associate, take an address, report the window, and start over.
///
/// Which of the three credential paths this is decides how a refusal is treated, so it is decided
/// here and handed down rather than read again below.
async fn link<S, R>(station: &mut S, stack: &Stack<'_>, mut saved: Option<Box<Store<R>>>) -> !
where
    S: Station,
    R: FlashRegion,
{
    // The list the page is showing lives in `setup`, which is the only path that surveys: the link
    // itself never needs it, because the credential it holds is one the store already has and the
    // rows it would re-mark are the ones `setup` publishes.
    if let Some((ssid, password)) = credentials_from_build() {
        // Diverges: a build that named its network is the only path it has.
        named_by_build(station, stack, ssid, password, saved).await
    }

    // Nothing was named by the build, so the first credential to try is the one this device
    // kept the last time it was set up — the point of keeping it is that a device that has been
    // set up once comes up on its own. What a refusal means here is the panel's: the access
    // point may have changed on the other side, and only a person can say the new passphrase.
    if let Some(credentials) = saved
        .as_ref()
        .and_then(|store| store.networks().iter().next())
    {
        log::info!(
            "[WIFI] this device saved a network; joining {:?}",
            credentials
                .ssid
                .as_display_text()
                .unwrap_or("<unprintable>"),
        );
        // `GiveBack` rather than `Retry`: the saved list is not the only answer this device has,
        // because the panel is one tap away. `Stored`, because this credential *is* the saved list:
        // there is nothing to write down about a network the part already holds.
        let pending = match hold(
            station,
            stack,
            credentials,
            Provenance::Stored,
            Refusal::GiveBack,
            saved.as_deref_mut(),
            None,
        )
        .await
        {
            Hold::Named(next, origin) => Some((next, origin)),
            Hold::Refused => {
                log::warn!(
                    "[WIFI] the saved network would not admit this device; the panel is what \
                     names the next one"
                );
                None
            }
            Hold::Left => None,
        };
        setup(station, stack, saved, pending).await
    }

    log::info!(
        "[WIFI] this build was given no network and this device has saved none, so the panel is \
         what names one: the radio looks around until the keyboard hands a credential over"
    );
    // Diverges: an unconfigured build with nothing saved never leaves `setup`.
    setup(station, stack, saved, None).await
}

/// The path a build that named its network takes.
///
/// Its own function because it never comes back: a build input is the only network a device with
/// nothing saved knows, so a refusal is retried rather than given back. The one thing the panel can
/// still say is that this network is not the one — which is `setup`'s.
async fn named_by_build<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    ssid: &'static str,
    password: &'static str,
    mut saved: Option<Box<Store<R>>>,
) -> !
where
    S: Station,
    R: FlashRegion,
{
    let credentials = match Credentials::from_text(ssid, password) {
        Ok(credentials) => credentials,
        Err(error) => {
            log::error!("[WIFI] the credentials this build was given are unusable: {error:?}");
            park().await
        }
    };
    log::info!("[WIFI] this build joins the network named {ssid:?}");
    // A build that named its network is the bench's, and the bench is not standing at the panel:
    // there is no other network this device could be asked for, which is what `Refusal::Retry`
    // and the parking arm below are.
    match hold(
        station,
        stack,
        credentials,
        Provenance::Named,
        Refusal::Retry,
        saved.as_deref_mut(),
        None,
    )
    .await
    {
        // Somebody at the panel did ask, though, and the one thing a device can be asked for from
        // the list is to leave: handing the link to `setup` is answering the panel with the path
        // this device would have had if the build had named nothing.
        Hold::Left => {
            log::info!(
                "[WIFI] the panel took this device off its build's network; the panel is what \
                 names the next one"
            );
            setup(station, stack, saved, None).await
        }
        // Unreachable: a retried credential has no other way out of `hold`. Parking rather than
        // returning, because a task that fell through would be one that had quietly stopped.
        Hold::Named(..) | Hold::Refused => park().await,
    }
}

/// Where the credential the link is joining came from, which decides whether joining it writes it
/// down: one from outside the device is worth keeping at the association that works, while one the
/// store handed over *is* the copy, and writing it back would be a sector of wear for nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Provenance {
    /// Named by the build, or typed on the panel.
    Named,
    /// Handed over by the store, for a row the panel picked without typing.
    Stored,
}

/// How a held credential stopped being held.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// The panel named another network while this one was up, carrying where that credential came
    /// from: the caller joins it the same way this one was joined.
    ///
    /// Handed to the caller rather than put back on the join plane, which has been read once already.
    Named(Credentials, Provenance),
    /// The radio would not associate, and this path gives the credential back rather than retrying
    /// it: the passphrase may be wrong, and the panel is where it can be fixed.
    Refused,
    /// The panel asked to leave the network the link was on, with the link down and reported.
    Left,
}

/// How a credential that will not associate is treated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// Try it again for ever: a build input is the only network this device knows.
    Retry,
    /// Hand it back to [`setup`]: a wrong passphrase is the most likely reason a credential was
    /// refused, and the panel can name another one.
    GiveBack,
}

/// Bring the link up with one credential and hold it there.
///
/// Returns on any of [`Hold`]'s three outcomes, and never on a link that merely dropped: a link that
/// came up and was then lost is reconciled here, because the credential was right and the answer to a
/// drop is to associate again with it. What a refusal means is the caller's, which is why `refusal` is
/// handed down rather than decided here.
///
/// An association is also where a credential from outside the device is written to the part — once per
/// credential, not once per association, because a link that drops and comes back is the same
/// credential joining again. A credential the store handed over is not written at all; see
/// [`Provenance`].
async fn hold<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    credentials: Credentials,
    origin: Provenance,
    refusal: Refusal,
    mut saved: Option<&mut Store<R>>,
    mut history: Option<&mut SurveyHistory>,
) -> Hold
where
    S: Station,
    R: FlashRegion,
{
    let mut kept = false;
    loop {
        if let Err(error) = station.begin_connect(&credentials) {
            log::error!("[WIFI] the radio would not take that credential: {error:?}");
            publish(station.status());
            if refusal == Refusal::GiveBack {
                return Hold::Refused;
            }
            Timer::after(Duration::from_millis(RETRY_MS)).await;
            continue;
        }
        // Published between the two halves of the attempt rather than after it, because the wait
        // below is where the whole attempt is spent and the panel draws the status for all of it: a
        // passphrase that is wrong is refused only when the handshake runs out, and until this was
        // here the page went on showing the phase the attempt started from — a strip offering
        // `SCAN` and a list that still took a pick. It is the station's own reading, taken the
        // moment the radio has one.
        publish(station.status());
        if let Err(error) = station.await_connected().await {
            log::error!("[WIFI] the radio would not associate: {error:?}");
            publish(station.status());
            if refusal == Refusal::GiveBack {
                return Hold::Refused;
            }
            Timer::after(Duration::from_millis(RETRY_MS)).await;
            continue;
        }
        log::info!(
            "[WIFI] associated; up to {ADDRESS_BUDGET_MS} ms for an address, then reported as \
             associated and not connected"
        );
        // Saved on the association rather than on the address: the passphrase is what this write
        // is about, and the answer to it is in hand here. Waiting for DHCP would put a link that
        // never gets an address between a working passphrase and the next boot's copy of it.
        if origin == Provenance::Named
            && !kept
            && let Some(store) = saved.as_deref_mut()
        {
            kept = true;
            keep(store, &credentials);
            // The row that says this network is remembered is the store's own word, and the write
            // above just changed it: the list the page is showing goes back with the new marks.
            // Only the panel's own path has that list — the other two hold a credential the store
            // already had, and the marks they would put back are the ones already there.
            if let Some(history) = history.as_deref_mut() {
                remark(history, saved.as_deref());
            }
        }

        // The budget arm is what makes "the network answered nothing" a report rather than a
        // link that never gets anywhere: the station is associated either way, and the phase
        // says which.
        let address = match select(
            wait_for_address(stack),
            Timer::after(Duration::from_millis(ADDRESS_BUDGET_MS)),
        )
        .await
        {
            Either::First(address) => address,
            Either::Second(()) => None,
        };
        station.note_address(address);
        publish(station.status());
        if address.is_none() {
            log::error!("[WIFI] no address within {ADDRESS_BUDGET_MS} ms");
        }

        let mut missing_windows: u8 = 0;
        loop {
            // Waiting on the credential plane as well as the window, so a panel that names
            // another network is heard even while this one is up: the list takes no pick under a
            // link, but a draft picked before the radio associated is still open, and a keyboard
            // that could not change the answer would be one that only worked while nothing was
            // happening. And on the leave plane, because the action strip is the other thing a
            // panel can say — and on the forget plane, because a row can be held while the link
            // is up: the store is not the link's.
            match select4(
                Timer::after(Duration::from_millis(WINDOW_MS)),
                JOIN.ready(),
                LEAVE.ready(),
                FORGET.ready(),
            )
            .await
            {
                Either4::Second(()) => {
                    if let Some(asked) = JOIN.take()
                        && let Some((next, origin)) = asked_for(asked, saved.as_deref())
                    {
                        log::info!("[WIFI] the panel named another network; leaving this one");
                        let _ = station.disconnect().await;
                        return Hold::Named(next, origin);
                    }
                }
                Either4::Third(()) => {
                    log::info!("[WIFI] the panel asked to leave this network; the link is down");
                    let _ = station.disconnect().await;
                    // Reported here rather than by a later window: the wait above is what the
                    // last report was made on, and the page has to say `Off` the moment there is
                    // no link rather than a window later.
                    publish(station.status());
                    return Hold::Left;
                }
                // Not this link's business, and acted on all the same: a forget is about the copy
                // on the part, and the window above is the rest of this loop rather than a state
                // the link is in.
                Either4::Fourth(ssid) => {
                    drop_saved(saved.as_deref_mut(), &ssid);
                    if let Some(history) = history.as_deref_mut() {
                        remark(history, saved.as_deref());
                    }
                    continue;
                }
                Either4::First(()) => {}
            }
            let address = stack_address(stack);
            // Every window, for the reason the phase can only follow DHCP if somebody says so
            // — and the only way the loop learns the address went away.
            station.note_address(address);
            let status = station.status();
            publish(status);
            log::info!(
                "[WIFI] phase {:?}, address {:?}, {} windows without one",
                status.phase,
                address.map(|address| address.octets()),
                missing_windows,
            );
            if address.is_some() {
                missing_windows = 0;
                continue;
            }
            missing_windows += 1;
            if missing_windows >= DROP_WINDOWS {
                log::warn!(
                    "[WIFI] no address for {DROP_WINDOWS} windows; leaving and associating again"
                );
                break;
            }
        }

        // Best effort: the driver may already be gone, and it is the association below that
        // matters. A refusal here would only be a second thing to report.
        let _ = station.disconnect().await;
        Timer::after(Duration::from_millis(RETRY_MS)).await;
    }
}

/// What an unconfigured build does with the radio: look around until the panel names a network,
/// then join it.
///
/// This is the state the product ships in. A credential is taken *between* rounds rather than by
/// racing the scan — a scan cut off mid-flight is one whose outcome nobody sees — and the pause waits
/// on the credential plane, so the panel's tap does not sit behind a five-second rest.
///
/// The store is carried through here rather than put back for the caller: this is where a panel-typed
/// credential is joined, and joining it is when it is worth keeping.
async fn setup<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    mut saved: Option<Box<Store<R>>>,
    first: Option<(Credentials, Provenance)>,
) -> !
where
    S: Station,
    R: FlashRegion,
{
    let page = &crate::render::PAGE;
    // In hand after a refusal that came back, from a boot credential the panel superseded, or
    // waiting on the plane. Held rather than re-taken, because a credential that arrives while a
    // link is being held is returned by [`hold`] rather than put back on a one-slot plane.
    let mut pending = first;
    // Rounds in a row that found nothing, which is what tells a bench hiccup from a place with
    // nothing in it; cleared by a credential coming back.
    let mut empty_rounds: u8 = 0;
    // Rounds in a row nobody has acted on, which is what lengthens the pause: a list nobody has
    // touched is still the truth about the room.
    let mut idle_rounds: u8 = 0;
    // The list as the page has it, carried from round to round: what keeps a row still while the
    // readings move under it. Cleared when the page is entered. It outlives the `hold` calls below,
    // so a save or a forget while the link is up can put the store's own marks back on the rows the
    // page is showing.
    let mut history = SurveyHistory::new();
    loop {
        pending = match pending.take().or_else(|| {
            JOIN.take()
                .and_then(|asked| asked_for(asked, saved.as_deref()))
        }) {
            Some((credentials, origin)) => {
                log::info!(
                    "[WIFI] joining the network the panel named: {:?}",
                    credentials
                        .ssid
                        .as_display_text()
                        .unwrap_or("<unprintable>"),
                );
                empty_rounds = 0;
                idle_rounds = 0;
                // Before the association that credentials this link, and not after: a leave
                // outstanding from the link before this one would otherwise take effect the moment
                // this one came up, which is a device that joins a network and leaves it at once.
                LEAVE.clear();
                match hold(
                    station,
                    stack,
                    credentials,
                    origin,
                    Refusal::GiveBack,
                    saved.as_deref_mut(),
                    Some(&mut history),
                )
                .await
                {
                    Hold::Named(next, origin) => Some((next, origin)),
                    Hold::Refused | Hold::Left => None,
                }
            }
            None => {
                // Nothing to act on, so look around — but only where the list can be seen: a scan
                // is a round trip the radio spends on a page nobody is on, and what it fills is a
                // list drawn a minute before it is read. A credential that arrives while the panel
                // is elsewhere is still acted on above, because the keyboard that names one is on
                // this page to begin with.
                if page.wait_for_list().await {
                    // Arriving is an act, and the list shown on arrival should be the room as it is
                    // rather than what the clock had worked its way up to.
                    empty_rounds = 0;
                    idle_rounds = 0;
                    history.clear();
                }
                // A forget lands in the rest between rounds and is acted on rather than held: it is
                // about the part, and a list that says a network is saved while it is not is the one
                // thing this path must not leave standing.
                if let Rest::Forget(ssid) = survey(
                    station,
                    &mut history,
                    saved.as_deref().map(|store| store.networks()),
                    &mut empty_rounds,
                    &mut idle_rounds,
                )
                .await
                {
                    drop_saved(saved.as_deref_mut(), &ssid);
                    remark(&mut history, saved.as_deref());
                }
                None
            }
        };
    }
}

/// What the panel's request asks the link to join with, and where that credential came from.
///
/// A name the store holds no copy of is not a credential at all, so the page is told: `None` is that
/// answer, and it is published here rather than handed back because both readers of the plane answer
/// a name the same way.
fn asked_for<R: FlashRegion>(
    asked: JoinRequest,
    saved: Option<&Store<R>>,
) -> Option<(Credentials, Provenance)> {
    match asked {
        JoinRequest::Typed(credentials) => Some((credentials, Provenance::Named)),
        JoinRequest::Held(ssid) => {
            let copy = saved.and_then(|store| store.networks().copy_of(&ssid));
            if copy.is_none() {
                log::info!(
                    "[WIFI] nothing is kept for {:?}; the panel is what asks for a passphrase",
                    ssid.as_display_text().unwrap_or("<unprintable>"),
                );
                let _ = INTENT_BUS.try_send(Intent::Business(BusinessIntent::NoCopy(ssid)));
            }
            copy.map(|credentials| (credentials, Provenance::Stored))
        }
    }
}

/// What ended a survey round's rest, when it is something the caller has to act on.
///
/// A credential and a look-around request are both taken where they are read; only a forget names a
/// network the caller is the one holding.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rest {
    /// The rest ran out, or something asked for the next round early.
    Woken,
    /// The panel asked to forget a saved network.
    Forget(Ssid),
}

/// One survey round: look around, publish what the radio saw, and rest.
///
/// Repeated rather than taken once, and never ending: the radio is the only thing that can say what is
/// in range, and a device waiting to be set up is exactly the one that should keep asking. The rest is
/// cut short by a credential arriving, so naming a network does not wait out a pause that belongs to
/// the survey, and by the panel asking to look around now. Its length is core's policy
/// ([`survey_pause_after`]), and the two counters above are what it reads.
///
/// What reaches the page is what `history` makes of the round rather than the round itself: a scan is
/// one reading of a radio that moves by decibels, and a list whose rows swap every round
/// is a list no finger can aim at. A round the page would draw the same is not published at all —
/// there is nothing to redraw, and a redraw here is a whole frame.
async fn survey<S>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: Option<&SavedNetworks>,
    empty_rounds: &mut u8,
    idle_rounds: &mut u8,
) -> Rest
where
    S: Station,
{
    let empty = match station.scan().await {
        Ok(ranked) => {
            let empty = ranked.is_empty();
            log::info!("[WIFI] survey found {} access points", ranked.len());
            // The phase first: the scan leaves the station `Off`, and the page draws the
            // list under that word, so a publish that carried only one of the two would
            // leave the page a round behind what it is showing.
            publish(station.status());
            if let Some(list) = history.observe(&ranked, saved) {
                crate::render::SCAN.publish(list);
            } else {
                log::info!("[WIFI] the list is unchanged, so the page is not redrawn");
            }
            empty
        }
        Err(error) => {
            log::error!("[WIFI] the radio would not scan: {error:?}");
            publish(station.status());
            false
        }
    };
    *empty_rounds = if empty {
        empty_rounds.saturating_add(1)
    } else {
        0
    };
    *idle_rounds = idle_rounds.saturating_add(1);
    // How long that buys the radio is core's policy — see [`survey_pause_after`] — and this loop
    // is what waits it out.
    let pause = survey_pause_after(*empty_rounds, *idle_rounds);
    match select5(
        Timer::after(Duration::from_millis(pause)),
        JOIN.ready(),
        // Woken when the panel is put *back* on the list, so a page entered during a pause that has
        // grown to half a minute surveys at once. Not the level check: this pause is spent with the
        // list up, so that would be true on the first poll and the pause would be skipped every time.
        crate::render::PAGE.wait_for_return(),
        // And woken by the strip's own "look around now", which is the same round this pause would
        // have started once it ran out: the panel asks because a person is looking at a list that
        // was read before they got there.
        RESCAN.ready(),
        FORGET.ready(),
    )
    .await
    {
        Either5::Fifth(ssid) => Rest::Forget(ssid),
        _ => Rest::Woken,
    }
}

/// What the stack currently holds, in the form the station takes it.
fn stack_address(stack: &Stack<'_>) -> Option<Ipv4> {
    stack
        .config_v4()
        .map(|config| Ipv4::from_octets(config.address.address().octets()))
}

/// Wait for DHCP to produce an address.
async fn wait_for_address(stack: &Stack<'_>) -> Option<Ipv4> {
    loop {
        if let Some(address) = stack_address(stack) {
            return Some(address);
        }
        stack.wait_config_up().await;
    }
}

/// Hand the link to the state layer.
///
/// A drop on a full bus is the right failure: a link snapshot is state, the next window
/// supersedes it, and a producer that waited for room would hold the radio behind the panel.
fn publish(status: LinkStatus) {
    let _ = INTENT_BUS.try_send(Intent::Business(BusinessIntent::Link(status)));
}

/// Write the network the link just joined to the part it is kept in.
///
/// Every credential the link joins goes through here, from all three paths, so what it says is about
/// the network rather than about who named it.
///
/// Best effort on purpose: the link is up either way, and what a failure costs is the next boot's copy
/// of it — a reader who sees that line will be typing this passphrase again after the next reset.
fn keep<R: FlashRegion>(store: &mut Store<R>, credentials: &Credentials) {
    match store.remember(credentials) {
        Ok(true) => {
            log::info!("[WIFI] this network was already saved, so the saved copy was updated")
        }
        Ok(false) => log::info!("[WIFI] this network is saved for the next boot"),
        Err(error) => {
            log::warn!("[WIFI] the part would not keep the network ({error:?}); this session only")
        }
    }
}

/// Erase one saved network, and tell the page what is left.
///
/// The store is optional because the part is: a board whose build wired a radio but no credential
/// partition has nothing to forget, and saying so is the whole of what this can do there.
fn drop_saved<R: FlashRegion>(store: Option<&mut Store<R>>, ssid: &Ssid) {
    let Some(store) = store else {
        log::warn!("[WIFI] the panel asked to forget a network, but this board keeps none");
        return;
    };
    match store.forget(ssid) {
        Ok(true) => log::info!(
            "[WIFI] {:?} is forgotten; the next boot will not join it",
            ssid.as_display_text().unwrap_or("<unprintable>"),
        ),
        Ok(false) => log::info!(
            "[WIFI] the panel asked to forget {:?}, which this device had not saved",
            ssid.as_display_text().unwrap_or("<unprintable>"),
        ),
        Err(error) => log::warn!(
            "[WIFI] the part would not drop the saved network ({error:?}); this session only"
        ),
    }
}

/// Put the store's own marks back on the list the page is showing.
///
/// The authority is the store and never what the page has been asked to join: a device whose part
/// refused the write will not remember the network on the next boot, and the row that marks it as
/// remembered has to say what the next boot will do. A no-op on a board with no partition.
///
/// The list is the last one the survey published rather than a new round — a save or a forget is about
/// the part, and what is in range has not changed. `marked` answers `None` when the marks come out the
/// same, and nothing is published then.
fn remark<R: FlashRegion>(history: &mut SurveyHistory, saved: Option<&Store<R>>) {
    if let Some(list) = history.marked(saved.map(|store| store.networks())) {
        crate::render::SCAN.publish(list);
    }
}

/// The seed the stack takes for the DHCP transaction id and its ephemeral ports.
///
/// The station's own address is the part that has to differ between two devices on one network,
/// and the boot's uptime the part that has to differ between two boots of one. There is no RNG
/// call here because the chip's is behind the chip layer, which this module is compiled without
/// — the address the driver reports is entropy this layer can reach.
fn seed_from<D: Driver>(device: &D) -> u64 {
    let octets = match device.hardware_address() {
        HardwareAddress::Ethernet(octets) => octets,
        _ => [0; 6],
    };
    let address = octets
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
}

/// The credentials this build was given, or `None` when the build named neither — or when this is
/// a bench build, whose credentials are typed on the panel instead.
///
/// `option_env!` rather than a secret store, the same shape the probe uses: a bench image saying
/// which network it joins, with the value it says compiled into the image. Both names are in
/// `scripts/iot-xtensa.sh`'s `FORWARDED_ENV`, and `build_env.rs` fails the build if one is read
/// but not forwarded.
fn credentials_from_build() -> Option<(&'static str, &'static str)> {
    if crate::panel::BENCH {
        return None;
    }
    let ssid = option_env!("IOT_WIFI_SSID")?;
    let password = option_env!("IOT_WIFI_PSK")?;
    Some((ssid, password))
}

/// What an arm with nothing to do waits on. Never returns: the alternative is an arm that
/// falls through, which would leave the product with a task that has quietly stopped.
async fn park() -> ! {
    pending().await
}
