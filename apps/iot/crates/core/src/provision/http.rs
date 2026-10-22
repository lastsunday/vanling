//! The slice of HTTP setup mode needs: one request read off a socket, one response written back on
//! it.
//!
//! HTTP/1.1 with `Content-Length` on every answer and no chunking, so one connection can carry the
//! next request instead of being opened again: a phone refreshing the page asks on the connection
//! it already holds, which is the difference between a refresh costing a listener and costing
//! nothing ([`Request::close`] is the client's own way of asking for the old behaviour).
//!
//! Reading is a buffer and a two-way answer: a request arrives in as many reads as the network
//! decides to make of it, so [`Request::parse`] says [`ParseError::Incomplete`] rather than
//! guessing that a truncated request was a bad one — the only other reader of that distinction is
//! the loop that reads more.

use super::wire::{Bounded, Sink, push_usize};

/// How a request begins, as far as this router cares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    Get,
    Post,
    /// Anything else. Routed like a path nobody serves, which is where a redirect
    /// is a better answer than an error page.
    Other,
}

/// Why a buffer is not a request yet, or is not one at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseError {
    /// Not all of it has arrived. Read more and ask again.
    Incomplete,
    /// It cannot become a request however much more arrives.
    Malformed,
}

/// One request, borrowed from the buffer it was read into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Request<'a> {
    pub method: Method,
    /// The path, without the query string and without any absolute-form prefix.
    pub path: &'a str,
    /// The message body, exactly `Content-Length` bytes of it.
    pub body: &'a [u8],
    /// Whether the client asked for the connection to end after this request: `Connection: close`,
    /// which is the one header value that overrides the keep-alive this server answers with.
    pub close: bool,
}

impl<'a> Request<'a> {
    /// Read a request out of `bytes`, or say why there is not one there.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        let body_start = find_body_start(bytes).ok_or(ParseError::Incomplete)?;
        let head = core::str::from_utf8(&bytes[..body_start]).map_err(|_| ParseError::Malformed)?;
        let mut lines = head.split("\r\n");

        let request_line = lines.next().ok_or(ParseError::Malformed)?;
        let (method, target) = parse_request_line(request_line)?;
        let path = path_of(target)?;

        let content_length = content_length(lines)?.unwrap_or(0);
        let len = body_start
            .checked_add(content_length)
            .ok_or(ParseError::Malformed)?;
        if bytes.len() < len {
            return Err(ParseError::Incomplete);
        }

        Ok(Self {
            method,
            path,
            body: &bytes[body_start..len],
            close: connection_close(head.split("\r\n")),
        })
    }
}

/// The offset just past the blank line that ends the headers, which is also where
/// the body begins.
fn find_body_start(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|start| start + 4)
}

fn parse_request_line(line: &str) -> Result<(Method, &str), ParseError> {
    let mut parts = line.split(' ');
    let method = match parts.next() {
        Some("GET") => Method::Get,
        Some("POST") => Method::Post,
        Some(_) => Method::Other,
        None => return Err(ParseError::Malformed),
    };
    let target = parts.next().ok_or(ParseError::Malformed)?;
    // The version is checked rather than kept, and a request that names none is not
    // one a browser made.
    match parts.next() {
        Some(version) if version.starts_with("HTTP/") => {}
        _ => return Err(ParseError::Malformed),
    }
    Ok((method, target))
}

/// The path out of the request target, with the query dropped. A captive portal sees the absolute
/// form as well as the origin form; the host is dropped because every name resolves here. Nothing
/// routes on a query.
fn path_of(target: &str) -> Result<&str, ParseError> {
    let path_and_query = match target.split_once("://") {
        Some((_, after_scheme)) => match after_scheme.find('/') {
            Some(slash) => &after_scheme[slash..],
            None => "/",
        },
        None => target,
    };
    if !path_and_query.starts_with('/') {
        return Err(ParseError::Malformed);
    }
    Ok(match path_and_query.split_once('?') {
        Some((path, _)) => path,
        None => path_and_query,
    })
}

/// The value of `Content-Length`, by case-insensitive name. The first wins, which is
/// what a client that sent two of one deserves.
fn content_length<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Option<usize>, ParseError> {
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map(Some)
                .map_err(|_| ParseError::Malformed);
        }
    }
    Ok(None)
}

/// Whether the `Connection` header names `close`, the only value honoured. It is a comma-separated
/// list, so `keep-alive, close` closes; a missing header keeps, as HTTP/1.1 says.
fn connection_close<'a>(lines: impl Iterator<Item = &'a str>) -> bool {
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.eq_ignore_ascii_case("connection") {
            return value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("close"));
        }
    }
    false
}

