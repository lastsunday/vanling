//! The credential rounds: what to join, what a refusal means, and the association that holds it.
//!
//! [`link`] is the loop `serve` hands the stack to. `hold` is one credential on the air; the
//! offline end is `browsing`, and a build that named its network takes `named_by_build` instead.

use alloc::boxed::Box;

use embassy_futures::select::{Either, Either4, select, select4};
use embassy_net::Stack;
use embassy_net::StackResources;
use embassy_net::driver::Driver;
use embassy_time::{Duration, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    Credentials, Front, Ipv4, LinkEnding, LinkHint, Ssid, Station, SurveyHistory, front_after_link,
    setup_ap_starts_secured,
};

use super::setup::{Face, Setup, asked_for, setup_credential, unconfigured};
use super::store::{drop_saved, first_saved, has_saved, keep, keeps_any, remark};
use super::{SETUP_SOCKETS, park, publish};
use crate::render::{FORGET, JOIN, LEAVE, SETUP_ASK};

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

/// The link's own loop: associate, take an address, report the window, and start over.
///
/// Which of the three credential paths this is decides how a refusal is treated, so it is decided
/// here and handed down rather than read again below.
pub(super) async fn link<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    saved: Option<Box<Store<R>>>,
    setup_name: &Ssid,
    setup_resources: &mut StackResources<SETUP_SOCKETS>,
    front: Front,
    panel: bool,
) -> !
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    // Which front end setup mode offers survives the rounds a refusal puts it through: a
    // passphrase retyped on a phone is one the page should still be serving, and a person who
    // switched to the panel is not sent back to the access point by a credential that did not work.
    // `start` is what the build answered, kept because a link's end goes back to it.
    let mut front = front;
    let start = front;
    let mut saved = saved;

    // Nothing is associated until the splash is over, and what the button asked for while it was up
    // is part of the answer rather than a separate decision: the render loop publishes the page's
    // end together with the request, so there is no window in which a hold could be read after the
    // link had already joined something.
    let landing = crate::render::PAGE.wait_for_splash_end().await;
    let mut setup = Setup {
        name: setup_name,
        resources: setup_resources,
        front: &mut front,
        panel,
        secure_on_entry: setup_ap_starts_secured(panel, landing),
    };
    if landing {
        log::info!(
            "[WIFI] the splash asked for setup mode; the access point is the front it opens on"
        );
        // The request names the access point whatever the build's own front is: the person is
        // holding a phone, and the panel's list is one cell away on the strip.
        *setup.front = Front::AccessPoint;
        let cancellable = has_saved(&saved);
        unconfigured(station, stack, &mut saved, None, cancellable, &mut setup).await;
        log::info!("[WIFI] setup mode is over; this device goes back to its own page");
        // The splash's round is over. Every later entry is made from a running device, so it
        // follows the running rule even on a board that booted into the hold.
        setup.secure_on_entry = setup_ap_starts_secured(panel, false);
    }

    if let Some((ssid, password)) = credentials_from_build() {
        // Diverges: a build that named its network is the only path it has.
        named_by_build(station, stack, ssid, password, saved, &mut setup).await
    }

    loop {
        // Nothing saved: the device runs offline, and its panel page still browses. No access point
        // is raised and no scan runs until somebody is on the page — this is the state an unwrapped
        // device boots into. What browsing returns is the first credential to hold.
        let (credentials, mut origin) = match first_saved(&saved) {
            Some(chosen) => chosen,
            // A board with no panel has no page to browse and no cell to open the access point
            // from: nothing is saved, so there is nothing it can do but wait. Setup mode is still
            // reachable from the splash, which is the only entry this board accepts.
            None if !panel => {
                log::info!("[WIFI] no panel and no saved network; waiting offline");
                core::future::pending().await
            }
            None => {
                log::info!(
                    "[WIFI] this build was given no network and this device has saved none; it runs \
                     offline, and the panel's page names one"
                );
                match Box::pin(browsing(station, &mut saved)).await {
                    Some(chosen) => chosen,
                    None => {
                        log::info!(
                            "[WIFI] the panel asked for the access point; setup mode opens on it"
                        );
                        *setup.front = Front::AccessPoint;
                        let cancellable = has_saved(&saved);
                        unconfigured(station, stack, &mut saved, None, cancellable, &mut setup)
                            .await;
                        log::info!("[WIFI] setup mode is over; this device goes back to its page");
                        continue;
                    }
                }
            }
        };
        // Boxed across the holds below: a credential is a hundred bytes, and every byte a frame
        // carries across an await is a static in the main task's pool. The heap copy is one
        // allocation per credential rather than one per window.
        let mut credentials = Box::new(credentials);
        // One credential and everything the link does with it, including the panel naming another
        // while it is up: a new credential replaces the one in hand rather than waiting for the
        // next round, which is what keeps this loop from carrying two credentials at once.
        let ended = loop {
            if origin == Provenance::Stored {
                log::info!(
                    "[WIFI] this device saved a network; joining {:?}",
                    credentials
                        .ssid
                        .as_display_text()
                        .unwrap_or("<unprintable>"),
                );
            }
            // `GiveBack` rather than `Retry`: the saved list is not the only answer this device
            // has, because the panel is one tap away.
            let held = hold(
                station,
                stack,
                credentials.as_ref(),
                origin,
                Refusal::GiveBack,
                saved.as_deref_mut(),
                None,
            )
            .await;
            *setup.front = front_after_link(start, *setup.front, held.ending());
            match held {
                Hold::Named(next, next_origin) => {
                    credentials = next;
                    origin = next_origin;
                }
                other => break other,
            }
        };
        match ended {
            // Consumed by the inner loop above; a link never breaks out of it with another
            // credential.
            Hold::Named(..) => {}
            Hold::Left => {
                // The panel's own leave. Setup mode is what it asked for, and the saved credential
                // is what `CANCEL` goes back to.
                log::info!(
                    "[WIFI] the panel left the link; setup mode opens on the {:?} front",
                    *setup.front,
                );
                let cancellable = has_saved(&saved);
                unconfigured(station, stack, &mut saved, None, cancellable, &mut setup).await;
                log::info!(
                    "[WIFI] setup mode is over; this device goes back to the network it keeps"
                );
            }
            Hold::Refused => {
                log::warn!(
                    "[WIFI] the saved network would not admit this device; the panel is what names \
                     the next one"
                );
                let cancellable = has_saved(&saved);
                unconfigured(station, stack, &mut saved, None, cancellable, &mut setup).await;
                log::info!(
                    "[WIFI] setup mode is over; this device goes back to the network it keeps"
                );
            }
        }
    }
}

