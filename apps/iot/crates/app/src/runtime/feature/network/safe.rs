//! Safe mode's runtime: the radio and one read-only page, without the board.
//!
//! The product path can fail anywhere between the chip and the first frame, so this path takes
//! only what the radio needs: the station, the IP stack, and a heap that is allowed to be
//! missing. It deliberately does not build a `Board`, start the coordinator, or arm the
//! watchdog. A boot that reached here does not get to reset itself out of the evidence.

use alloc::boxed::Box;
use core::cell::Cell;

use embassy_futures::join::{join, join4};
use embassy_futures::select::{Either, select};
use embassy_net::driver::Driver;
use embassy_net::tcp::{State, TcpSocket};
use embassy_net::{Config, Stack, StackResources};
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{Credentials, Ipv4, LinkPhase, Station};
use iot_core::provision::{ParseError, Request, Response, Status, setup_ap_ssid};
use iot_core::safe::{Facts, STATUS_BYTES, write_status};

use super::link::wait_for_address;
use super::{heap_free, provision};

/// Socket resources for the access point: one HTTP listener plus DHCP and DNS.
const SAFE_SOCKETS: usize = 3;

const HTTP_PORT: u16 = 80;

const JOIN_BUDGET_MS: u64 = 20_000;

const PAGE_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_WAIT: Duration = Duration::from_millis(100);
const RETRY_PAUSE: Duration = Duration::from_millis(200);

const REQUEST_BYTES: usize = 1_024;
const HEAD_BYTES: usize = 512;
const SOCKET_RX_BYTES: usize = 1_024;
const SOCKET_TX_BYTES: usize = 1_024;

/// Run the safe-mode surface until the part is reset, power-cycled or reflashed.
///
/// The access point is open: the normal setup passphrase is shown on a panel this boot does
/// not trust, and the page is read-only.
pub async fn run<S, R>(
    station: Option<Box<S>>,
    saved: Option<Box<Store<R>>>,
    crashed_boots: u16,
) -> !
where
    S: Station,
    S::Device: Driver,
    R: FlashRegion,
{
    let Some(mut station) = station else {
        serial_park("the radio would not start").await
    };
    let saved_networks = saved.as_ref().map_or(0, |store| store.networks().len());
    let credentials = credentials_for(&saved);

    // Taking the interface here also gives the access point's name its MAC, even when the
    // station attempt is about to fail. Dropping it releases the singleton so setup can claim
    // the access-point interface.
    let device = station.take_device();
    let mac = device.as_ref().map(mac_of).unwrap_or([0; 6]);
    if let Some(device) = device
        && let Some(credentials) = credentials
    {
        station_session(
            &mut *station,
            device,
            &credentials,
            mac,
            crashed_boots,
            saved_networks,
        )
        .await;
    }

    access_point(*station, mac, crashed_boots, saved_networks).await
}

fn credentials_for<R: FlashRegion>(saved: &Option<Box<Store<R>>>) -> Option<Credentials> {
    crate::state::boot::build_credentials()
        .and_then(|(ssid, password)| Credentials::from_text(ssid, password).ok())
        .or_else(|| {
            saved
                .as_ref()
                .and_then(|store| store.networks().iter().next())
        })
}

/// One association attempt on the station. A success keeps the stack and serves the page for
/// the rest of the boot; a failure returns so the open access point can take over.
async fn station_session<S>(
    station: &mut S,
    device: S::Device,
    credentials: &Credentials,
    mac: [u8; 6],
    crashed_boots: u16,
    saved_networks: usize,
) where
    S: Station,
    S::Device: Driver,
{
    let resources = Box::leak(Box::new(StackResources::<SAFE_SOCKETS>::new()));
    let seed = seed_from(mac);
    let (stack, mut runner) =
        embassy_net::new(device, Config::dhcpv4(Default::default()), resources, seed);
    let report = Report {
        mac,
        crashed_boots,
        saved_networks,
        started: Instant::now(),
        phase: Cell::new(LinkPhase::Connecting),
        address: Cell::new(None),
        station: true,
    };
    log::info!("[SAFE] station attempt on \"{}\"", display(credentials));
    let connected = matches!(
        select(runner.run(), attempt(station, &stack, credentials, &report),).await,
        Either::Second(true)
    );
    if !connected {
        log::warn!(
            "[SAFE] the station attempt did not produce an address; raising the access point"
        );
        return;
    }

    match join(runner.run(), service_page(stack, &report)).await {}
}

