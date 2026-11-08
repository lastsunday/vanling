//! The credential rounds: what to join, what a refusal means, and the association that holds
//! it.

use alloc::boxed::Box;

use crate::state::{NetworkCommand, Page};
use embassy_futures::select::{Either, select};
use embassy_net::Stack;
use embassy_net::driver::Driver;
use embassy_time::{Duration, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    ADDRESS_LOST_WINDOWS, AddressWatch, Credentials, Ipv4, LinkHint, LinkPhase, NetworkMode,
    ScanHistory, SetupHost, Station, same_network, setup_ap_starts_secured,
};

use super::setup::{Choice, SetupMode, await_network, pick, resolve_join};
use super::store::{any, first_saved, forget, remark_and_publish, save};
use super::{NETWORK, discard_setup, park, publish_link};
use crate::bench::replay::build_credentials;
use crate::runtime::workers::NetworkInbox;

const WINDOW_MS: u64 = 2_000;

const ADDRESS_BUDGET_MS: u64 = 30_000;

const RETRY_MS: u64 = 5_000;

/// Where a credential came from. What a refusal means follows from it: a network this build was
/// named is retried rather than handed back to setup, and only the ones that came from
/// somewhere else are written to the part once they work.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Stored,
    Build,
    Typed,
}

/// The splash's verdict, taken from the same queue as everything else the panel asks for: the
/// coordinator sends it once the splash is over. Nothing else is meaningful before then, so
/// anything else that reached the queue first is dropped.
async fn splash_setup(commands: &NetworkInbox) -> bool {
    loop {
        match commands.receive().await {
            NetworkCommand::SplashOver { setup } => return setup,
            other => log::debug!("[WIFI] dropping {other:?} before the splash ends"),
        }
    }
}

pub(super) async fn link<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    saved: Option<Box<Store<R>>>,
    mut setup: SetupMode<'_>,
) -> !
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let mut saved = saved;
    let entry_host = setup.host;

    let from_splash = splash_setup(NETWORK.commands).await;
    setup.secure_on_entry = setup_ap_starts_secured(setup.has_pages, from_splash);
    if from_splash {
        log::info!(
            "[WIFI] the splash asked for setup mode; the access point is the host it opens on"
        );
        setup.host = SetupHost::AccessPoint;
        let cancellable = any(saved.as_deref());
        await_network(station, stack, &mut saved, None, cancellable, &mut setup).await;
        log::info!("[WIFI] setup mode is over; this device goes back to its own page");
        setup.secure_on_entry = setup_ap_starts_secured(setup.has_pages, false);
    }

    // The network this build was named is joined before the saved list and is never handed back
    // to setup: it is what the build says this device runs on.
    let build = build_credentials().and_then(|(ssid, password)| {
        match Credentials::from_text(ssid, password) {
            Ok(credentials) => {
                log::info!("[WIFI] this build joins the network named \"{ssid}\"");
                Some((credentials, Origin::Build))
            }
            Err(error) => {
                log::error!("[WIFI] the credentials this build was given are unusable: {error:?}");
                None
            }
        }
    });

    loop {
        let (credentials, mut origin) = match build {
            Some(chosen) => chosen,
            None => match first_saved(&saved) {
                Some(chosen) => chosen,
                None if !setup.has_pages => {
                    log::info!("[WIFI] no page surface and no saved network; waiting offline");
                    park().await
                }
                None => {
                    log::info!(
                        "[WIFI] this build was given no network and this device has saved none; it \
                         runs offline, and the panel's page names one"
                    );
                    match Box::pin(browse(station, &mut saved)).await {
                        Some(chosen) => chosen,
                        None => {
                            log::info!(
                                "[WIFI] the panel asked for the access point; setup mode opens on it"
                            );
                            setup.host = SetupHost::AccessPoint;
                            let cancellable = any(saved.as_deref());
                            await_network(
                                station,
                                stack,
                                &mut saved,
                                None,
                                cancellable,
                                &mut setup,
                            )
                            .await;
                            log::info!(
                                "[WIFI] setup mode is over; this device goes back to its page"
                            );
                            continue;
                        }
                    }
                }
            },
        };
        let mut credentials = Box::new(credentials);
        // One credential at a time: a new one replaces the one in hand rather than waiting for
        // the next round.
        let stopped = loop {
            if origin == Origin::Stored {
                log::info!(
                    "[WIFI] this device saved a network; joining \"{}\"",
                    credentials
                        .ssid
                        .as_display_text()
                        .unwrap_or("<unprintable>"),
                );
            }
            match hold(
                station,
                stack,
                credentials.as_ref(),
                origin,
                saved.as_deref_mut(),
                None,
            )
            .await
            {
                Outcome::Next(next, next_origin) => {
                    credentials = next;
                    origin = next_origin;
                }
                Outcome::Refused => break Stopped::Refused,
                Outcome::Left => break Stopped::Left,
            }
        };
        match stopped {
            Stopped::Left => {
                setup.host = entry_host;
                log::info!(
                    "[WIFI] the panel left the link; setup mode opens on the {:?} host",
                    setup.host,
                );
            }
            Stopped::Refused => log::warn!(
                "[WIFI] the saved network would not admit this device; the panel is what names \
                 the next one"
            ),
        }
        let cancellable = any(saved.as_deref());
        await_network(station, stack, &mut saved, None, cancellable, &mut setup).await;
        log::info!("[WIFI] setup mode is over; this device goes back to the network it keeps");
    }
}

/// How a hold ended for the outer loop: the two endings that leave the credential behind.
enum Stopped {
    Refused,
    Left,
}