/// The browsing end: the list and its survey, with nothing on the air and no way back.
///
/// The survey runs only while the panel is on the page, so an unwrapped device is idle rather than
/// a beacon nobody asked for. Returns a credential, or [`None`] when the page asks for the access
/// point; every other answer waits for the page or surveys again.
async fn browsing<S, R>(
    station: &mut S,
    saved: &mut Option<Box<Store<R>>>,
) -> Option<(Credentials, Provenance)>
where
    S: Station,
    R: FlashRegion,
{
    // A request that outlived the list it was made from would open the access point before anyone
    // is looking at the page again, which is the mode opening itself.
    SETUP_ASK.clear();
    // Boxed for the reason every other history in this file is: it is the survey's hundreds of
    // bytes, and this future lives on the product's one stack (`@/records/iot/wifi.md`).
    let mut history = Box::new(SurveyHistory::new());
    loop {
        crate::render::PAGE.wait_for_list().await;
        match select(
            setup_credential(station, &mut history, saved, Face::browsing(), true),
            SETUP_ASK.ready(),
        )
        .await
        {
            Either::First(Some(chosen)) => return Some(chosen),
            Either::First(None) => {
                log::info!("[WIFI] the panel left the list; the survey stops until it is back");
            }
            Either::Second(ask) if ask.applies_to(Front::Panel) => {
                log::info!("[WIFI] the panel asked for the access point from the list");
                return None;
            }
            // The box's own answers have no box on this face; a stale one is dropped rather than
            // answered on a page that never drew it.
            Either::Second(_) => {}
        }
    }
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
    saved: Option<Box<Store<R>>>,
    setup: &mut Setup<'_>,
) -> !
where
    S: Station,
    S::Device: Driver,
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
    let mut saved = saved;
    // The build's answer, kept for the same reason `link` keeps it: the panel's leave is a tap
    // made at the panel.
    let start = *setup.front;
    // A build that named its network is the bench's, and the bench is not standing at the panel:
    // there is no other network this device could be asked for, which is what `Refusal::Retry`
    // and the parking arm below are. The one thing the panel can still say is that this network is
    // not the one — and closing setup mode is what goes back to it.
    loop {
        let ended = hold(
            station,
            stack,
            &credentials,
            Provenance::Named,
            Refusal::Retry,
            saved.as_deref_mut(),
            None,
        )
        .await;
        *setup.front = front_after_link(start, *setup.front, ended.ending());
        match ended {
            // Somebody at the panel did ask, though: handing the link to the unconfigured path is
            // answering the panel with the path this device would have had if the build had named
            // nothing.
            Hold::Left => {
                log::info!(
                    "[WIFI] the panel took this device off its build's network; setup mode opens \
                     on the {:?} front until it names another",
                    *setup.front,
                );
                unconfigured(station, stack, &mut saved, None, true, setup).await;
                log::info!(
                    "[WIFI] setup mode is over; this device goes back to the network its build \
                     named"
                );
            }
            // Unreachable: a retried credential has no other way out of `hold`. Parking rather than
            // returning, because a task that fell through would be one that had quietly stopped.
            Hold::Named(..) | Hold::Refused => park().await,
        }
    }
}