/// What the response line says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ok,
    /// Nothing follows the head, and the client is to take that as the answer rather than
    /// as a failure: a browser asking for the page's icon by its default name is asking a
    /// question whose honest answer is "no icon", not a second copy of the page.
    NoContent,
    /// Where a name resolved to this device but nothing serves that path: the
    /// client is sent to the page rather than shown an error its user would read
    /// as a broken network.
    Redirect,
    BadRequest,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Self::Ok => "200 OK",
            Self::NoContent => "204 No Content",
            Self::Redirect => "302 Found",
            Self::BadRequest => "400 Bad Request",
        }
    }
}

/// How long a client may keep the answer.
///
/// Everything here is the device's state at the moment it was asked, so nothing is reusable —
/// except the page's own icon, which is the same bytes for the life of the build and which a
/// browser may ask for by name even though the pages carry it inline (`@/records/iot/wifi.md`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cache {
    NoStore,
    /// A day, which outlives any one setup session.
    ForADay,
}

impl Cache {
    fn line(self) -> &'static str {
        match self {
            Self::NoStore => "no-store",
            Self::ForADay => "public, max-age=86400",
        }
    }
}

/// A response body that is already somewhere.
///
/// The one answer whose bytes are made out of something — the scan list — is not a body at all:
/// [`Response::json_list`] names its length and the caller writes it.
#[derive(Debug)]
pub enum Body {
    Empty,
    Static(&'static str),
}

impl Body {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Empty => &[],
            Self::Static(text) => text.as_bytes(),
        }
    }
}

/// One response, ready to be written.
#[derive(Debug)]
pub struct Response {
    pub status: Status,
    /// The header a redirect needs, which nothing else sets.
    pub location: Option<&'static str>,
    pub content_type: &'static str,
    /// How many bytes follow the head, which is what the head has to name. The body's
    /// own length for everything built here, and the caller's to write for a list.
    pub content_length: usize,
    pub cache: Cache,
    pub body: Body,
}

impl Response {
    pub fn new(status: Status, content_type: &'static str, body: Body) -> Self {
        Self {
            status,
            location: None,
            content_type,
            content_length: body.as_bytes().len(),
            cache: Cache::NoStore,
            body,
        }
    }

    pub fn html(body: &'static str) -> Self {
        Self::new(Status::Ok, "text/html; charset=utf-8", Body::Static(body))
    }

    /// A file the page asked for by name, kept as long as a client is willing to.
    pub fn asset(content_type: &'static str, body: &'static str) -> Self {
        Self {
            cache: Cache::ForADay,
            ..Self::new(Status::Ok, content_type, Body::Static(body))
        }
    }

    /// A head with no body: the answer is the status itself.
    pub fn no_content() -> Self {
        Self::new(Status::NoContent, "text/plain; charset=utf-8", Body::Empty)
    }

    /// The head of a JSON list the caller writes after it, `length` bytes of it.
    ///
    /// The scan list is the one answer that cannot be handed over ready-made: its length
    /// is what is in range, and it is written from the survey's own list a row at a time.
    /// [`scan_json_len`](super::scan::scan_json_len) is where that length comes from.
    pub fn json_list(length: usize) -> Self {
        Self {
            status: Status::Ok,
            location: None,
            content_type: "application/json",
            content_length: length,
            cache: Cache::NoStore,
            body: Body::Empty,
        }
    }

    pub fn text(status: Status, body: &'static str) -> Self {
        Self::new(status, "text/plain; charset=utf-8", Body::Static(body))
    }

    pub fn redirect(location: &'static str) -> Self {
        Self {
            status: Status::Redirect,
            location: Some(location),
            content_type: "text/plain; charset=utf-8",
            content_length: 0,
            cache: Cache::NoStore,
            body: Body::Empty,
        }
    }

    /// Write the head and return its length. `None` means `out` is too small; nothing here
    /// allocates, so that is a caller bug, not a client one. `Content-Length` always frames the
    /// body, and `close` is the caller's only word on the connection.
    pub fn write_head(&self, out: &mut [u8], close: bool) -> Option<usize> {
        let mut sink = Bounded::new(out);
        self.head(&mut sink, close).then(|| sink.written())
    }

    /// The bytes that follow the head, where they already are.
    pub fn body(&self) -> &[u8] {
        self.body.as_bytes()
    }

