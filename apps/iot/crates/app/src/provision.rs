//! Setup mode's device side: the access point, its DHCP and DNS services, and the page.
//!
//! The only front door on a board with no panel: a name server answers everything with this
//! device, so the phone's connectivity check opens the page by itself.

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

use crate::render::{JOIN, RESCAN};

/// Port every browser asks for, and the only one this answers on.
const HTTP_PORT: u16 = 80;
/// The port a resolver listens on. Answering on any other would be a name server no client
/// would ask.
const DNS_PORT: u16 = 53;
/// Bytes a request may occupy before it is refused. Phone headers alone can spend 1 KB, and a
/// request that does not fit is dropped without an answer. The form is two fields.
const REQUEST_BYTES: usize = 2_048;
/// How long one connection may take over a request after its first byte. A page is answered in
/// ~100 ms; three seconds is long enough for a slow phone and short enough that one stalled
/// connection cannot hold a page socket.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a finished answer waits for the phone's FIN. Closing earlier sends a RST, which can
/// discard bytes the phone has not read; waiting longer keeps a socket from listening.
const CLOSE_WAIT: Duration = Duration::from_millis(100);
/// How long a connection may take to send its first byte. Half a second bounds preconnects and
/// abandoned checks; each waiting socket is one that is not listening.
const PENDING_TIMEOUT: Duration = Duration::from_millis(500);
/// How long an answered connection stays open for the phone's next request. Two seconds lets a
/// reload reuse it and still frees the listener before the next burst.
const KEEP_ALIVE: Duration = Duration::from_secs(2);
/// How long the access point stays up after the phone has been answered. Two seconds lets the done
/// page reach the screen; an acknowledged write is not a drawn page.
const LEAVE_GRACE: Duration = Duration::from_secs(2);
/// How many connections the page can have open at once. One answering socket is not listening,
/// and a phone opens several beside the page; six is Chrome's per-host ceiling.
pub const PAGE_SOCKETS: usize = 6;
/// One full segment per direction. A smaller buffer costs a round trip per window; the pairs are
/// heap, held by the page's boxed future rather than the main stack.
const PAGE_RX_BYTES: usize = 1_460;
const PAGE_TX_BYTES: usize = 1_460;

/// The newest list the survey published, for the page to read. The panel's copy rides
/// [`crate::render::SCAN`], which the render loop takes, so the page gets its own window rather
/// than consuming that one. A static because the 338-byte round that used to be allocated here
/// could not fit the S3's heap (`@/records/iot/wifi.md`).
static LATEST: Mutex<CriticalSectionRawMutex, RefCell<ApList>> =
    Mutex::new(RefCell::new(ApList::empty()));

/// Hand the page the list the survey just published, by reference so the caller's own copy is not
/// repeated in its frame.
pub fn publish_scan(list: &ApList) {
    LATEST.lock(|cell| *cell.borrow_mut() = *list);
}

fn latest_scan() -> ApList {
    LATEST.lock(|cell| *cell.borrow())
}

/// Raised for every request the page answers, so the survey can wait its pause again rather than
/// scanning under a load the phone would report as no page at all (`@/records/iot/wifi.md`).
static PAGE_READ: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Resolve once the page has answered a request since the last time this was taken.
pub async fn page_read() {
    PAGE_READ.wait().await;
}

/// The access point's own network: this device at the setup address, with no way out. The gateway
/// points at the device because a client with no route may refuse the lease.
pub fn setup_config() -> Config {
    let address = Ipv4Addr::from(ADDRESS);
    Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(address, 24),
        gateway: Some(address),
        dns_servers: Default::default(),
    })
}

/// Serve setup mode on `stack` until the caller drops the future. `Infallible` is the contract:
/// a credential ends the mode, never this function. `mac` is the address the page and the panel
/// both name.
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

