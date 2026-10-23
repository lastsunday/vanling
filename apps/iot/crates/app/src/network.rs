//! The link, running beside the product's input, render and playback futures: the station's
//! futures are not `Send`, and one stack is what this has to fit into.
//!
//! Three decisions are here rather than taken from a default:
//!
//! * **The address is the link.** `LinkPhase::Connected` means "has an address"; the stack owns
//!   it, and a lease that lapses is visible only to a reader that keeps looking.
//! * **A missing address is also the drop.** embassy-net clears the address when the link goes;
//!   the station remembers what it was asked to do, so a drop is not a call it makes.
//! * **The loop never gives up.** A refusal, a missing address and a lost address all lead back
//!   to associating after a pause.
//!
//! A device with no credentials stays offline: the page surveys only while somebody is on it,
//! and no access point comes up until it is asked for. Setup mode is asked for, never assumed —
//! the splash's BOOT hold, the panel's `LEAVE`, or a refused saved credential. The survey and
//! join planes are windows rather than the intent ring, because a list is not a command.
//!
//! Credentials come from a build input, the flash, or the panel. A build input has nowhere else
//! to go, so a refusal is retried; a stored one falls back to the panel; a panel one is kept on
//! the way past, with the BSSID and channel it worked on. Setup mode's access point belongs to
//! [`crate::provision`] and is raced against the survey below.
//!
//! The long form, including the bench readings, is in `@/records/iot/wifi.md`.

use alloc::boxed::Box;
use core::cell::{Cell, RefCell};
use core::future::pending;

use embassy_futures::join::join;
use embassy_futures::select::{Either, Either4, Either6, select, select4, select6};
use embassy_net::driver::{Driver, HardwareAddress};
use embassy_net::{Config, Stack, StackResources};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    Credentials, Front, Ipv4, JoinRequest, LinkHint, LinkStatus, SavedNetworks, Ssid, Station,
    SurveyHistory, setup_ap_starts_secured, survey_pause_after,
};
use iot_core::intent::{BusinessIntent, Intent};

use crate::input::INTENT_BUS;
use crate::render::{FORGET, JOIN, LEAVE, RESCAN, SETUP_ASK, SetupAsk};

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

/// Socket slots for the setup access point: DHCP, DNS, and the page's
/// [`PAGE_SOCKETS`](crate::provision::PAGE_SOCKETS).
///
/// More than one because the provision loops contend, and a socket that is answering is not
/// listening; a phone opens a handful of connections at once (`@/records/iot/wifi.md`).
const SETUP_SOCKETS: usize = 2 + crate::provision::PAGE_SOCKETS;

/// What the part's heap has left, handed in by the binary that owns the part.
///
/// A static because the only reader is a log line deep in this module's futures; threading the
/// reading through them would be the report wagging the link (`@/records/iot/wifi.md`).
static HEAP_FREE: HeapReport = Mutex::new(RefCell::new(None));

/// The board's own address, read off the driver once at boot.
///
/// A static because no round changes it and threading it would add an argument to a chain of
/// futures that already carry the setup name and the stack's resources.
static MAC: Mutex<CriticalSectionRawMutex, Cell<[u8; 6]>> = Mutex::new(Cell::new([0; 6]));

/// None until a binary hands a reading in; a call so the number is current when the line is
/// written.
type HeapReport = Mutex<CriticalSectionRawMutex, RefCell<Option<fn() -> usize>>>;

/// Hand in what the part's heap has left, once, before the product starts.
pub fn report_heap_free(report: fn() -> usize) {
    HEAP_FREE.lock(|slot| *slot.borrow_mut() = Some(report));
}

/// What the part's heap had left. Zero when no binary handed a reading in, which no build of
/// this product does — zero there reads as a heap with nothing in it rather than as a margin.
fn heap_free() -> usize {
    HEAP_FREE
        .lock(|slot| *slot.borrow())
        .map_or(0, |report| report())
}

