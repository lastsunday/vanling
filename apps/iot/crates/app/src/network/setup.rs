//! Setup mode: the panel's list and the access point's page, one front at a time.
//!
//! `unconfigured` is the mode's own loop, `configure` is one stretch of it, and `setup_credential`
//! is the survey end both fronts share. The mode ends only by returning a credential or `None`.

use alloc::boxed::Box;

use embassy_futures::select::{Either, Either4, select, select4};
use embassy_net::driver::Driver;
use embassy_net::{Stack, StackResources};
use embassy_time::{Duration, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    Credentials, Front, JoinRequest, LinkStatus, Ssid, Station, SurveyHistory,
};
use iot_core::intent::{BusinessIntent, Intent};

use super::link::{Hold, Provenance, Refusal, hold};
use super::store::{drop_saved, has_saved, remark};
use super::survey::{Rest, survey};
use super::{MAC, SETUP_SOCKETS, hardware_octets, heap_free, park, publish, seed_from};
use crate::input::INTENT_BUS;
use crate::render::{JOIN, LEAVE, SETUP_ASK, SetupAsk};

/// Setup mode's own state between rounds: the access point's name and sockets, which front is
/// up, and what this board can show.
///
/// One struct because `link` fills it, `named_by_build` and `unconfigured` pass it through, and
/// `configure` is the only reader.
pub(super) struct Setup<'d> {
    /// The access point's name, derived from the board's own address.
    pub(super) name: &'d Ssid,
    /// The sockets its page is served from, re-borrowed every time the access point is raised.
    pub(super) resources: &'d mut StackResources<SETUP_SOCKETS>,
    /// The front offering the mode. It survives a refusal — the page that was typed on should
    /// still be serving — but not a leave or a row picked at the panel; see
    /// [`iot_core::drivers::network::front_after_link`].
    pub(super) front: &'d mut Front,
    /// Whether this build can show a passphrase at all.
    pub(super) panel: bool,
    /// Whether the next access point starts secured; [`setup_ap_starts_secured`] is the rule.
    /// `configure` reads this once per raise, so a box's answer lasts for that visit only.
    pub(super) secure_on_entry: bool,
}

/// What setup mode is being offered with: the front, and whether there is a way back.
///
/// [`Station::status`] knows neither, so every publish carries this alongside the radio's own
/// [`LinkStatus`]. Without it the box over the list would close itself a window after opening,
/// and `CANCEL` would turn into `SCAN`.
#[derive(Clone, Copy)]
pub(super) struct Face {
    pub(super) front: Front,
    cancellable: bool,
    /// Whether this is setup mode. The page is drawn either way; only a mode says so.
    pub(super) setup: bool,
    /// Whether this is the offline list rather than a mode. The page offers `SCAN | AP`; a hold
    /// does nothing, and only `AP` leaves this end.
    browsing: bool,
}

impl Face {
    /// The link as this front end reports it.
    pub(super) fn status(self, station: LinkStatus) -> LinkStatus {
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
    pub(super) const fn browsing() -> Self {
        Self {
            front: Front::Panel,
            cancellable: false,
            setup: false,
            browsing: true,
        }
    }
}

/// What an unconfigured device does: wait to be set up from the panel or a phone, with the survey
/// underneath filling the list both front ends show.
///
/// Returns only when the panel closes setup mode; every other way out is another round. The caller
/// owns the network to go back to.
pub(super) async fn unconfigured<S, R>(
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
            "[WIFI] joining the network setup named: \"{}\"",
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
                        "[WIFI] setup access point \"{}\" is up, {}",
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
pub(super) async fn setup_credential<S, R>(
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

pub(super) fn asked_for<R: FlashRegion>(
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
