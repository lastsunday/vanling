//! The link, beside the product's futures: the station's futures are not `Send`, so this is
//! all the stack one thread has.

use alloc::boxed::Box;
use core::cell::Cell;

use crate::shared::intent::{BusinessIntent, Intent, LinkIntent, NetworkPageIntent};
use embassy_futures::join::join;
use embassy_net::driver::{Driver, HardwareAddress};
use embassy_net::{Config, StackResources};
use embassy_time::Instant;
use iot_bsp_esp::components::credentials::CredentialStore;
use iot_bsp_esp::components::wifi::EspStation;
use iot_core::boot::Readiness;
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{LinkStatus, SetupHost, Ssid, Station};

use crate::NetworkWorker;
use crate::runtime::feature::input::IntentBus;
use crate::runtime::workers::{Network, NetworkInbox, PageWatch, Scan};

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

/// What the entry hands the network worker: its own handles, plus the two facts only the
/// composition root knows — where readiness is reported, and how to read the heap.
#[derive(Clone, Copy)]
pub struct NetCtx {
    pub bus: &'static IntentBus,
    pub network: &'static Network,
    pub readiness: &'static Readiness,
    pub heap_free: fn() -> usize,
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
    ctx: NetCtx,
) -> ! {
    serve(station, saved, host, has_pages, ctx).await
}

/// The handle the composition root hands `run`: it starts the worker without naming the radio.
pub fn worker(
    station: Box<EspStation>,
    saved: Option<Box<CredentialStore>>,
    host: SetupHost,
    has_pages: bool,
    ctx: NetCtx,
) -> NetworkWorker {
    NetworkWorker::new(move |spawner| {
        log::info!("[WIFI] starting the network worker");
        spawner.spawn(
            task(station, saved, host, has_pages, ctx).expect("the network task fits its slot"),
        );
    })
}

pub async fn serve<S, R>(
    mut station: Box<S>,
    saved: Option<Box<Store<R>>>,
    host: SetupHost,
    has_pages: bool,
    ctx: NetCtx,
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
    let watch = LinkWatch::new(ctx);
    watch.note_mac(mac);
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
    watch.deliver(station.status(), store::any(saved.as_deref()));
    let resources = Box::leak(Box::new(StackResources::<SOCKETS>::new()));
    let (stack, mut runner) =
        embassy_net::new(device, Config::dhcpv4(Default::default()), resources, seed);
    log::info!("[WIFI] network stack up, DHCP requested");
    // The radio and the IP stack are what the network half of a boot can prove. Association
    // and DHCP are policy on top: an AP that will not answer is a live device, not a crash.
    ctx.readiness.note_network();

    match join(
        runner.run(),
        link::link(&mut *station, &stack, saved, setup, &watch),
    )
    .await {}
}

pub(super) fn mac_of<D: Driver>(device: &D) -> [u8; 6] {
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
pub(super) struct LinkWatch {
    ctx: NetCtx,
    last: Cell<Option<LinkStatus>>,
    mac: Cell<Option<[u8; 6]>>,
}

impl LinkWatch {
    fn new(ctx: NetCtx) -> Self {
        Self {
            ctx,
            last: Cell::new(None),
            mac: Cell::new(None),
        }
    }

    fn commands(&self) -> &'static NetworkInbox {
        &self.ctx.network.commands
    }

    fn page(&self) -> &'static PageWatch {
        &self.ctx.network.page
    }

    fn scan(&self) -> &'static Scan {
        &self.ctx.network.scan
    }

    fn network(&self) -> &'static Network {
        self.ctx.network
    }

    fn heap_free(&self) -> usize {
        (self.ctx.heap_free)()
    }

    fn note_mac(&self, mac: [u8; 6]) {
        self.mac.set(Some(mac));
        self.flush_mac();
    }

    fn flush_mac(&self) {
        let Some(mac) = self.mac.get() else {
            return;
        };
        if self
            .ctx
            .bus
            .try_send(Intent::Business(BusinessIntent::Link(LinkIntent::Mac(mac))))
            .is_ok()
        {
            self.mac.set(None);
        }
    }

    /// Delivers `status` with the store's bit folded in, if it differs from the last
    /// delivery. The reading is only remembered once the bus took it, so a full bus retries
    /// rather than swallowing the change.
    fn deliver(&self, status: LinkStatus, saved: bool) {
        self.flush_mac();
        let status = LinkStatus { saved, ..status };
        if self.last.get() == Some(status) {
            return;
        }
        if self
            .ctx
            .bus
            .try_send(Intent::Business(BusinessIntent::Link(LinkIntent::Status(
                status,
            ))))
            .is_ok()
        {
            self.last.set(Some(status));
        }
    }

    /// Nothing is kept for `ssid`, so the panel must ask for a passphrase. Awaited rather than
    /// tried: the question is asked once, and a full ring may delay it but not lose it.
    async fn request_passphrase(&self, ssid: Ssid) {
        log::info!(
            "[WIFI] nothing is kept for {:?}; the panel is what asks for a passphrase",
            ssid.as_display_text().unwrap_or("<unprintable>"),
        );
        self.ctx
            .bus
            .send(Intent::Business(BusinessIntent::NetworkPage(
                NetworkPageIntent::NoCopy(ssid),
            )))
            .await;
    }
}

pub(super) fn seed_from(mac: [u8; 6]) -> u64 {
    let address = mac
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
}

async fn park() -> ! {
    core::future::pending().await
}