/// Hand out leases to whoever joins the access point, and name this device as the resolver.
///
/// The DHCP server rather than the stack's own, which carries a client and nothing else — and the
/// address it hands out is its own, because there is nothing behind this access point but this
/// device.
async fn leases(stack: Stack<'_>) {
    let address = Ipv4Addr::from(ADDRESS);
    let mut gateway = [Ipv4Addr::UNSPECIFIED];
    let resolvers = [address];
    let mut options = ServerOptions::new(address, Some(&mut gateway));
    // The lease names this device as the resolver, which is the other half of answering every name: a
    // client handed a resolver it cannot reach shows "no internet" and no page.
    options.dns = resolvers.as_slice();
    // The DHCP option for a captive portal's URL, so a client that understands it opens the
    // page by itself rather than after its connectivity check gives up.
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

    // Eight leases: a phone, a laptop and whoever is helping, which is more than this network
    // ever holds and still a few hundred bytes of table.
    let mut server = Server::<_, 8>::new_with_et(address);
    let mut buf = [0u8; 640];
    loop {
        // The server treats a malformed request as one to skip rather than one to stop on, so reaching
        // here is the socket itself failing — a loop to restart rather than one to leave.
        if let Err(error) = io::server::run(&mut server, &options, &mut bound, &mut buf).await {
            log::warn!("[WIFI] the DHCP server stopped: {error:?}");
            Timer::after(Duration::from_millis(500)).await;
        }
    }
}

/// Answer every name with this device's own address. That is the phone's connectivity check
/// answered with a reachable host, so it reports a portal rather than a broken network. The reply
/// itself is [`iot_core::provision::answer`].
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
        // A query nothing can be made of is one to drop: answering it with something invented
        // would be a client told a name resolves to a place it does not.
        let Some(answer_len) = iot_core::provision::answer(&query[..len], ADDRESS, &mut reply)
        else {
            continue;
        };
        if let Err(error) = socket.send_to(&reply[..answer_len], from.endpoint).await {
            log::warn!("[WIFI] a name answer would not send: {error:?}");
        }
    }
}

/// The config page: answer on whichever socket is free. A socket that is answering or keeping a
/// connection open is not listening, so the six sockets, keep-alive and the short pending timeout
/// are what keep the phone from seeing `ERR_CONNECTION_REFUSED` (`@/records/iot/wifi.md`).
///
/// The credential is published here, after the answer is acknowledged: publishing ends setup mode
/// and takes the access point down, and a phone whose page never finished would post again.
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
    // Every socket after every buffer: they are one type, so the borrows have one lifetime, and
    // a socket dropped after any of the buffers is a borrow the drop order cannot satisfy.
    let mut first = TcpSocket::new(stack, &mut first_rx, &mut first_tx);
    let mut second = TcpSocket::new(stack, &mut second_rx, &mut second_tx);
    let mut third = TcpSocket::new(stack, &mut third_rx, &mut third_tx);
    let mut fourth = TcpSocket::new(stack, &mut fourth_rx, &mut fourth_tx);
    let mut fifth = TcpSocket::new(stack, &mut fifth_rx, &mut fifth_tx);
    let mut sixth = TcpSocket::new(stack, &mut sixth_rx, &mut sixth_tx);

    loop {
        // A connection whose request has already arrived is served before one that is only
        // held: the page, its icon and its list arrive together, and a connection that has said
        // nothing yet must not keep one that has waiting (`@/records/iot/wifi.md`).
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
        // `flush` returns in `FinWait2`: our FIN is acknowledged and the phone's is not in yet, which
        // means it is still reading. Releasing the socket there makes the stack answer the phone's
        // next segment with a RST, so the wait is for the phone's own FIN — `TimeWait` — and what
        // ends the connection is then a FIN. `Closed` is the phone having reset it first, where there
        // is nothing left to send.
        let picked_close = Instant::now();
        let deadline = picked_close + CLOSE_WAIT;
        while !matches!(socket.state(), State::TimeWait | State::Closed)
            && Instant::now() < deadline
        {
            Timer::after(Duration::from_millis(2)).await;
        }
        // Every connection measured comes back already in `TimeWait` — the phone's FIN is in before
        // our own is even acknowledged — so a bound reached is the exception and the one case worth a
        // line. What ends that one is a RST, 100 ms after the body was acknowledged, where nothing is
        // left to read (`@/records/iot/wifi.md`).
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
            // The access point is leaving with this credential, so there is nothing left to answer:
            // a second submit arriving inside the grace would be published over the first, and the
            // link would take the newest one as its own after joining — a device told twice to go
            // somewhere, on a front that is no longer answering.
            core::future::pending::<()>().await;
        }
    }
}