/// Setup mode's own state between rounds: the access point's name and sockets, which front is
/// up, and what this board can show.
///
/// One struct because `link` fills it, `named_by_build` and `unconfigured` pass it through, and
/// `configure` is the only reader.
struct Setup<'d> {
    /// The access point's name, derived from the board's own address.
    name: &'d Ssid,
    /// The sockets its page is served from, re-borrowed every time the access point is raised.
    resources: &'d mut StackResources<SETUP_SOCKETS>,
    /// The front offering the mode. It survives a refusal — the page that was typed on should
    /// still be serving — but not a leave or a row picked at the panel; see `front_after_link`.
    front: &'d mut Front,
    /// Whether this build can show a passphrase at all.
    panel: bool,
    /// Whether the next access point starts secured; [`setup_ap_starts_secured`] is the rule.
    /// `configure` reads this once per raise, so a box's answer lasts for that visit only.
    secure_on_entry: bool,
}

/// What setup mode is being offered with: the front, and whether there is a way back.
///
/// [`Station::status`] knows neither, so every publish carries this alongside the radio's own
/// [`LinkStatus`]. Without it the box over the list would close itself a window after opening,
/// and `CANCEL` would turn into `SCAN`.
#[derive(Clone, Copy)]
struct Face {
    front: Front,
    cancellable: bool,
    /// Whether this is setup mode. The page is drawn either way; only a mode says so.
    setup: bool,
    /// Whether this is the offline list rather than a mode. The page offers `SCAN | AP`; a hold
    /// does nothing, and only `AP` leaves this end.
    browsing: bool,
}

impl Face {
    /// The link as this front end reports it.
    fn status(self, station: LinkStatus) -> LinkStatus {
        if self.setup {
            station.on_setup_face(self.front, self.cancellable)
        } else {
            LinkStatus {
                browsing: self.browsing,
                ..station
            }
        }
    }

    /// The list's page outside setup mode: the survey a device with nothing configured runs while
    /// somebody is looking. It is not setup mode: the page offers `SCAN | AP`, and only `AP`
    /// leaves this end and raises the access point.
    const fn browsing() -> Self {
        Self {
            front: Front::Panel,
            cancellable: false,
            setup: false,
            browsing: true,
        }
    }
}

/// Bring the link up and keep it there.
///
/// `None` means this build wired no radio; the rest of the device still runs. `saved` stays boxed
/// because this future lives inside the shared task and the storage is the only copy anyway.
///
/// `front` is where a device with nothing configured enters setup mode first; `panel` says
/// whether a passphrase can be shown there. The splash hold opens the access point either way —
/// a secret nobody can read is a network nobody can join.
pub async fn serve<S, R>(
    station: Option<Box<S>>,
    saved: Option<Box<Store<R>>>,
    front: Front,
    panel: bool,
) -> !
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
    let octets = hardware_octets(&device);
    let seed = seed_from(octets);
    // The access point's own name, from the board's address rather than from the build: two of
    // these devices on one bench are two networks, and the only thing that differs between them
    // is the number at the end of it.
    let setup_name = setup_ssid(octets);
    // And the same address for the two things that draw it: the page the phone reads, and the
    // panel's header. Sent once, because it is the one thing the radio has to say that cannot
    // change while the device is on.
    MAC.lock(|cell| cell.set(octets));
    let _ = INTENT_BUS.try_send(Intent::Business(BusinessIntent::Mac(octets)));
    // The boot's own reading, before any attempt: a lamp-only board has to know whether this
    // device keeps a network from the first second, and the radio's first report otherwise
    // arrives after the splash to answer a question asked before it.
    publish(station.status(), has_saved(&saved));
    // The stack borrows its resources for as long as it runs and every loop below is inside
    // that borrow, so they have to outlive this frame. Leaked rather than put in a `static`
    // cell: this is the only place that ever asks for them, and one allocation of a few
    // hundred bytes that is never given back is cheaper than a dependency for a cell.
    let resources = Box::leak(Box::new(StackResources::<SOCKETS>::new()));
    // Setup mode's own, leaked for the same reason with one more: the access point is raised and
    // lowered every time a credential comes back refused, so what has to outlive each round is
    // one allocation rather than one per round. `embassy-net` writes its resources over when it
    // builds a stack, so what re-uses them is safe only once the stack they held is gone — which
    // is the dropping of the setup future that ends each round.
    let setup_resources = Box::leak(Box::new(StackResources::<SETUP_SOCKETS>::new()));
    let (stack, mut runner) =
        embassy_net::new(device, Config::dhcpv4(Default::default()), resources, seed);
    log::info!("[WIFI] stack up");

    match join(
        runner.run(),
        link(
            &mut *station,
            &stack,
            saved,
            &setup_name,
            &mut *setup_resources,
            front,
            panel,
        ),
    )
    .await {}
}