/// Where the credential the link is joining came from, which decides whether joining it writes it
/// down: one from outside the device is worth keeping at the association that works, while one the
/// store handed over *is* the copy, and writing it back would be a sector of wear for nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Provenance {
    /// Named by the build, or typed on the panel.
    Named,
    /// Handed over by the store, for a row the panel picked without typing.
    Stored,
}

/// How a held credential stopped being held.
///
/// Boxed where it carries a credential, for the reason the credential store is: this is the output
/// of a future three frames await, and a credential by value would be a hundred bytes of each
/// frame's state (`@/records/iot/wifi.md`).
pub(super) enum Hold {
    /// The panel named another network while this one was up, carrying where that credential came
    /// from: the caller joins it the same way this one was joined.
    ///
    /// Handed to the caller rather than put back on the join plane, which has been read once already.
    Named(Box<Credentials>, Provenance),
    /// The radio would not associate, and this path gives the credential back rather than retrying
    /// it: the passphrase may be wrong, and the panel is where it can be fixed.
    Refused,
    /// The panel asked to leave the network the link was on, with the link down and reported.
    Left,
}

impl Hold {
    /// This ending as [`front_after_link`] reads it: a refusal keeps the front it was typed on,
    /// while a leave or a pick goes back to the build's own.
    const fn ending(&self) -> LinkEnding {
        match self {
            Self::Named(..) => LinkEnding::Named,
            Self::Refused => LinkEnding::Refused,
            Self::Left => LinkEnding::Left,
        }
    }
}

/// How a credential that will not associate is treated.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// Try it again for ever: a build input is the only network this device knows.
    Retry,
    /// Hand it back to [`setup`]: a wrong passphrase is the most likely reason a credential was
    /// refused, and the panel can name another one.
    GiveBack,
}

