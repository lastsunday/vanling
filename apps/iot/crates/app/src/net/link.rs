//! The credential rounds: what to join, what a refusal means, and the association that
//! holds it.

use alloc::boxed::Box;

use embassy_futures::select::{Either, Either4, select, select4};
use embassy_net::Stack;
use embassy_net::StackResources;
use embassy_net::driver::Driver;
use embassy_time::{Duration, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    ADDRESS_LOST_WINDOWS, AddressWatch, Credentials, Front, Ipv4, LinkEnding, LinkHint, LinkPhase,
    Ssid, Station, SurveyHistory, front_after_link, setup_ap_starts_secured,
};
use iot_core::state::DisplayPage;

use super::setup::{Face, Setup, asked_for, setup_credential, unconfigured};
use super::store::{drop_saved, first_saved, has_saved, keep, keeps_any, remark};
use super::{SETUP_SOCKETS, park, publish, publish_scan};
use crate::bridge::{FORGET, JOIN, LEAVE, SETUP_ASK};

const WINDOW_MS: u64 = 2_000;

const ADDRESS_BUDGET_MS: u64 = 30_000;

const RETRY_MS: u64 = 5_000;

/// The panel the link serves: whether it has pages at all, and which page it is on.
#[derive(Clone, Copy)]
pub(super) struct Panel {
    pub(super) page: &'static crate::bridge::PageWatch,
    pub(super) has_pages: bool,
}

pub(super) async fn link<S, R>(
    station: &mut S,
    stack: &Stack<'_>,
    saved: Option<Box<Store<R>>>,
    setup_name: &Ssid,
    setup_resources: &mut StackResources<SETUP_SOCKETS>,
    front: Front,
    panel: Panel,
) -> !
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let mut front = front;
    let start = front;
    let mut saved = saved;

    let landing = panel.page.wait_for_splash_end().await;
    let mut setup = Setup {
        name: setup_name,
        resources: setup_resources,
        front: &mut front,
        has_pages: panel.has_pages,
        secure_on_entry: setup_ap_starts_secured(panel.has_pages, landing),
        page: panel.page,
    };
    if landing {
        log::info!(
            "[WIFI] the splash asked for setup mode; the access point is the front it opens on"
        );
        *setup.front = Front::AccessPoint;
        let cancellable = has_saved(&saved);
        unconfigured(station, stack, &mut saved, None, cancellable, &mut setup).await;
        log::info!("[WIFI] setup mode is over; this device goes back to its own page");
        setup.secure_on_entry = setup_ap_starts_secured(panel.has_pages, false);
    }

    if let Some((ssid, password)) = crate::boot::wifi_credentials() {
        named_by_build(station, stack, ssid, password, saved, &mut setup).await
    }

    loop {
        let (credentials, mut origin) = match first_saved(&saved) {
            Some(chosen) => chosen,
            None if !panel.has_pages => {
                log::info!("[WIFI] no page surface and no saved network; waiting offline");
                core::future::pending().await
            }
            None => {
                log::info!(
                    "[WIFI] this build was given no network and this device has saved none; it runs \
                     offline, and the panel's page names one"
                );
                match Box::pin(browsing(station, &mut saved, panel.page)).await {
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
        let mut credentials = Box::new(credentials);
        // One credential at a time: a new one replaces the one in hand rather than waiting for
        // the next round.
        let ended = loop {
            if origin == Provenance::Stored {
                log::info!(
                    "[WIFI] this device saved a network; joining \"{}\"",
                    credentials
                        .ssid
                        .as_display_text()
                        .unwrap_or("<unprintable>"),
                );
            }
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
            Hold::Named(..) => {}
            Hold::Left => {
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

async fn browsing<S, R>(
    station: &mut S,
    saved: &mut Option<Box<Store<R>>>,
    page: &'static crate::bridge::PageWatch,
) -> Option<(Credentials, Provenance)>
where
    S: Station,
    R: FlashRegion,
{
    SETUP_ASK.clear();
    let mut history = Box::new(SurveyHistory::new());
    loop {
        page.wait_for(DisplayPage::Network).await;
        match select(
            setup_credential(station, &mut history, saved, Face::browsing(), true, page),
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
            Either::Second(_) => {}
        }
    }
}

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
    log::info!("[WIFI] this build joins the network named \"{ssid}\"");
    let mut saved = saved;
    let start = *setup.front;
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
            // Park rather than return: a task that fell through would be one that quietly stopped.
            Hold::Named(..) | Hold::Refused => park().await,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Provenance {
    Named,
    Stored,
}

pub(super) enum Hold {
    Named(Box<Credentials>, Provenance),
    Refused,
    Left,
}

impl Hold {
    const fn ending(&self) -> LinkEnding {
        match self {
            Self::Named(..) => LinkEnding::Named,
            Self::Refused => LinkEnding::Refused,
            Self::Left => LinkEnding::Left,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    Retry,
    GiveBack,
}

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
                if let Some(history) = history.as_deref_mut()
                    && let Some(list) = remark(history, saved.as_deref())
                {
                    publish_scan(&list);
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

        let mut lost = AddressWatch::new(ADDRESS_LOST_WINDOWS);
        let mut last_seen: Option<(LinkPhase, Option<Ipv4>)> = None;
        loop {
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
                    publish(
                        station.status(),
                        keeps_any(saved.as_deref().map(|store| store.networks())),
                    );
                    return Hold::Left;
                }
                Either4::Fourth(ssid) => {
                    drop_saved(saved.as_deref_mut(), &ssid);
                    if let Some(history) = history.as_deref_mut()
                        && let Some(list) = remark(history, saved.as_deref())
                    {
                        publish_scan(&list);
                    }
                    continue;
                }
                Either4::First(()) => {}
            }
            let address = stack_address(stack);
            station.note_address(address);
            let status = station.status();
            publish(
                status,
                keeps_any(saved.as_deref().map(|store| store.networks())),
            );
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
