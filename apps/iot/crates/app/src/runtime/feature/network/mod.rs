//! The link, beside the product's futures: the station's futures are not `Send`, so this is
//! all the stack one thread has.

use alloc::boxed::Box;
use core::cell::Cell;

use crate::shared::intent::{BusinessIntent, Intent, LinkIntent};
use embassy_futures::join::join;
use embassy_net::driver::{Driver, HardwareAddress};
use embassy_net::{Config, StackResources};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Instant;
use iot_bsp_esp::components::credentials::CredentialStore;
use iot_bsp_esp::components::wifi::EspStation;
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{LinkStatus, SetupHost, Station};

use crate::NetworkWorker;
use crate::runtime::feature::input::INTENT_BUS;
use crate::runtime::workers::NETWORK;

mod link;
mod provision;
pub mod safe;
mod scan;
mod setup;
mod store;
mod watch;

pub(crate) use watch::discard_setup;

const SOCKETS: usize = 1;

const SETUP_SOCKETS: usize = 2 + provision::PAGE_SOCKETS;

static HEAP_FREE: HeapReport = Mutex::new(Cell::new(unreported));

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

/// The worker as a task: `serve` is generic over the driver and a task may not be, so this is
/// where the concrete station and flash land. `host` stays an argument because the portal's
/// home is a board fact, and only the composition point knows which board it built.
#[embassy_executor::task]
pub async fn task(
    station: Box<EspStation>,
    saved: Option<Box<CredentialStore>>,
    host: SetupHost,
    has_pages: bool,
) -> ! {
    serve(station, saved, host, has_pages).await
}

/// The handle the composition root hands `run`: it starts the worker without naming the radio.
pub fn worker(
    station: Box<EspStation>,
    saved: Option<Box<CredentialStore>>,
    host: SetupHost,
    has_pages: bool,
) -> NetworkWorker {
    NetworkWorker::new(move |spawner| {
        log::info!("[WIFI] starting the network worker");
        spawner
            .spawn(task(station, saved, host, has_pages).expect("the network task fits its slot"));
    })
}

pub async fn serve<S, R>(
    mut station: Box<S>,
    saved: Option<Box<Store<R>>>,
    host: SetupHost,
    has_pages: bool,
) -> !
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let device = station
        .take_device()
        .expect("the station hands its interface over on the first ask");
    let mac = mac_of(&device);
    let seed = seed_from(mac);
    LINK.note_mac(mac);
    let name = iot_core::provision::setup_ap_ssid(crate::assets::AP_PREFIX, mac);
    let setup = setup::SetupMode {
        name: &name,
        mac,
        resources: Box::leak(Box::new(StackResources::<SETUP_SOCKETS>::new())),
        host,
        has_pages,
        // A board with a page can show the passphrase, so `link` secures every entry; a board
        // without one leaves it open. Publishing happens after that decision lands.
        secure_on_entry: false,
    };
    publish_link(station.status(), store::any(saved.as_deref()));
    let resources = Box::leak(Box::new(StackResources::<SOCKETS>::new()));
    let (stack, mut runner) =
        embassy_net::new(device, Config::dhcpv4(Default::default()), resources, seed);
    log::info!("[WIFI] network stack up, DHCP requested");
    // The radio and the IP stack are what the network half of a boot can prove. Association
    // and DHCP are policy on top: an AP that will not answer is a live device, not a crash.
    iot_core::boot::note_network_settled();

    match join(
        runner.run(),
        link::link(&mut *station, &stack, saved, setup),
    )
    .await {}
}

fn mac_of<D: Driver>(device: &D) -> [u8; 6] {
    match device.hardware_address() {
        HardwareAddress::Ethernet(octets) => octets,
        _ => [0; 6],
    }
}

/// The link the panel was last told about, and the address it has not been told yet.
///
/// A reading that repeats is not a report: the panel's state only moves on a difference, and
/// the intent bus also carries every command the other way. The address is separate because it
/// is news once, and a full bus must cost a retry rather than the address.
struct LinkWatch {
    last: Mutex<CriticalSectionRawMutex, Cell<Option<LinkStatus>>>,
    mac: Mutex<CriticalSectionRawMutex, Cell<Option<[u8; 6]>>>,
}

impl LinkWatch {
    const fn new() -> Self {
        Self {
            last: Mutex::new(Cell::new(None)),
            mac: Mutex::new(Cell::new(None)),
        }
    }

    fn note_mac(&self, mac: [u8; 6]) {
        self.mac.lock(|slot| slot.set(Some(mac)));
        self.flush_mac();
    }

    fn flush_mac(&self) {
        let Some(mac) = self.mac.lock(|slot| slot.get()) else {
            return;
        };
        if INTENT_BUS
            .try_send(Intent::Business(BusinessIntent::Link(LinkIntent::Mac(mac))))
            .is_ok()
        {
            self.mac.lock(|slot| slot.set(None));
        }
    }

    /// Delivers `status` with the store's bit folded in, if it differs from the last
    /// delivery. The reading is only remembered once the bus took it, so a full bus retries
    /// rather than swallowing the change.
    fn deliver(&self, status: LinkStatus, saved: bool) {
        self.flush_mac();
        let status = LinkStatus { saved, ..status };
        if self.last.lock(|last| last.get()) == Some(status) {
            return;
        }
        if INTENT_BUS
            .try_send(Intent::Business(BusinessIntent::Link(LinkIntent::Status(
                status,
            ))))
            .is_ok()
        {
            self.last.lock(|last| last.set(Some(status)));
        }
    }
}

static LINK: LinkWatch = LinkWatch::new();

/// Reports the link to the panel. The network worker's whole view of the intent bus is this
/// call.
pub(super) fn publish_link(status: LinkStatus, saved: bool) {
    LINK.deliver(status, saved);
}

fn seed_from(mac: [u8; 6]) -> u64 {
    let address = mac
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
}

async fn park() -> ! {
    core::future::pending().await
}
