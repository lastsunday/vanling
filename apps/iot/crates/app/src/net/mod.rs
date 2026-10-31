//! The link, beside the product's futures: the station's futures are not `Send`, so this is
//! all the stack one thread has.

use alloc::boxed::Box;
use core::cell::Cell;
use core::future::pending;

use embassy_futures::join::join;
use embassy_net::driver::{Driver, HardwareAddress};
use embassy_net::{Config, StackResources};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Instant;
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{ApList, Front, LinkStatus, Ssid, Station};
use iot_core::intent::{BusinessIntent, Intent, NetworkIntent};

use crate::input::INTENT_BUS;

mod link;
mod provision;
mod setup;
mod store;
mod survey;

const SOCKETS: usize = 1;

const SETUP_SOCKETS: usize = 2 + crate::net::provision::PAGE_SOCKETS;

static HEAP_FREE: HeapReport = Mutex::new(Cell::new(unreported));

static MAC: Mutex<CriticalSectionRawMutex, Cell<[u8; 6]>> = Mutex::new(Cell::new([0; 6]));

type HeapReport = Mutex<CriticalSectionRawMutex, Cell<fn() -> usize>>;

const fn unreported() -> usize {
    0
}

pub fn report_heap_free(report: fn() -> usize) {
    HEAP_FREE.lock(|slot| slot.set(report));
}

fn heap_free() -> usize {
    HEAP_FREE.lock(|slot| slot.get())()
}

pub async fn serve<S, R>(
    station: Option<Box<S>>,
    saved: Option<Box<Store<R>>>,
    front: Front,
    has_pages: bool,
    page: &'static crate::bridge::PageWatch,
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
    let setup_name = setup_ssid(octets);
    MAC.lock(|cell| cell.set(octets));
    let _ = INTENT_BUS.try_send(Intent::Business(BusinessIntent::Network(
        NetworkIntent::Mac(octets),
    )));
    publish(station.status(), store::has_saved(&saved));
    let resources = Box::leak(Box::new(StackResources::<SOCKETS>::new()));
    let setup_resources = Box::leak(Box::new(StackResources::<SETUP_SOCKETS>::new()));
    let (stack, mut runner) =
        embassy_net::new(device, Config::dhcpv4(Default::default()), resources, seed);
    log::info!("[WIFI] network stack up, DHCP requested");

    match join(
        runner.run(),
        link::link(
            &mut *station,
            &stack,
            saved,
            &setup_name,
            &mut *setup_resources,
            front,
            link::Panel { page, has_pages },
        ),
    )
    .await {}
}

fn hardware_octets<D: Driver>(device: &D) -> [u8; 6] {
    match device.hardware_address() {
        HardwareAddress::Ethernet(octets) => octets,
        _ => [0; 6],
    }
}

fn setup_ssid(octets: [u8; 6]) -> Ssid {
    iot_core::provision::ap_name(octets)
}

fn publish(status: LinkStatus, saved: bool) {
    crate::bridge::deliver(status, saved);
}

/// Hands one list to both consumers at once: the panel's state and setup mode's page. A
/// refresh that reached one and not the other would leave the phone and the panel showing
/// different lists.
pub(super) fn publish_scan(list: &ApList) {
    crate::bridge::SCAN.publish(list);
    provision::publish_scan(list);
}

fn seed_from(octets: [u8; 6]) -> u64 {
    let address = octets
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
}

async fn park() -> ! {
    pending().await
}