async fn attempt<S>(
    station: &mut S,
    stack: &Stack<'_>,
    credentials: &Credentials,
    report: &Report,
) -> bool
where
    S: Station,
{
    if let Err(error) = station.begin_connect(credentials) {
        report.phase.set(LinkPhase::Failed(error));
        log::warn!("[SAFE] the radio refused the credential: {error:?}");
        return false;
    }
    if let Err(error) = station.await_connected().await {
        report.phase.set(LinkPhase::Failed(error));
        log::warn!("[SAFE] association failed: {error:?}");
        return false;
    }

    log::info!("[SAFE] associated; waiting up to {JOIN_BUDGET_MS} ms for an address");
    let address = match select(
        wait_for_address(stack),
        Timer::after(Duration::from_millis(JOIN_BUDGET_MS)),
    )
    .await
    {
        Either::First(address) => address,
        Either::Second(()) => None,
    };
    station.note_address(address);
    report.phase.set(station.status().phase);
    report.address.set(address);
    if let Some(address) = address {
        let [a, b, c, d] = address.octets();
        log::info!("[SAFE] diagnostics are at http://{a}.{b}.{c}.{d}/");
    }
    address.is_some()
}

async fn access_point<S>(
    mut station: S,
    mac: [u8; 6],
    crashed_boots: u16,
    saved_networks: usize,
) -> !
where
    S: Station,
    S::Device: Driver,
{
    let name = setup_ap_ssid(crate::assets::SAFE_AP_PREFIX, mac);
    let device = match station.begin_setup(&name, false) {
        Ok(device) => device,
        Err(error) => {
            log::error!("[SAFE] the access point would not come up: {error:?}");
            serial_park("the access point would not start").await
        }
    };
    let resources = Box::leak(Box::new(StackResources::<SAFE_SOCKETS>::new()));
    let seed = seed_from(mac);
    let (stack, mut runner) = embassy_net::new(device, provision::setup_config(), resources, seed);

    let report = Report {
        mac,
        crashed_boots,
        saved_networks,
        started: Instant::now(),
        phase: Cell::new(station.status().phase),
        address: Cell::new(None),
        station: false,
    };
    log::error!(
        "[SAFE] diagnostics are open on \"{}\" at {}",
        name.as_display_text().unwrap_or("<not drawable>"),
        iot_core::provision::BASE_URL,
    );
    match join4(
        runner.run(),
        provision::leases(stack),
        provision::names(stack),
        service_page(stack, &report),
    )
    .await {}
}

struct Report {
    mac: [u8; 6],
    crashed_boots: u16,
    saved_networks: usize,
    started: Instant,
    phase: Cell<LinkPhase>,
    address: Cell<Option<Ipv4>>,
    station: bool,
}

impl Report {
    fn facts(&self) -> Facts {
        Facts {
            mac: self.mac,
            crashed_boots: self.crashed_boots,
            saved_networks: self.saved_networks,
            heap_free: heap_free(),
            uptime_ms: self.started.elapsed().as_millis().min(u32::MAX as u64) as u32,
            phase: self.phase.get(),
            address: self.address.get(),
            station: self.station,
        }
    }
}

async fn service_page(stack: Stack<'_>, report: &Report) -> ! {
    let mut rx = [0u8; SOCKET_RX_BYTES];
    let mut tx = [0u8; SOCKET_TX_BYTES];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    let mut bytes = [0u8; REQUEST_BYTES];
    let mut body = [0u8; STATUS_BYTES];
    let mut head = [0u8; HEAD_BYTES];

    loop {
        if let Err(error) = socket.accept(HTTP_PORT).await {
            log::warn!("[SAFE] the page could not accept a connection: {error:?}");
            Timer::after(RETRY_PAUSE).await;
            continue;
        }

        match read_request(&mut socket, &mut bytes).await {
            Read::Filled(filled) => match Request::parse(&bytes[..filled]) {
                Ok(request) => {
                    answer(&mut socket, &request, report, &mut body, &mut head).await;
                }
                Err(_) => refuse(&mut socket, &mut head, "malformed request").await,
            },
            Read::Malformed => refuse(&mut socket, &mut head, "malformed request").await,
            Read::Closed => {}
        }

        socket.close();
        if let Err(error) = socket.flush().await {
            log::warn!("[SAFE] an answer did not finish: {error:?}");
        }
        let deadline = Instant::now() + CLOSE_WAIT;
        while !matches!(socket.state(), State::TimeWait | State::Closed)
            && Instant::now() < deadline
        {
            Timer::after(Duration::from_millis(2)).await;
        }
        socket.abort();
    }
}