/// Whether a socket can listen for the next connection. `TimeWait` counts: the connection has
/// ended and the stack accepts a listener again. Re-listening any other state fails with
/// `InvalidState` while the phone waits (`@/records/iot/wifi.md`).
fn armable(state: State) -> bool {
    matches!(state, State::Closed | State::TimeWait)
}

/// Read requests off one connection, answering each, and return the credential if one was named.
/// Keep-alive means a refresh reuses the connection; it ends on close, [`KEEP_ALIVE`] silence, or a
/// credential, because publishing that takes the access point down.
///
/// Pipelined requests are not read: the bytes past the answered one go with the buffer, and a phone
/// that sent two asks again on a new connection.
async fn answer(socket: &mut TcpSocket<'_>, mac: [u8; 6]) -> Option<Credentials> {
    let mut bytes = [0u8; REQUEST_BYTES];
    // The one buffer an answer is built in, and the reason nothing here allocates: a head is written
    // into it, and the scan list — the only answer whose length is not known until it is built — goes
    // out of it a row at a time. An allocation that failed here is the device's death rather than a
    // bad answer, and this path is the one that took the S3 there (`@/records/iot/wifi.md`).
    let mut piece = [0u8; PIECE_BYTES];
    // The first request on a connection is the phone's to open; every request after it is the same
    // phone asking again on a connection it already holds, which is what [`KEEP_ALIVE`] bounds.
    let mut wait = PENDING_TIMEOUT;
    loop {
        socket.set_timeout(Some(wait));
        let mut filled = 0;
        let picked = Instant::now();
        let request = loop {
            match socket.read(&mut bytes[filled..]).await {
                // The client went away before saying anything whole.
                Ok(0) => return None,
                Ok(read) => {
                    // From the first byte of a request this is a person watching a page load, and the
                    // rest of it arrives as their phone allows: the whole timeout is theirs now.
                    if filled == 0 {
                        socket.set_timeout(Some(CONNECTION_TIMEOUT));
                    }
                    filled += read;
                }
                Err(error) => {
                    // The wait is the whole story: a fraction of a second is the phone going away,
                    // and a whole [`PENDING_TIMEOUT`] is a connection nobody sent anything on
                    // (`@/records/iot/wifi.md`). A wait that runs out after an answer is the
                    // keep-alive closing on schedule, which is the design rather than a finding, so
                    // it is not a line: only the first request on a connection is worth one.
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

        // A phone asking for a *fresh* round is a person who pressed the button; reading the list
        // is not, and it must not be: every read used to wake the survey, so a page that read the
        // list every four seconds kept the radio scanning for as long as it was open — which is
        // radio time the access point does not have while a phone is holding it open
        // (`@/records/iot/wifi.md`).
        if request.path == "/rescan" {
            RESCAN.set(());
        }
        let aps = latest_scan();
        // The phone may ask again on this connection unless it said otherwise, and an answer that
        // did not go out leaves nothing to ask on.
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
            // The credential is what the page was for, so the answer is the last one this connection
            // carries: the access point goes down as soon as it is published.
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

/// One line per answered request: the only device-side record of a bench round. The time runs from
/// taking the connection to having the answer out (`@/records/iot/wifi.md`).
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

/// Write one answer: the head, and then the body where it already is. `false` when the head did not
/// fit its buffer, which is a connection with nothing left to say on it.
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

/// Write a page body with the address and scan list spliced into their slots. The head already
/// names the filled length and each token appears once, so this is one body of the promised length
/// and no second copy of the page (`@/records/iot/wifi.md`).
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

/// Write the head and say whether it fit. A head that does not fit is this build's bug; the phone
/// sees a closed connection and the log gets the line.
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

/// Write the scan list in pieces out of the head's buffer. A row that does not fit is refused,
/// not shortened: a list short of bytes is one the page reads as an error.
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

/// Write all of `bytes`, or log and give up. A partial write is ordinary; an unrecoverable one still
/// belongs to a request that was made.
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
