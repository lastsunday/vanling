//! Setup mode's device side: the access point, its DHCP and DNS services, and the page.

use core::cell::RefCell;
use core::convert::Infallible;
use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use edge_dhcp::io::{self, DEFAULT_SERVER_PORT};
use edge_dhcp::server::{Server, ServerOptions};
use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_futures::join::join4;
use embassy_futures::select::select_array;
use embassy_net::driver::Driver;
use embassy_net::tcp::{State, TcpSocket};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Config, Ipv4Cidr, Runner, Stack, StaticConfigV4};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::network::{ApList, Credentials, JoinRequest};
use iot_core::provision::{
    ADDRESS, Action, BASE_URL, MAC_TEXT_BYTES, MAC_TOKEN, PIECE_BYTES, ParseError, Piece, Request,
    Response, SEED_AT, SEED_SLOT, ScanJson, Status, mac_text, route, scan_json_row,
};

use crate::bridge::{JOIN, RESCAN};

const HTTP_PORT: u16 = 80;
const DNS_PORT: u16 = 53;
const REQUEST_BYTES: usize = 2_048;
// A page answers in about 100 ms; 3 s absorbs a slow phone without letting one stalled
// connection hold a socket.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_WAIT: Duration = Duration::from_millis(100);
const PENDING_TIMEOUT: Duration = Duration::from_millis(500);
const KEEP_ALIVE: Duration = Duration::from_secs(2);
const LEAVE_GRACE: Duration = Duration::from_secs(2);
pub const PAGE_SOCKETS: usize = 6;
const PAGE_RX_BYTES: usize = 1_460;
const PAGE_TX_BYTES: usize = 1_460;

static LATEST: Mutex<CriticalSectionRawMutex, RefCell<ApList>> =
    Mutex::new(RefCell::new(ApList::empty()));

/// By reference: the caller's copy would otherwise be repeated in its frame.
pub fn publish_scan(list: &ApList) {
    LATEST.lock(|cell| *cell.borrow_mut() = *list);
}

fn latest_scan() -> ApList {
    LATEST.lock(|cell| *cell.borrow())
}

static PAGE_READ: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub async fn page_read() {
    PAGE_READ.wait().await;
}

pub fn setup_config() -> Config {
    let address = Ipv4Addr::from(ADDRESS);
    Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(address, 24),
        gateway: Some(address),
        dns_servers: Default::default(),
    })
}

/// Serve setup mode on `stack` until the caller drops the future.
pub async fn serve<'d, D>(stack: Stack<'d>, mut runner: Runner<'d, D>, mac: [u8; 6]) -> Infallible
where
    D: Driver,
{
    log::info!(
        "[WIFI] setup mode: an access point, a lease for whoever joins it, and the page at \
         {BASE_URL}"
    );
    match join4(runner.run(), leases(stack), names(stack), page(stack, mac)).await {}
}

async fn leases(stack: Stack<'_>) {
    let address = Ipv4Addr::from(ADDRESS);
    let mut gateway = [Ipv4Addr::UNSPECIFIED];
    let resolvers = [address];
    let mut options = ServerOptions::new(address, Some(&mut gateway));
    options.dns = resolvers.as_slice();
    options.captive_url = Some(BASE_URL);

    let buffers = UdpBuffers::<1, 640, 640, 2>::new();
    let socket = Udp::new(stack, &buffers);
    let mut bound = match socket
        .bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            DEFAULT_SERVER_PORT,
        )))
        .await
    {
        Ok(bound) => bound,
        Err(error) => {
            log::error!("[WIFI] the DHCP server could not bind: {error:?}");
            return;
        }
    };

    let mut server = Server::<_, 8>::new_with_et(address);
    let mut buf = [0u8; 640];
    loop {
        if let Err(error) = io::server::run(&mut server, &options, &mut bound, &mut buf).await {
            log::warn!("[WIFI] the DHCP server stopped: {error:?}");
            Timer::after(Duration::from_millis(500)).await;
        }
    }
}

async fn names(stack: Stack<'_>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf = [0u8; 512];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_buf = [0u8; 512];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if let Err(error) = socket.bind(DNS_PORT) {
        log::error!("[WIFI] the name server could not bind: {error:?}");
        return;
    }

    let mut query = [0u8; 512];
    let mut reply = [0u8; 512];
    loop {
        let (len, from) = match socket.recv_from(&mut query).await {
            Ok(received) => received,
            Err(error) => {
                log::warn!("[WIFI] the name server stopped reading: {error:?}");
                Timer::after(Duration::from_millis(200)).await;
                continue;
            }
        };
        let Some(answer_len) = iot_core::provision::answer(&query[..len], ADDRESS, &mut reply)
        else {
            continue;
        };
        if let Err(error) = socket.send_to(&reply[..answer_len], from.endpoint).await {
            log::warn!("[WIFI] a name answer would not send: {error:?}");
        }
    }
}

