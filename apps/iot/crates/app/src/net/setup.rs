//! Setup mode: the panel's list and the access point's page, one front at a time.

use alloc::boxed::Box;

use embassy_futures::select::{Either, Either4, select, select4};
use embassy_net::driver::Driver;
use embassy_net::{Stack, StackResources};
use embassy_time::{Duration, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{
    Credentials, Front, JoinRequest, LinkStatus, Ssid, Station, SurveyHistory,
};
use iot_core::intent::{BusinessIntent, Intent, NetworkIntent};
use iot_core::state::DisplayPage;

use super::link::{Hold, Provenance, Refusal, hold};
use super::store::{drop_saved, has_saved, remark};
use super::survey::{Rest, survey};
use super::{
    MAC, SETUP_SOCKETS, hardware_octets, heap_free, park, publish, publish_scan, seed_from,
};
use crate::bridge::{JOIN, LEAVE, SETUP_ASK, SetupAsk};
use crate::input::INTENT_BUS;

pub(super) struct Setup<'d> {
    pub(super) name: &'d Ssid,
    pub(super) resources: &'d mut StackResources<SETUP_SOCKETS>,
    pub(super) front: &'d mut Front,
    pub(super) has_pages: bool,
    pub(super) secure_on_entry: bool,
    pub(super) page: &'static crate::bridge::PageWatch,
}

#[derive(Clone, Copy)]
pub(super) struct Face {
    pub(super) front: Front,
    cancellable: bool,
    pub(super) setup: bool,
    browsing: bool,
}

impl Face {
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

    pub(super) const fn browsing() -> Self {
        Self {
            front: Front::Panel,
            cancellable: false,
            setup: false,
            browsing: true,
        }
    }
}

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
    let mut history = Box::new(SurveyHistory::new());
    loop {
        let (credentials, origin) = match pending.take() {
            Some(chosen) => chosen,
            None => match configure(station, &mut history, saved, cancellable, setup).await {
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
    let has_pages = setup.has_pages;
    let secure_on_entry = setup.secure_on_entry;
    let page = setup.page;
    SETUP_ASK.clear();
    loop {
        SETUP_ASK.clear();
        match *front {
            Front::Panel => {
                let face = Face {
                    front: Front::Panel,
                    cancellable,
                    setup: true,
                    browsing: false,
                };
                publish(face.status(station.status()), has_saved(saved));
                match select(
                    setup_credential(station, history, saved, face, false, page),
                    SETUP_ASK.ready(),
                )
                .await
                {
                    Either::First(chosen) => return chosen,
                    Either::Second(ask) if !ask.applies_to(Front::Panel) => {
                        log::info!(
                            "[WIFI] dropped a setup request made on the access point's front; the box is \
                             not open"
                        );
                    }
                    Either::Second(SetupAsk::Front { .. }) => *front = Front::AccessPoint,
                    Either::Second(SetupAsk::Pass { .. }) => {}
                    Either::Second(SetupAsk::Cancel { .. }) => {
                        log::info!("[WIFI] the panel closed setup mode; the list goes down");
                        return None;
                    }
                }
            }
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
                    let (stack, runner) = embassy_net::new(
                        device,
                        crate::net::provision::setup_config(),
                        resources,
                        seed,
                    );
                    log::info!(
                        "[WIFI] setup access point \"{}\" is up, {}",
                        setup_name.as_display_text().unwrap_or("<not drawable>"),
                        if secured {
                            "under a passphrase"
                        } else {
                            "open"
                        },
                    );
                    let face = Face {
                        front: Front::AccessPoint,
                        cancellable,
                        setup: true,
                        browsing: false,
                    };
                    publish(face.status(station.status()), has_saved(saved));

                    // Boxed, not inline: four server loops with their socket buffers inside the
                    // product's one task, whose static pool is the scarce resource on the S3
                    // (`@/records/iot/wifi.md`). One allocation for a loop that ends on drop is
                    // the cheapest thing on offer.
                    let page_serve = Box::pin(crate::net::provision::serve(
                        stack,
                        runner,
                        MAC.lock(|cell| cell.get()),
                    ));
                    log::info!("[HEAP] {} B free with setup mode's future up", heap_free());
                    let ending = select4(
                        setup_credential(station, history, saved, face, false, page),
                        page_serve,
                        SETUP_ASK.ready(),
                        page_idle(),
                    )
                    .await;
                    if let Err(error) = station.end_setup() {
                        log::warn!("[WIFI] the setup access point would not come down: {error:?}");
                    }
                    match ending {
                        Either4::First(chosen) => return chosen,
                        Either4::Second(never) => match never {},
                        Either4::Third(ask) if !ask.applies_to(Front::AccessPoint) => {
                            log::info!(
                                "[WIFI] dropped a setup request made on the panel's front; the \
                                 access point stays up"
                            );
                        }
                        Either4::Third(SetupAsk::Front { .. }) => {
                            log::info!(
                                "[WIFI] the panel took its own front back; the access point goes down"
                            );
                            *front = Front::Panel;
                            break;
                        }
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
                        Either4::Fourth(()) if has_pages => {
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
                            publish(station.status(), has_saved(saved));
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
            crate::net::provision::page_read(),
        )
        .await
        {
            Either::First(()) => return,
            Either::Second(()) => {}
        }
    }
}

pub(super) async fn setup_credential<S, R>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: &mut Option<Box<Store<R>>>,
    face: Face,
    stop_when_hidden: bool,
    page: &'static crate::bridge::PageWatch,
) -> Option<(Credentials, Provenance)>
where
    S: Station,
    R: FlashRegion,
{
    let mut empty_rounds: u8 = 0;
    let mut idle_rounds: u8 = 0;
    let mut panel_was_showing = false;
    loop {
        if stop_when_hidden && !page.shows(DisplayPage::Network) {
            return None;
        }
        if let Some(chosen) = JOIN
            .take()
            .and_then(|asked| asked_for(asked, saved.as_deref()))
        {
            return Some(chosen);
        }
        let showing = page.shows(DisplayPage::Network);
        if showing && !panel_was_showing {
            empty_rounds = 0;
            idle_rounds = 0;
            history.clear();
        }
        panel_was_showing = showing;
        match survey(
            station,
            history,
            saved.as_deref().map(|store| store.networks()),
            &mut empty_rounds,
            &mut idle_rounds,
            face,
            page,
        )
        .await
        {
            Rest::Forget(ssid) => {
                drop_saved(saved.as_deref_mut(), &ssid);
                if let Some(list) = remark(history, saved.as_deref()) {
                    publish_scan(&list);
                }
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
                let _ = INTENT_BUS.try_send(Intent::Business(BusinessIntent::Network(
                    NetworkIntent::NoCopy(ssid),
                )));
            }
            copy.map(|credentials| (credentials, Provenance::Stored))
        }
    }
}