async fn browse<S, R>(
    station: &mut S,
    saved: &mut Option<Box<Store<R>>>,
) -> Option<(Credentials, Origin)>
where
    S: Station,
    R: FlashRegion,
{
    discard_setup(NETWORK.commands);
    let mut history = Box::new(ScanHistory::new());
    loop {
        NETWORK.page.wait_for(Page::Network).await;
        match pick(station, &mut history, saved, NetworkMode::Browsing).await {
            Choice::Network(credentials, origin) => return Some((credentials, origin)),
            Choice::SwitchHost => {
                log::info!("[WIFI] the panel asked for the access point from the list");
                return None;
            }
            Choice::Hidden => {
                log::info!("[WIFI] the panel left the list; the scan stops until it is back");
            }
            // Browsing is not setup, so neither can be asked for here.
            Choice::Cancel | Choice::SetupOpen(_) => {}
        }
    }
}

/// How one association ended: the panel named another network, the credential was refused, or
/// the panel left the link.
pub(super) enum Outcome {
    Next(Box<Credentials>, Origin),
    Refused,
    Left,
}

/// Holds one association until the panel or the radio ends it.
///
/// A hinted association is tried first — the channel it was last seen on — and a hint that
/// fails is dropped rather than retried, so the next attempt walks every channel.
pub(super) async fn hold<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    credentials: &Credentials,
    origin: Origin,
    mut saved: Option<&mut Store<R>>,
    mut history: Option<&mut ScanHistory>,
) -> Outcome
where
    S: Station,
    R: FlashRegion,
{
    let mut written = false;
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
            report(station, saved.as_deref());
            if origin != Origin::Build {
                return Outcome::Refused;
            }
            Timer::after(Duration::from_millis(RETRY_MS)).await;
            continue;
        }
        report(station, saved.as_deref());
        if let Err(error) = station.await_connected().await {
            if hint.take().is_some() {
                log::warn!(
                    "[WIFI] the remembered access point did not answer ({error:?}); scanning every \
                     channel"
                );
                continue;
            }
            log::error!("[WIFI] the radio would not associate: {error:?}");
            report(station, saved.as_deref());
            if origin != Origin::Build {
                return Outcome::Refused;
            }
            Timer::after(Duration::from_millis(RETRY_MS)).await;
            continue;
        }
        let found = station
            .status()
            .ap
            .map(|ap| LinkHint::new(ap.bssid, ap.channel))
            .filter(|hint| hint.is_known());
        hint = found;
        let mut saved_now = false;
        if let Some(store) = saved.as_deref_mut() {
            if origin != Origin::Stored && !written {
                written = true;
                saved_now = true;
                save(store, credentials, found.unwrap_or(LinkHint::NONE));
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
        if saved_now {
            remark_and_publish(history.as_deref_mut(), saved.as_deref());
        }

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
        report(station, saved.as_deref());
        if address.is_none() {
            log::error!("[WIFI] no address within {ADDRESS_BUDGET_MS} ms");
        }

        let mut lost = AddressWatch::new(ADDRESS_LOST_WINDOWS);
        let mut last_seen: Option<(LinkPhase, Option<Ipv4>)> = None;
        loop {
            match select(
                Timer::after(Duration::from_millis(WINDOW_MS)),
                NETWORK.commands.receive(),
            )
            .await
            {
                Either::Second(NetworkCommand::Join(asked)) => {
                    if let Some((next, origin)) = resolve_join(asked, saved.as_deref()) {
                        if same_network(&next, credentials) {
                            log::debug!("[WIFI] a join for the network already held was ignored");
                            continue;
                        }
                        log::info!("[WIFI] the panel named another network; leaving this one");
                        let _ = station.disconnect().await;
                        return Outcome::Next(Box::new(next), origin);
                    }
                }
                Either::Second(NetworkCommand::Leave) => {
                    log::info!("[WIFI] the panel asked to leave this network; the link is down");
                    let _ = station.disconnect().await;
                    report(station, saved.as_deref());
                    return Outcome::Left;
                }
                Either::Second(NetworkCommand::Forget(ssid)) => {
                    forget(saved.as_deref_mut(), history.as_deref_mut(), &ssid);
                    continue;
                }
                // A connected link has no setup to change and nothing to scan: those requests
                // belong to a phase that is not running.
                Either::Second(_) => {}
                Either::First(()) => {}
            }
            let address = stack_address(stack);
            station.note_address(address);
            let status = station.status();
            report(station, saved.as_deref());
            let seen = (status.phase, address);
            let level = if last_seen == Some(seen) {
                log::Level::Debug
            } else {
                last_seen = Some(seen);
                log::Level::Info
            };
            match address {
                Some(address) => {
                    let [a, b, c, d] = address.octets();
                    log::log!(level, "[WIFI] {:?}, address {a}.{b}.{c}.{d}", status.phase);
                    lost.observe(Some(address));
                    continue;
                }
                None => log::log!(
                    level,
                    "[WIFI] {:?}, no address for {} windows",
                    status.phase,
                    lost.missing() + 1,
                ),
            }
            if lost.observe(None) {
                log::warn!(
                    "[WIFI] no address for {ADDRESS_LOST_WINDOWS} windows; leaving and associating again"
                );
                break;
            }
        }

        let _ = station.disconnect().await;
        Timer::after(Duration::from_millis(RETRY_MS)).await;
    }
}

fn report<S: Station, R: FlashRegion>(station: &S, saved: Option<&Store<R>>) {
    publish_link(station.status(), any(saved));
}

fn stack_address(stack: &Stack<'_>) -> Option<Ipv4> {
    stack
        .config_v4()
        .map(|config| Ipv4::from_octets(config.address.address().octets()))
}

async fn wait_for_address(stack: &Stack<'_>) -> Option<Ipv4> {
    loop {
        if let Some(address) = stack_address(stack) {
            return Some(address);
        }
        stack.wait_config_up().await;
    }
}