async fn page(stack: Stack<'_>, mac: [u8; 6]) {
    let mut first_rx = [0u8; PAGE_RX_BYTES];
    let mut first_tx = [0u8; PAGE_TX_BYTES];
    let mut second_rx = [0u8; PAGE_RX_BYTES];
    let mut second_tx = [0u8; PAGE_TX_BYTES];
    let mut third_rx = [0u8; PAGE_RX_BYTES];
    let mut third_tx = [0u8; PAGE_TX_BYTES];
    let mut fourth_rx = [0u8; PAGE_RX_BYTES];
    let mut fourth_tx = [0u8; PAGE_TX_BYTES];
    let mut fifth_rx = [0u8; PAGE_RX_BYTES];
    let mut fifth_tx = [0u8; PAGE_TX_BYTES];
    let mut sixth_rx = [0u8; PAGE_RX_BYTES];
    let mut sixth_tx = [0u8; PAGE_TX_BYTES];
    let mut first = TcpSocket::new(stack, &mut first_rx, &mut first_tx);
    let mut second = TcpSocket::new(stack, &mut second_rx, &mut second_tx);
    let mut third = TcpSocket::new(stack, &mut third_rx, &mut third_tx);
    let mut fourth = TcpSocket::new(stack, &mut fourth_rx, &mut fourth_tx);
    let mut fifth = TcpSocket::new(stack, &mut fifth_rx, &mut fifth_tx);
    let mut sixth = TcpSocket::new(stack, &mut sixth_rx, &mut sixth_tx);

    loop {
        let sockets = [
            &mut first,
            &mut second,
            &mut third,
            &mut fourth,
            &mut fifth,
            &mut sixth,
        ];
        let pick = sockets
            .iter()
            .position(|socket| !armable(socket.state()) && socket.can_recv())
            .or_else(|| sockets.iter().position(|socket| !armable(socket.state())));

        let socket = match pick {
            Some(index) => &mut *sockets[index],
            None => {
                let [first, second, third, fourth, fifth, sixth] = sockets;
                let (accepted, index) = select_array([
                    first.accept(HTTP_PORT),
                    second.accept(HTTP_PORT),
                    third.accept(HTTP_PORT),
                    fourth.accept(HTTP_PORT),
                    fifth.accept(HTTP_PORT),
                    sixth.accept(HTTP_PORT),
                ])
                .await;
                match accepted {
                    Ok(()) => match index {
                        0 => first,
                        1 => second,
                        2 => third,
                        3 => fourth,
                        4 => fifth,
                        _ => sixth,
                    },
                    Err(error) => {
                        log::warn!("[WIFI] the page could not accept a connection: {error:?}");
                        Timer::after(Duration::from_millis(200)).await;
                        continue;
                    }
                }
            }
        };
        let joining = answer(socket, mac).await;
        socket.close();
        if let Err(error) = socket.flush().await {
            log::warn!("[WIFI] an answer did not finish: {error:?}");
        }
        let picked_close = Instant::now();
        let deadline = picked_close + CLOSE_WAIT;
        while !matches!(socket.state(), State::TimeWait | State::Closed)
            && Instant::now() < deadline
        {
            Timer::after(Duration::from_millis(2)).await;
        }
        // `[实测]` connections come back already in `TimeWait`; the exception is a RST 100 ms
        // after the body was acknowledged, with nothing left to read (`@/records/iot/wifi.md`).
        if !matches!(socket.state(), State::TimeWait | State::Closed) {
            log::warn!(
                "[WIFI] the answer's socket was still {:?} after {} ms",
                socket.state(),
                picked_close.elapsed().as_millis()
            );
        }
        socket.abort();
        if let Some(credentials) = joining {
            log::info!("[WIFI] the page named a network, so the link is joining it");
            Timer::after(LEAVE_GRACE).await;
            JOIN.publish(JoinRequest::Typed(credentials));
            core::future::pending::<()>().await;
        }
    }
}

fn armable(state: State) -> bool {
    matches!(state, State::Closed | State::TimeWait)
}