/// The board's own address, as the driver reports it.
fn hardware_octets<D: Driver>(device: &D) -> [u8; 6] {
    match device.hardware_address() {
        HardwareAddress::Ethernet(octets) => octets,
        // Not a wired frame: a board that wired a radio always answers with an Ethernet
        // address, so this arm exists because the enum says it can and not because a board
        // here reaches it. A name nobody can tell from another board's is the alternative,
        // and a name is all this is for.
        _ => [0; 6],
    }
}

/// The name the setup access point answers to, from the board's own address.
fn setup_ssid(octets: [u8; 6]) -> Ssid {
    iot_core::provision::ap_name(octets)
}

/// The link's own loop: associate, take an address, report the window, and start over.
///
/// Which of the three credential paths this is decides how a refusal is treated, so it is decided
/// here and handed down rather than read again below.
async fn link<S, R>(
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
            *setup.front = front_after_link(start, *setup.front, &held);
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

/// Whether this device keeps a network to go back to, which is what makes setup mode's `CANCEL` a
/// way out rather than a door that opens onto the same room.
fn has_saved<R: FlashRegion>(saved: &Option<Box<Store<R>>>) -> bool {
    keeps_any(saved.as_deref().map(|store| store.networks()))
}

/// The network reconnect tries first, or `None` when there is nothing saved.
fn first_saved<R: FlashRegion>(saved: &Option<Box<Store<R>>>) -> Option<(Credentials, Provenance)> {
    saved
        .as_ref()
        .and_then(|store| store.networks().iter().next())
        .map(|credentials| (credentials, Provenance::Stored))
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
        *setup.front = front_after_link(start, *setup.front, &ended);
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
enum Provenance {
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
enum Hold {
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

/// Which front the next stretch of setup mode opens on. A leave or a pick at the panel opens
/// there; a refusal keeps the front the credential was typed on. That is why the front is carried
/// across one stretch rather than recomputed.
fn front_after_link(start: Front, current: Front, ended: &Hold) -> Front {
    match ended {
        Hold::Named(..) | Hold::Left => start,
        Hold::Refused => current,
    }
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

/// Bring the link up with one credential and hold it there. A link that drops is reconciled here,
/// because the credential was right; only a refusal leaves. An outside credential is written once,
/// not per association, and a stored one not at all; see [`Provenance`].
async fn hold<S, R>(
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

/// What an unconfigured device does: wait to be set up from the panel or a phone, with the survey
/// underneath filling the list both front ends show.
///
/// Returns only when the panel closes setup mode; every other way out is another round. The caller
/// owns the network to go back to.
async fn unconfigured<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    saved: &mut Option<Box<Store<R>>>,
    first: Option<(Credentials, Provenance)>,
    cancellable: bool,
    setup: &mut Setup<'_>,
) where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    // In hand after a refusal that came back, or from a boot credential the panel superseded, or
    // waiting on the plane. Held rather than re-taken, because a credential that arrives while a
    // link is being held is returned by [`hold`] rather than put back on a one-slot plane.
    let mut pending = first;
    // The list as the page has it, carried from round to round: what keeps a row still while the
    // readings move under it, and what a save or a forget while a link is up puts the store's own
    // marks back on. It outlives the `hold` calls below.
    //
    // Boxed for the reason the credential store is: it is seven hundred bytes of entries,
    // readings, marks and the last list published, and every frame that carries it by value is a
    // fixed static's worth of the product's one task (`@/records/iot/wifi.md`).
    let mut history = Box::new(SurveyHistory::new());
    loop {
        // A credential already in hand means there is nothing to configure: the access point is
        // not raised at all.
        let (credentials, origin) = match pending.take() {
            Some(chosen) => chosen,
            None => {
                match configure(station, &mut history, saved, cancellable, setup).await {
                    Some(chosen) => chosen,
                    // The panel closed setup mode. There is nothing here to join, and what to do
                    // about that is the caller's: it is the one holding a network this device
                    // already knows.
                    None => return,
                }
            }
        };
        log::info!(
            "[WIFI] joining the network setup named: {:?}",
            credentials
                .ssid
                .as_display_text()
                .unwrap_or("<unprintable>"),
        );
        // Before the association that credentials this link, and not after: a leave outstanding
        // from the link before this one would otherwise take effect the moment this one came up,
        // which is a device that joins a network and leaves it at once. A setup request is the same
        // kind of stale command in the other direction, and it is dropped here for the same reason.
        LEAVE.clear();
        SETUP_ASK.clear();
        match hold(
            station,
            stack,
            &credentials,
            origin,
            Refusal::GiveBack,
            saved.as_deref_mut(),
            Some(&mut history),
        )
        .await
        {
            Hold::Named(next, origin) => pending = Some((*next, origin)),
            Hold::Refused | Hold::Left => {}
        }
    }
}

/// One stretch of setup mode, one front at a time: the panel's list, or the access point's page
/// with the list behind a box. `front` decides, and the strip switches; both are never up,
/// because they are two answers to one question and the radio is one radio.
///
/// The two ends are raced, not joined: whichever answers first ends the mode, drops the page and
/// its access point, and gives the credential back to the caller. A page that never resolves is
/// the contract — it exists to serve while the device is unconfigured. `None` is `CANCEL`.
///
/// Resources are re-borrowed each entry, not taken: every refused credential starts the same few
/// hundred bytes again.
async fn configure<S, R>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: &mut Option<Box<Store<R>>>,
    cancellable: bool,
    setup: &mut Setup<'_>,
) -> Option<(Credentials, Provenance)>
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let setup_name = setup.name;
    let resources = &mut *setup.resources;
    let front = &mut *setup.front;
    let panel = setup.panel;
    let secure_on_entry = setup.secure_on_entry;
    // A request that predates this stretch would act on it before the loop reads it.
    SETUP_ASK.clear();
    loop {
        // A request that outlived the round it was about would act on this one before it was
        // read; one aimed at the other front is answered there, not here.
        SETUP_ASK.clear();
        match *front {
            Front::Panel => {
                // No access point, so nothing to announce about one: the mode is the list and the
                // header, and the header is drawn from this publish.
                let face = Face {
                    front: Front::Panel,
                    cancellable,
                    setup: true,
                    browsing: false,
                };
                publish(face.status(station.status()), has_saved(saved));
                match select(
                    setup_credential(station, history, saved, face, false),
                    SETUP_ASK.ready(),
                )
                .await
                {
                    // `None` from the survey is the browsing end's answer, which this arm cannot
                    // give: a mode that is on stays on until the panel says otherwise.
                    Either::First(chosen) => return chosen,
                    // Asked on the front end that is no longer armed — the tap that raised the
                    // access point, made again while the box was still coming up. It is about
                    // work already done, and answering it would close what it opened.
                    Either::Second(ask) if !ask.applies_to(Front::Panel) => {
                        log::info!(
                            "[WIFI] dropped a setup request made on the access point's front; the box is \
                             not open"
                        );
                    }
                    Either::Second(SetupAsk::Front { .. }) => *front = Front::AccessPoint,
                    // Nothing on this front can ask it: the box it is asked in belongs to the
                    // other one.
                    Either::Second(SetupAsk::Pass { .. }) => {}
                    Either::Second(SetupAsk::Cancel { .. }) => {
                        log::info!("[WIFI] the panel closed setup mode; the list goes down");
                        return None;
                    }
                }
            }
            // The entry default: open from the splash hold, secured from a running panel. The
            // box's `PASS`/`NO PASS` flips this visit only.
            Front::AccessPoint => {
                let mut secured = secure_on_entry;
                loop {
                    let device = match station.begin_setup(setup_name, secured) {
                        Ok(device) => device,
                        Err(error) => {
                            log::error!(
                                "[WIFI] the setup access point would not come up: {error:?}"
                            );
                            park().await
                        }
                    };
                    let seed = seed_from(hardware_octets(&device));
                    let (stack, runner) =
                        embassy_net::new(device, crate::provision::setup_config(), resources, seed);
                    log::info!(
                        "[WIFI] setup mode for {:?} is open, {}",
                        setup_name.as_display_text().unwrap_or("<not drawable>"),
                        if secured {
                            "under a passphrase"
                        } else {
                            "open"
                        },
                    );
                    // What the panel draws for as long as the access point is up: the mode, the
                    // front it is up for, the passphrase it answers with and the address the page
                    // is served at. Reported here rather than by a window, because this is the only
                    // thing that knows the mode is on — and the caller's own link is gone by now.
                    let face = Face {
                        front: Front::AccessPoint,
                        cancellable,
                        setup: true,
                        browsing: false,
                    };
                    publish(face.status(station.status()), has_saved(saved));

                    // The page's future is boxed rather than held inline, and that is a memory
                    // decision rather than a convenience: it is four server loops with their socket
                    // buffers, several kilobytes of frame, and this whole chain of futures lives
                    // inside the product's one task — whose pool is a fixed static, and that static
                    // is the scarce resource on the S3 (see `@/records/iot/wifi.md`). The heap it
                    // moves to is PSRAM on the S3 and spare DRAM on the C6, and one allocation for a
                    // loop that ends only when it is dropped is the cheapest thing on offer here.
                    let page = Box::pin(crate::provision::serve(
                        stack,
                        runner,
                        MAC.lock(|cell| cell.get()),
                    ));
                    // What the heap has left once setup mode's own frame is on it. This is the
                    // number that says whether a build's budget was the right one, and the one
                    // every request the page answers has to fit in — an allocation that fails on
                    // this part is the device's death rather than a bad answer
                    // (`@/records/iot/wifi.md`).
                    log::info!("[HEAP] {} B free with setup mode's future up", heap_free());
                    let ending = select4(
                        setup_credential(station, history, saved, face, false),
                        page,
                        SETUP_ASK.ready(),
                        page_idle(),
                    )
                    .await;
                    // Down before the station is asked to associate, and not after: the two roles
                    // share the radio, and a page left up across the association would be a phone
                    // still joined to an access point the device has walked away from. The losing
                    // future — the page's, or the panel's — is dropped with the `select` above, so
                    // the access point's runner is already gone by here and this releases the last
                    // thing holding the radio's other mode.
                    if let Err(error) = station.end_setup() {
                        log::warn!("[WIFI] the setup access point would not come down: {error:?}");
                    }
                    match ending {
                        Either4::First(chosen) => return chosen,
                        // Unreachable: the page's own future never resolves.
                        Either4::Second(never) => match never {},
                        // Asked before this front end was up: the tap that switched here, made
                        // again in the second the access point takes to come up. Answering it
                        // would be the box opening and closing at once.
                        Either4::Third(ask) if !ask.applies_to(Front::AccessPoint) => {
                            log::info!(
                                "[WIFI] dropped a setup request made on the panel's front; the \
                                 access point stays up"
                            );
                        }
                        // Back to the panel: the access point a phone can see is one somebody has
                        // to ask for, and this is the asking.
                        Either4::Third(SetupAsk::Front { .. }) => {
                            log::info!(
                                "[WIFI] the panel took its own front back; the access point goes down"
                            );
                            *front = Front::Panel;
                            break;
                        }
                        // The same access point, open or under its passphrase again: a phone that
                        // was already told which network to join has to join it again. The target
                        // comes from the request rather than from a flip, so a duplicate cannot
                        // undo the answer it repeats.
                        Either4::Third(SetupAsk::Pass { open, .. }) => {
                            secured = !open;
                            log::info!(
                                "[WIFI] the setup access point is being put up again, {}",
                                if secured {
                                    "under a passphrase"
                                } else {
                                    "open"
                                }
                            );
                        }
                        Either4::Third(SetupAsk::Cancel { .. }) => {
                            log::info!(
                                "[WIFI] the panel closed setup mode; the access point goes down"
                            );
                            return None;
                        }
                        // Nobody came: an access point left up is one anybody in range can
                        // configure. What it goes back to is the front end this board still has —
                        // the panel on a board with one, and on a board with no screen the network
                        // this device keeps, which is the only other way to reach it.
                        Either4::Fourth(()) if panel => {
                            log::info!(
                                "[WIFI] nobody read the page in {} s; the access point goes down",
                                SETUP_IDLE_MS / 1_000
                            );
                            *front = Front::Panel;
                            break;
                        }
                        Either4::Fourth(()) => {
                            log::info!(
                                "[WIFI] nobody read the page in {} s; the access point goes down \
                                 and the saved network is tried again",
                                SETUP_IDLE_MS / 1_000
                            );
                            // The access point is gone, so a lamp still reading the last
                            // `setup` report would stay blue about a mode that is no longer
                            // on. This is the last word this path publishes: a board with no
                            // panel and nothing saved waits in `browsing` without scanning.
                            publish(station.status(), has_saved(saved));
                            return None;
                        }
                    }
                }
            }
        }
    }
}

/// How long the setup access point stays up with nobody reading its page, counted from the last
/// request. It bounds the one cost the panel's own front does not have: a network anyone in range
/// can join.
const SETUP_IDLE_MS: u64 = 180_000;

/// Resolve when the setup access point has gone `SETUP_IDLE_MS` without a page request. The clock
/// always runs, so a factory device does not beacon until somebody finds it; setup mode is only
/// entered by an explicit ask, so there is always a way back in.
async fn page_idle() {
    loop {
        match select(
            Timer::after(Duration::from_millis(SETUP_IDLE_MS)),
            crate::provision::page_read(),
        )
        .await
        {
            Either::First(()) => return,
            Either::Second(()) => {}
        }
    }
}

/// The other end of setup mode: the radio's survey and the list, until a credential is named.
///
/// The panel's front surveys even when nobody is looking, and arriving at its page starts a round
/// over; a level check is used rather than `wait_for_list`, which would park on a board with no
/// page. The access point's front scans only when the phone asks, because a round takes the access
/// point off the air.
async fn setup_credential<S, R>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: &mut Option<Box<Store<R>>>,
    face: Face,
    stop_when_hidden: bool,
) -> Option<(Credentials, Provenance)>
where
    S: Station,
    R: FlashRegion,
{
    // Rounds in a row that found nothing, which is what tells a bench hiccup from a place with
    // nothing in it; cleared by a credential coming back.
    let mut empty_rounds: u8 = 0;
    // Rounds in a row nobody has acted on, which is what lengthens the pause: a list nobody has
    // touched is still the truth about the room.
    let mut idle_rounds: u8 = 0;
    // Whether the panel was on the list's page last time round, so that arriving at it is a
    // change to be acted on rather than a state it is in.
    let mut panel_was_showing = false;
    loop {
        // The browser's end: the page is the survey's only reason to run, so the round that was
        // cancelled by the page leaving does not start again until it is back.
        if stop_when_hidden && !crate::render::PAGE.shows_list() {
            return None;
        }
        if let Some(chosen) = JOIN
            .take()
            .and_then(|asked| asked_for(asked, saved.as_deref()))
        {
            return Some(chosen);
        }
        let showing = crate::render::PAGE.shows_list();
        if showing && !panel_was_showing {
            // Arriving is an act, and the list shown on arrival should be the room as it is rather
            // than what the clock had worked its way up to.
            empty_rounds = 0;
            idle_rounds = 0;
            history.clear();
        }
        panel_was_showing = showing;
        // A forget lands in the rest between rounds and is acted on rather than held: it is about
        // the part, and a list that says a network is saved while it is not is the one thing this
        // path must not leave standing.
        match survey(
            station,
            history,
            saved.as_deref().map(|store| store.networks()),
            &mut empty_rounds,
            &mut idle_rounds,
            face,
        )
        .await
        {
            Rest::Forget(ssid) => {
                drop_saved(saved.as_deref_mut(), &ssid);
                remark(history, saved.as_deref());
            }
            Rest::Left if stop_when_hidden => return None,
            Rest::Left | Rest::Woken => {}
        }
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
    /// The panel left the list's page. Only the browsing caller acts on it: in setup mode the
    /// list is behind a box the person may step away from without closing the mode.
    Left,
}

/// One survey round: look, publish through `history`, and rest.
///
/// Never ends; a credential or a "look now" cuts the rest short. `history` keeps rows aimable
/// against radio jitter, and an unchanged list is not republished — a redraw here is a whole frame.
async fn survey<S>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: Option<&SavedNetworks>,
    empty_rounds: &mut u8,
    idle_rounds: &mut u8,
    face: Face,
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
            // leave the page a round behind what it is showing. The mode is this loop's own
            // premise — it is only ever reached with the access point up — so it is said here
            // rather than left to a station that does not know about it.
            publish(face.status(station.status()), keeps_any(saved));
            if let Some(list) = history.observe(&ranked, saved) {
                crate::render::SCAN.publish(&list);
                // The page's copy of the same list: the plane above is taken by the render
                // loop, and the page is a reader that must not take it.
                crate::provision::publish_scan(&list);
            } else {
                log::info!("[WIFI] the list is unchanged, so the page is not redrawn");
            }
            empty
        }
        Err(error) => {
            log::error!("[WIFI] the radio would not scan: {error:?}");
            publish(face.status(station.status()), keeps_any(saved));
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
    // is what waits it out. `None` is the access point's front, and it is not a pause of zero: no clock
    // asks for a round at all, so what is waited on is the page's own request.
    let pause = survey_pause_after(face.front, *empty_rounds, *idle_rounds);
    loop {
        let clock = async {
            match pause {
                Some(ms) => Timer::after(Duration::from_millis(ms)).await,
                None => pending().await,
            }
        };
        match select6(
            clock,
            JOIN.ready(),
            // Woken when the panel is put *back* on the list, so a page entered during a pause that has
            // grown to half a minute surveys at once. Not the level check: this pause is spent with the
            // list up, so that would be true on the first poll and the pause would be skipped every
            // time. The browsing end is the opposite question — the page leaving is what ends its
            // survey — and the two are one future because the face already says which end this is.
            async {
                if face.setup {
                    crate::render::PAGE.wait_for_return().await;
                } else {
                    crate::render::PAGE.wait_for_leave().await;
                }
            },
            // And woken by the strip's own "look around now", which is the same round this pause would
            // have started once it ran out: the panel asks because a person is looking at a list that
            // was read before they got there.
            RESCAN.ready(),
            FORGET.ready(),
            // A phone reading the page is the one thing a round must not interrupt: the scan is what
            // takes the access point off the air for its length, and a load landing in it is a load
            // the phone reports as no page at all. The same pause starts over.
            crate::provision::page_read(),
        )
        .await
        {
            Either6::Third(()) if !face.setup => return Rest::Left,
            Either6::Fifth(ssid) => return Rest::Forget(ssid),
            Either6::Sixth(()) => {}
            _ => return Rest::Woken,
        }
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
///
/// `saved` is the store's answer to "does this device keep a network", which the radio knows
/// nothing about; it rides here because every reader of the link is also a reader of that.
fn publish(status: LinkStatus, saved: bool) {
    let status = LinkStatus { saved, ..status };
    let _ = INTENT_BUS.try_send(Intent::Business(BusinessIntent::LinkReport(status)));
}

/// Whether the store holds at least one network.
fn keeps_any(saved: Option<&SavedNetworks>) -> bool {
    saved.is_some_and(|networks| !networks.is_empty())
}

/// Write the network the link just joined to the part it is kept in, whichever path named it.
/// Best effort: the link is up either way, and a failure only costs the next boot's copy.
fn keep<R: FlashRegion>(store: &mut Store<R>, credentials: &Credentials, hint: LinkHint) {
    match store.remember_with_hint(credentials, hint) {
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

/// Put the store's own marks back on the list the page is showing. The store is the authority, so a
/// row says what the next boot will do; a board with no partition is a no-op. The last list is used
/// rather than a new round, and equal marks publish nothing.
fn remark<R: FlashRegion>(history: &mut SurveyHistory, saved: Option<&Store<R>>) {
    if let Some(list) = history.marked(saved.map(|store| store.networks())) {
        crate::render::SCAN.publish(&list);
        crate::provision::publish_scan(&list);
    }
}

/// The seed the stack takes for the DHCP transaction id and its ephemeral ports: the address
/// differs between devices, the uptime between boots. There is no RNG call here because the chip's
/// is behind the chip layer, which this module does not compile.
fn seed_from(octets: [u8; 6]) -> u64 {
    let address = octets
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
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

/// What an arm with nothing to do waits on. Never returns: the alternative is an arm that
/// falls through, which would leave the product with a task that has quietly stopped.
async fn park() -> ! {
    pending().await
}