enum Read {
    Filled(usize),
    Malformed,
    Closed,
}

async fn read_request(socket: &mut TcpSocket<'_>, bytes: &mut [u8]) -> Read {
    let mut filled = 0;
    loop {
        socket.set_timeout(Some(PAGE_TIMEOUT));
        match socket.read(&mut bytes[filled..]).await {
            Ok(0) => return Read::Closed,
            Ok(read) => filled += read,
            Err(error) => {
                log::warn!("[SAFE] a request would not read: {error:?}");
                return Read::Closed;
            }
        }
        match Request::parse(&bytes[..filled]) {
            Ok(_) => return Read::Filled(filled),
            Err(ParseError::Incomplete) if filled < bytes.len() => {}
            Err(_) => return Read::Malformed,
        }
    }
}

async fn answer(
    socket: &mut TcpSocket<'_>,
    request: &Request<'_>,
    report: &Report,
    body: &mut [u8; STATUS_BYTES],
    head: &mut [u8; HEAD_BYTES],
) {
    match iot_core::safe::route(request) {
        iot_core::safe::Action::Status => {
            let Some(len) = write_status(body, &report.facts()) else {
                let response =
                    Response::text(Status::ServiceUnavailable, "diagnostics did not fit");
                return send_response(socket, &response, response.body(), head).await;
            };
            let response = Response {
                content_length: len,
                ..Response::text(Status::Ok, "")
            };
            send_response(socket, &response, &body[..len], head).await;
        }
        iot_core::safe::Action::NoContent => {
            send_response(socket, &Response::no_content(), &[], head).await;
        }
        iot_core::safe::Action::BadRequest => {
            refuse(socket, head, "read-only diagnostics").await;
        }
    }
    log::info!("[SAFE] {:?} {} answered", request.method, request.path);
}

async fn refuse(socket: &mut TcpSocket<'_>, head: &mut [u8; HEAD_BYTES], message: &'static str) {
    let response = Response::text(Status::BadRequest, message);
    send_response(socket, &response, response.body(), head).await;
}

async fn send_response(
    socket: &mut TcpSocket<'_>,
    response: &Response,
    body: &[u8],
    head: &mut [u8; HEAD_BYTES],
) {
    let Some(len) = response.write_head(head, true) else {
        log::warn!("[SAFE] a response head did not fit {HEAD_BYTES} B");
        return;
    };
    send(socket, &head[..len]).await;
    send(socket, body).await;
}

async fn send(socket: &mut TcpSocket<'_>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match socket.write(bytes).await {
            Ok(0) => {
                log::warn!("[SAFE] an answer stopped {} B short", bytes.len());
                return;
            }
            Ok(written) => bytes = &bytes[written..],
            Err(error) => {
                log::warn!(
                    "[SAFE] an answer stopped {} B short: {error:?}",
                    bytes.len()
                );
                return;
            }
        }
    }
}

async fn serial_park(reason: &str) -> ! {
    log::error!("[SAFE] {reason}; the product is not started and no watchdog is armed");
    loop {
        Timer::after(Duration::from_secs(10)).await;
        log::error!("[SAFE] still parked; press reset, reflash over USB, or power-cycle");
    }
}

fn mac_of<D: Driver>(device: &D) -> [u8; 6] {
    match device.hardware_address() {
        embassy_net::driver::HardwareAddress::Ethernet(octets) => octets,
        _ => [0; 6],
    }
}

fn seed_from(mac: [u8; 6]) -> u64 {
    let address = mac
        .iter()
        .fold(0u64, |seed, octet| (seed << 8) | u64::from(*octet));
    (address << 16) ^ Instant::now().as_micros()
}

fn display(credentials: &Credentials) -> &str {
    credentials
        .ssid
        .as_display_text()
        .unwrap_or("<unprintable>")
}