async fn answer(socket: &mut TcpSocket<'_>, mac: [u8; 6]) -> Option<Credentials> {
    let mut bytes = [0u8; REQUEST_BYTES];
    let mut piece = [0u8; PIECE_BYTES];
    let mut wait = PENDING_TIMEOUT;
    loop {
        socket.set_timeout(Some(wait));
        let mut filled = 0;
        let picked = Instant::now();
        let request = loop {
            match socket.read(&mut bytes[filled..]).await {
                Ok(0) => return None,
                Ok(read) => {
                    if filled == 0 {
                        socket.set_timeout(Some(CONNECTION_TIMEOUT));
                    }
                    filled += read;
                }
                Err(error) => {
                    if wait == PENDING_TIMEOUT {
                        log::warn!(
                            "[WIFI] a request would not be read after {} ms: {error:?}",
                            picked.elapsed().as_millis()
                        );
                    }
                    return None;
                }
            }

            match Request::parse(&bytes[..filled]) {
                Ok(request) => break request,
                Err(ParseError::Incomplete) => {
                    if filled == bytes.len() {
                        log::warn!("[WIFI] a request filled the buffer without ending");
                        return None;
                    }
                }
                Err(ParseError::Malformed) => {
                    log::warn!("[WIFI] a request line did not parse, so it was refused");
                    let refused = Response::text(Status::BadRequest, "malformed request");
                    send_answer(socket, &refused, &mut piece, mac, &latest_scan(), true).await;
                    return None;
                }
            }
        };

        if request.path == "/rescan" {
            RESCAN.set(());
        }
        let aps = latest_scan();
        let mut keep = !request.close;
        match route(&request, &aps) {
            Action::Respond(response) => {
                let sent =
                    send_answer(socket, &response, &mut piece, mac, &aps, request.close).await;
                log_answer(&request, &response, picked.elapsed());
                keep = sent && keep;
            }
            Action::Scan(response) => {
                let sent = send_head(socket, &response, &mut piece, request.close).await;
                if sent {
                    send_scan(socket, &aps, &mut piece).await;
                }
                log_answer(&request, &response, picked.elapsed());
                keep = sent && keep;
            }
            Action::Join {
                credentials,
                response,
            } => {
                send_answer(socket, &response, &mut piece, mac, &aps, true).await;
                log_answer(&request, &response, picked.elapsed());
                return Some(credentials);
            }
        }
        if !keep {
            return None;
        }
        wait = KEEP_ALIVE;
    }
}

fn log_answer(request: &Request<'_>, response: &Response, took: Duration) {
    PAGE_READ.signal(());
    log::info!(
        "[WIFI] {:?} {} answered {:?}, {} B of body, in {} ms",
        request.method,
        request.path,
        response.status,
        response.content_length,
        took.as_millis(),
    );
}

async fn send_answer(
    socket: &mut TcpSocket<'_>,
    response: &Response,
    piece: &mut [u8; PIECE_BYTES],
    mac: [u8; 6],
    aps: &ApList,
    close: bool,
) -> bool {
    if !send_head(socket, response, piece, close).await {
        return false;
    }
    send_page(socket, response.body(), mac, aps, piece).await;
    true
}

async fn send_page(
    socket: &mut TcpSocket<'_>,
    body: &[u8],
    mac: [u8; 6],
    aps: &ApList,
    piece: &mut [u8; PIECE_BYTES],
) {
    let mut text = [0u8; MAC_TEXT_BYTES];
    let mac = mac_text(mac, &mut text);
    let mut rest = body;
    if let Some(at) = find(rest, MAC_TOKEN.as_bytes()) {
        send(socket, &rest[..at]).await;
        send(socket, mac).await;
        rest = &rest[at + MAC_TOKEN.len()..];
    }
    if let Some(at) = find(rest, SEED_AT.as_bytes()) {
        send(socket, &rest[..at + SEED_AT.len()]).await;
        send_scan(socket, aps, piece).await;
        rest = &rest[at + SEED_AT.len() + SEED_SLOT..];
    }
    send(socket, rest).await;
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn send_head(
    socket: &mut TcpSocket<'_>,
    response: &Response,
    piece: &mut [u8; PIECE_BYTES],
    close: bool,
) -> bool {
    let Some(head) = response.write_head(piece, close) else {
        log::warn!("[WIFI] a response head did not fit {PIECE_BYTES} B");
        return false;
    };
    send(socket, &piece[..head]).await;
    true
}

async fn send_scan(socket: &mut TcpSocket<'_>, aps: &ApList, piece: &mut [u8; PIECE_BYTES]) {
    for next in ScanJson::new(aps) {
        match next {
            Piece::Static(bytes) => send(socket, bytes).await,
            Piece::Row { ap, saved } => {
                let Some(row) = scan_json_row(&ap, saved, piece) else {
                    log::warn!("[WIFI] a scan row did not fit {PIECE_BYTES} B");
                    return;
                };
                send(socket, &piece[..row]).await;
            }
        }
    }
}

async fn send(socket: &mut TcpSocket<'_>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match socket.write(bytes).await {
            Ok(0) => {
                log::warn!("[WIFI] an answer stopped {} B short", bytes.len());
                return;
            }
            Ok(written) => bytes = &bytes[written..],
            Err(error) => {
                log::warn!(
                    "[WIFI] an answer stopped {} B short: {error:?}",
                    bytes.len()
                );
                return;
            }
        }
    }
}