    /// The head, into anything that takes bytes — the same code the device writes from
    /// and a test measures against the buffer the device writes into.
    fn head(&self, sink: &mut impl Sink, close: bool) -> bool {
        sink.write(b"HTTP/1.1 ")
            && sink.write(self.status.line().as_bytes())
            && sink.write(b"\r\nContent-Type: ")
            && sink.write(self.content_type.as_bytes())
            && sink.write(b"\r\nContent-Length: ")
            && push_usize(sink, self.content_length)
            && self.location.is_none_or(|location| {
                sink.write(b"\r\nLocation: ") && sink.write(location.as_bytes())
            })
            // The answer is the room as it is now and the page is the server's own state,
            // so nothing here is reusable — bar the icon, which is the same bytes for the
            // life of the build.
            && sink.write(b"\r\nCache-Control: ")
            && sink.write(self.cache.line().as_bytes())
            && sink.write(if close {
                b"\r\nConnection: close\r\n\r\n"
            } else {
                b"\r\nConnection: keep-alive\r\n\r\n"
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provision::PIECE_BYTES;
    use alloc::vec::Vec;

    /// The bytes the device writes for a response, a head and a body as one buffer.
    fn composed(response: &Response) -> Vec<u8> {
        let mut head = [0u8; PIECE_BYTES];
        let len = response
            .write_head(&mut head, false)
            .expect("a head that fits");
        let mut out = head[..len].to_vec();
        out.extend_from_slice(response.body());
        out
    }

    #[test]
    fn a_query_is_dropped_from_the_path() {
        let bytes = b"GET /scan?all=1 HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n";
        let request = Request::parse(bytes).expect("a whole request");
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, "/scan");
        assert_eq!(request.body, b"");
    }

    #[test]
    fn a_truncated_request_asks_for_more_rather_than_failing() {
        // The distinction is the whole reason this returns a `Result`: the loop
        // that reads says "read again", and a parser that called this malformed
        // would answer a phone with a 400 for arriving in two packets.
        assert_eq!(
            Request::parse(b"GET / HTTP/1.0\r\nHost: x\r\n"),
            Err(ParseError::Incomplete)
        );
    }

    #[test]
    fn a_body_is_read_exactly_as_long_as_it_says() {
        let bytes = b"POST /submit HTTP/1.1\r\nContent-Length: 10\r\n\r\nssid=home&";
        let request = Request::parse(bytes).expect("headers and body");
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.body, b"ssid=home&");
    }

    #[test]
    fn a_body_that_has_not_all_arrived_asks_for_more() {
        let bytes = b"POST /submit HTTP/1.1\r\nContent-Length: 40\r\n\r\nssid=home";
        assert_eq!(Request::parse(bytes), Err(ParseError::Incomplete));
    }

    #[test]
    fn the_absolute_form_a_captive_probe_uses_routes_like_the_origin_form() {
        // A phone that has decided this network is a portal asks for the URL its
        // connectivity check was written with, host and all.
        let bytes = b"GET http://connectivity.example/generate_204 HTTP/1.1\r\n\r\n";
        let request = Request::parse(bytes).expect("absolute form");
        assert_eq!(request.path, "/generate_204");
    }

    #[test]
    fn a_line_that_is_not_a_request_line_is_malformed() {
        assert_eq!(
            Request::parse(b"nonsense\r\n\r\n"),
            Err(ParseError::Malformed)
        );
        assert_eq!(
            Request::parse(b"GET / NONSENSE\r\n\r\n"),
            Err(ParseError::Malformed)
        );
    }

    #[test]
    fn a_header_that_names_a_length_that_is_not_a_number_is_malformed() {
        let bytes = b"POST /submit HTTP/1.1\r\nContent-Length: lots\r\n\r\n";
        assert_eq!(Request::parse(bytes), Err(ParseError::Malformed));
    }

    #[test]
    fn the_header_name_is_matched_whatever_case_it_arrived_in() {
        let bytes = b"POST /submit HTTP/1.0\r\ncontent-length: 5\r\n\r\nhello";
        let request = Request::parse(bytes).expect("lowercase header");
        assert_eq!(request.body, b"hello");
    }

    #[test]
    fn an_encoded_response_carries_the_length_a_client_has_to_trust() {
        let response = Response::html("<html>hi</html>");
        let written = composed(&response);
        let text = core::str::from_utf8(&written).expect("ascii");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Content-Length: 15\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\n<html>hi</html>"), "{text}");
    }

    #[test]
    fn a_redirect_carries_the_location_and_no_body() {
        let response = Response::redirect(super::super::BASE_URL);
        let written = composed(&response);
        let text = core::str::from_utf8(&written).expect("ascii");
        assert!(text.starts_with("HTTP/1.1 302 Found\r\n"), "{text}");
        assert!(
            text.contains("\r\nLocation: http://192.168.4.1/\r\n"),
            "{text}"
        );
        assert!(text.contains("\r\nContent-Length: 0\r\n"), "{text}");
        assert!(text.ends_with("Connection: keep-alive\r\n\r\n"), "{text}");
    }

    #[test]
    fn a_head_says_how_long_the_answer_may_be_kept() {
        // Everything here is the room or the device as they are at the moment they were asked, so
        // a cache must not answer for them — bar the icon, which is the same bytes for the life of
        // the build and is asked for by name often enough to be worth keeping.
        let fresh = composed(&Response::html("<html></html>"));
        let fresh = core::str::from_utf8(&fresh).expect("ascii");
        assert!(fresh.contains("Cache-Control: no-store\r\n"), "{fresh}");

        let icon = composed(&Response::asset("image/svg+xml", "<svg/>"));
        let icon = core::str::from_utf8(&icon).expect("ascii");
        assert!(
            icon.contains("Cache-Control: public, max-age=86400\r\n"),
            "{icon}"
        );
    }

    #[test]
    fn a_head_with_nothing_behind_it_says_so_and_keeps_the_connection() {
        // What a browser asking for the icon by its default name gets: a status rather than a
        // second copy of the page, which would be one more whole document written for nobody.
        let response = Response::no_content();
        let written = composed(&response);
        let text = core::str::from_utf8(&written).expect("ascii");
        assert!(text.starts_with("HTTP/1.1 204 No Content\r\n"), "{text}");
        assert!(text.contains("\r\nContent-Length: 0\r\n"), "{text}");
        assert!(text.ends_with("Connection: keep-alive\r\n\r\n"), "{text}");
    }

    #[test]
    fn the_head_says_whether_the_connection_outlives_the_answer() {
        // Keep-alive is what makes a refresh a request on the connection the phone already holds
        // rather than a new one, and it is the head — not the server loop — that tells the phone
        // so. A request that names a network, and a phone that asks for `close`, both end it.
        let mut keep = [0u8; PIECE_BYTES];
        let len = Response::no_content()
            .write_head(&mut keep, false)
            .expect("a head that fits");
        let keep = core::str::from_utf8(&keep[..len]).expect("ascii");
        assert!(keep.ends_with("Connection: keep-alive\r\n\r\n"), "{keep}");

        let mut close = [0u8; PIECE_BYTES];
        let len = Response::no_content()
            .write_head(&mut close, true)
            .expect("a head that fits");
        let close = core::str::from_utf8(&close[..len]).expect("ascii");
        assert!(close.ends_with("Connection: close\r\n\r\n"), "{close}");
    }

    #[test]
    fn a_request_names_whether_it_wants_the_connection_back() {
        // Every live connection is one not listening, so the client's own word on it is the one
        // thing that decides the socket's fate after the answer — a browser reloading the page
        // sends none and stays, and a client that is done says so.
        let open = b"GET / HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n";
        assert!(!Request::parse(open).expect("a whole request").close);

        let staying = b"GET / HTTP/1.1\r\nConnection: keep-alive\r\n\r\n";
        assert!(!Request::parse(staying).expect("a whole request").close);

        let leaving = b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n";
        assert!(Request::parse(leaving).expect("a whole request").close);

        let both = b"GET / HTTP/1.1\r\nconnection: Keep-Alive, CLOSE\r\n\r\n";
        assert!(Request::parse(both).expect("a whole request").close);
    }

    #[test]
    fn every_head_the_device_writes_fits_the_buffer_it_writes_one_into() {
        // The device builds a head in one stack buffer of `PIECE_BYTES`, because nothing here
        // allocates: a head that did not fit is a phone waiting for an answer and a warning in
        // the log.
        let heads = [
            Response::html("<html></html>"),
            Response::text(Status::Ok, "no"),
            Response::text(Status::BadRequest, "no"),
            Response::redirect(super::super::BASE_URL),
            Response::no_content(),
            Response::asset("image/svg+xml", "<svg/>"),
            // The longest length a head can be asked to name, so the digits are the ones
            // that decide whether the rest fits.
            Response::json_list(usize::MAX),
        ];
        for response in heads {
            let mut out = [0u8; PIECE_BYTES];
            assert!(
                response.write_head(&mut out, false).is_some(),
                "{response:?} does not fit {PIECE_BYTES} B"
            );
        }
    }

    #[test]
    fn a_head_that_does_not_fit_is_refused_rather_than_truncated() {
        // Half a head is a request line a client cannot read, which is worse than no
        // answer at all: what it does with one is wait, and what it does with the other
        // is retry.
        let mut out = [0u8; 8];
        assert_eq!(
            Response::redirect(super::super::BASE_URL).write_head(&mut out, false),
            None
        );
    }
}
