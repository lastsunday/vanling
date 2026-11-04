//! Setup mode: the panel's list and the access point's page, one host at a time.

use alloc::boxed::Box;

use crate::intent::{ActionIntent, Intent, NetworkPageIntent};
use crate::state::{DisplayPage, NetworkCommand};
use embassy_futures::select::{Either, Either3, select, select3};
use embassy_net::driver::Driver;
use embassy_net::{Stack, StackResources};
use embassy_time::{Duration, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    Credentials, JoinRequest, NetworkMode, ScanHistory, SetupHost, Ssid, Station,
};

use super::link::{Origin, Outcome, hold};
use super::scan::{Rest, scan};
use super::store::{any, forget};
use super::{
    NETWORK, SETUP_SOCKETS, discard_setup, heap_free, mac_of, park, provision, publish_link,
    seed_from,
};
use crate::input::INTENT_BUS;

/// Everything setup mode needs to run: one value, because a session on the panel and one on the
/// access point are the same session on two hosts.
pub(super) struct SetupMode<'d> {
    pub(super) name: &'d Ssid,
    pub(super) mac: [u8; 6],
    pub(super) resources: &'d mut StackResources<SETUP_SOCKETS>,
    pub(super) host: SetupHost,
    /// Whether the first access point this mode raises starts under a passphrase; the splash
    /// decides, so `serve` leaves it off and `link` sets it the moment the verdict lands.
    pub(super) secure_on_entry: bool,
    pub(super) has_pages: bool,
}

/// What one round of the panel's list came to.
pub(super) enum Choice {
    Network(Credentials, Origin),
    /// The panel left the list; browsing waits for it to come back.
    Hidden,
    /// The panel closed setup mode.
    Cancel,
    /// The panel asked for setup mode's other host.
    SwitchHost,
    /// The access point should be put up again, open (`true`) or under a passphrase.
    SetupOpen(bool),
}

/// Runs setup mode until someone names a network, or until the panel closes it.
pub(super) async fn await_network<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    saved: &mut Option<Box<Store<R>>>,
    first: Option<(Credentials, Origin)>,
    cancellable: bool,
    setup: &mut SetupMode<'_>,
) where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let mut pending = first;
    let mut history = Box::new(ScanHistory::new());
    loop {
        let (credentials, origin) = match pending.take() {
            Some(chosen) => chosen,
            None => match setup_session(station, &mut history, saved, cancellable, setup).await {
                Some(chosen) => chosen,
                None => return,
            },
        };
        log::info!(
            "[WIFI] joining the network setup named: \"{}\"",
            credentials
                .ssid
                .as_display_text()
                .unwrap_or("<unprintable>"),
        );
        match hold(
            station,
            stack,
            &credentials,
            origin,
            saved.as_deref_mut(),
            Some(&mut history),
        )
        .await
        {
            Outcome::Next(next, origin) => pending = Some((*next, origin)),
            Outcome::Refused | Outcome::Left => {}
        }
    }
}

async fn setup_session<S, R>(
    station: &mut S,
    history: &mut ScanHistory,
    saved: &mut Option<Box<Store<R>>>,
    cancellable: bool,
    setup: &mut SetupMode<'_>,
) -> Option<(Credentials, Origin)>
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    discard_setup(NETWORK.commands);
    loop {
        match setup.host {
            SetupHost::Panel => {
                let mode = NetworkMode::Setup {
                    host: SetupHost::Panel,
                    cancellable,
                };
                publish_link(station.status().with_mode(mode), any(saved.as_deref()));
                match pick(station, history, saved, mode).await {
                    Choice::Network(credentials, origin) => return Some((credentials, origin)),
                    Choice::SwitchHost => setup.host = SetupHost::AccessPoint,
                    Choice::Cancel => {
                        log::info!("[WIFI] the panel closed setup mode; the list goes down");
                        return None;
                    }
                    // The access point is not up on this host, so there is nothing to reopen.
                    Choice::SetupOpen(_) | Choice::Hidden => {}
                }
            }
            SetupHost::AccessPoint => {
                let mut secured = setup.secure_on_entry;
                loop {
                    let device = match station.begin_setup(setup.name, secured) {
                        Ok(device) => device,
                        Err(error) => {
                            log::error!(
                                "[WIFI] the setup access point would not come up: {error:?}"
                            );
                            park().await
                        }
                    };
                    let seed = seed_from(mac_of(&device));
                    let (stack, runner) = embassy_net::new(
                        device,
                        provision::setup_config(),
                        &mut *setup.resources,
                        seed,
                    );
                    log::info!(
                        "[WIFI] setup access point \"{}\" is up, {}",
                        setup.name.as_display_text().unwrap_or("<not drawable>"),
                        if secured {
                            "under a passphrase"
                        } else {
                            "open"
                        },
                    );
                    let mode = NetworkMode::Setup {
                        host: SetupHost::AccessPoint,
                        cancellable,
                    };
                    publish_link(station.status().with_mode(mode), any(saved.as_deref()));

                    // Boxed, not inline: four server loops with their socket buffers inside the
                    // product's one task, whose static pool is the scarce resource on the S3
                    // (`@/records/iot/wifi.md`). One allocation for a loop that ends on drop is
                    // the cheapest thing on offer.
                    let page_serve = Box::pin(provision::serve(stack, runner, setup.mac));
                    log::info!("[HEAP] {} B free with setup mode's future up", heap_free());
                    let ending =
                        select3(pick(station, history, saved, mode), page_serve, page_idle()).await;
                    if let Err(error) = station.end_setup() {
                        log::warn!("[WIFI] the setup access point would not come down: {error:?}");
                    }
                    match ending {
                        Either3::First(Choice::Network(credentials, origin)) => {
                            return Some((credentials, origin));
                        }
                        Either3::Second(never) => match never {},
                        Either3::First(Choice::SwitchHost) => {
                            log::info!(
                                "[WIFI] the panel took its own host back; the access point goes down"
                            );
                            setup.host = SetupHost::Panel;
                            break;
                        }
                        Either3::First(Choice::SetupOpen(open)) => {
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
                        Either3::First(Choice::Cancel) => {
                            log::info!(
                                "[WIFI] the panel closed setup mode; the access point goes down"
                            );
                            return None;
                        }
                        Either3::First(Choice::Hidden) => {}
                        Either3::Third(()) if setup.has_pages => {
                            log::info!(
                                "[WIFI] nobody read the page in {} s; the access point goes down",
                                SETUP_IDLE_MS / 1_000
                            );
                            setup.host = SetupHost::Panel;
                            break;
                        }
                        Either3::Third(()) => {
                            log::info!(
                                "[WIFI] nobody read the page in {} s; the access point goes down \
                                 and the saved network is tried again",
                                SETUP_IDLE_MS / 1_000
                            );
                            publish_link(station.status(), any(saved.as_deref()));
                            return None;
                        }
                    }
                }
            }
        }
    }
}