/// Bring the link up with one credential and hold it there. A link that drops is reconciled here,
/// because the credential was right; only a refusal leaves. An outside credential is written once,
/// not per association, and a stored one not at all; see [`Provenance`].
pub(super) async fn hold<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    credentials: &Credentials,
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
    // The access point this network was last found on, when the store knows. Best effort by
    // construction: a hinted attempt that fails is retried once with no hint, so a stale channel
    // costs one failed association rather than a credential the page reports as refused.
    let mut hint = saved
        .as_ref()
        .map(|store| store.networks().hint_of(&credentials.ssid))
        .filter(|hint| hint.is_known());
    loop {
        let began = match hint {
            Some(hint) => station.begin_connect_hinted(credentials, hint),
            None => station.begin_connect(credentials),
        };
        if let Err(error) = began {
            if hint.take().is_some() {
                log::warn!(
                    "[WIFI] the remembered access point would not take the attempt ({error:?}); \
                     scanning every channel"
                );
                continue;
            }
            log::error!("[WIFI] the radio would not take that credential: {error:?}");
            publish(
                station.status(),
                keeps_any(saved.as_deref().map(|store| store.networks())),
            );
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
        publish(
            station.status(),
            keeps_any(saved.as_deref().map(|store| store.networks())),
        );
        if let Err(error) = station.await_connected().await {
            if hint.take().is_some() {
                log::warn!(
                    "[WIFI] the remembered access point did not answer ({error:?}); scanning every \
                     channel"
                );
                continue;
            }
            log::error!("[WIFI] the radio would not associate: {error:?}");
            publish(
                station.status(),
                keeps_any(saved.as_deref().map(|store| store.networks())),
            );
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
        //
        // The access point the radio just named is the hint that makes the next association
        // directed, so it is written in the same pass: a network saved without one would pay the
        // full scan again on the very next boot.
        let found = station
            .status()
            .ap
            .map(|ap| LinkHint::new(ap.bssid, ap.channel))
            .filter(|hint| hint.is_known());
        hint = found;
        if let Some(store) = saved.as_deref_mut() {
            if origin == Provenance::Named && !kept {
                kept = true;
                keep(store, credentials, found.unwrap_or(LinkHint::NONE));
                // The row that says this network is remembered is the store's own word, and the write
                // above just changed it: the list the page is showing goes back with the new marks.
                // Only the panel's own path has that list — the other two hold a credential the store
                // already had, and the marks they would put back are the ones already there.
                if let Some(history) = history.as_deref_mut() {
                    remark(history, saved.as_deref());
                }
            } else if let Some(hint) = found {
                match store.learn(&credentials.ssid, hint) {
                    Ok(true) => log::info!(
                        "[WIFI] this network answers on channel {}; the next association goes \
                         straight there",
                        hint.channel
                    ),
                    Ok(false) => {}
                    Err(error) => log::warn!(
                        "[WIFI] the part would not keep the channel ({error:?}); the next boot \
                         scans for it"
                    ),
                }
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
        publish(
            station.status(),
            keeps_any(saved.as_deref().map(|store| store.networks())),
        );
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
                        return Hold::Named(Box::new(next), origin);
                    }
                }
                Either4::Third(()) => {
                    log::info!("[WIFI] the panel asked to leave this network; the link is down");
                    let _ = station.disconnect().await;
                    // Reported here rather than by a later window: the wait above is what the
                    // last report was made on, and the page has to say `Off` the moment there is
                    // no link rather than a window later.
                    publish(
                        station.status(),
                        keeps_any(saved.as_deref().map(|store| store.networks())),
                    );
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
            publish(
                status,
                keeps_any(saved.as_deref().map(|store| store.networks())),
            );
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

/// The credentials this build was given, or `None` when it named neither or is a bench build.
/// `option_env!` compiles the values into the image; `scripts/iot-xtensa.sh` forwards both names
/// and `build_env.rs` fails the build if one is read but not forwarded.
fn credentials_from_build() -> Option<(&'static str, &'static str)> {
    if crate::panel::BENCH {
        return None;
    }
    let ssid = option_env!("IOT_WIFI_SSID")?;
    let password = option_env!("IOT_WIFI_PSK")?;
    Some((ssid, password))
}
