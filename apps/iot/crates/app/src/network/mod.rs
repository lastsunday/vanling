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
//!
//! One module per thing that owns state: [`link`] the credential rounds and the association,
//! [`setup`] the mode's two fronts, [`survey`] a round and its rest, and [`store`] the copy on the
//! part. They are separate files because each is a future chain with its own memory story, not
//! because any of them is useful without the others.

use alloc::boxed::Box;
use core::cell::{Cell, RefCell};
use core::future::pending;

use embassy_futures::join::join;
use embassy_net::driver::{Driver, HardwareAddress};
use embassy_net::{Config, StackResources};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Instant;
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{Front, LinkStatus, Ssid, Station};
use iot_core::intent::{BusinessIntent, Intent};

use crate::input::INTENT_BUS;

mod link;
mod setup;
mod store;
mod survey;

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
    publish(station.status(), store::has_saved(&saved));
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

/// The seed the stack takes for the DHCP transaction id and its ephemeral ports: the address
/// differs between devices, the uptime between boots. There is no RNG call here because the chip's
/// is behind the chip layer, which this module does not compile.
fn seed_from(octets: [u8; 6]) -> u64 {
    let address = octets
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
}

/// What an arm with nothing to do waits on. Never returns: the alternative is an arm that
/// falls through, which would leave the product with a task that has quietly stopped.
async fn park() -> ! {
    pending().await
}