const SETUP_IDLE_MS: u64 = 180_000;

async fn page_idle() {
    loop {
        match select(
            Timer::after(Duration::from_millis(SETUP_IDLE_MS)),
            provision::page_read(),
        )
        .await
        {
            Either::First(()) => return,
            Either::Second(()) => {}
        }
    }
}

/// One round of the list, until the panel names something, leaves, or asks setup mode to
/// change. Every command taken here is either acted on or known to be stale.
pub(super) async fn pick<S, R>(
    station: &mut S,
    history: &mut ScanHistory,
    saved: &mut Option<Box<Store<R>>>,
    mode: NetworkMode,
) -> Choice
where
    S: Station,
    R: FlashRegion,
{
    let mut empty_rounds: u8 = 0;
    let mut idle_rounds: u8 = 0;
    let mut panel_was_showing = false;
    loop {
        let showing = NETWORK.page.shows(DisplayPage::Network);
        // Browsing ends when the panel leaves; setup keeps the scan up for the phone.
        if mode.is_browsing() && !showing {
            return Choice::Hidden;
        }
        if showing && !panel_was_showing {
            empty_rounds = 0;
            idle_rounds = 0;
            history.clear();
        }
        panel_was_showing = showing;
        match scan(
            station,
            history,
            saved.as_deref().map(|store| store.networks()),
            &mut empty_rounds,
            &mut idle_rounds,
            mode,
        )
        .await
        {
            Rest::Again => {}
            Rest::Left if mode.is_browsing() => return Choice::Hidden,
            Rest::Left => {}
            Rest::Command(command) => match command {
                NetworkCommand::Join(asked) => {
                    if let Some((credentials, origin)) = resolve_join(asked, saved.as_deref()) {
                        return Choice::Network(credentials, origin);
                    }
                }
                NetworkCommand::Forget(ssid) => {
                    forget(saved.as_deref_mut(), Some(history), &ssid);
                }
                NetworkCommand::SwitchHost { from } if mode.host() == Some(from) => {
                    return Choice::SwitchHost;
                }
                NetworkCommand::LeaveSetup { from }
                    if mode.is_setup() && mode.host() == Some(from) =>
                {
                    return Choice::Cancel;
                }
                NetworkCommand::SetupOpen { from, open }
                    if mode.host() == Some(SetupHost::AccessPoint)
                        && from == SetupHost::AccessPoint =>
                {
                    return Choice::SetupOpen(open);
                }
                // Made on the other host, or outside the mode it belongs to.
                _ => {}
            },
        }
    }
}

pub(super) fn resolve_join<R: FlashRegion>(
    asked: JoinRequest,
    saved: Option<&Store<R>>,
) -> Option<(Credentials, Origin)> {
    match asked {
        JoinRequest::Typed(credentials) => Some((credentials, Origin::Typed)),
        JoinRequest::Held(ssid) => {
            let copy = saved.and_then(|store| store.networks().copy_of(&ssid));
            if copy.is_none() {
                log::info!(
                    "[WIFI] nothing is kept for {:?}; the panel is what asks for a passphrase",
                    ssid.as_display_text().unwrap_or("<unprintable>"),
                );
                let _ = INTENT_BUS.try_send(Intent::Action(ActionIntent::NetworkPage(
                    NetworkPageIntent::NoCopy(ssid),
                )));
            }
            copy.map(|credentials| (credentials, Origin::Stored))
        }
    }
}
