//! What a request to the setup access point means.
//!
//! The router is pure: a request in, an [`Action`] out, with the scan list passed in
//! rather than read from anywhere. What makes it a *captive portal* rather than a web
//! server is the last arm: a path nobody serves is answered with the page.

use crate::drivers::network::{ApList, Credentials, PASSWORD_BYTES, SSID_BYTES};

use super::BASE_URL;
use super::http::{Method, Request, Response, Status};
use super::scan::scan_json_len;

/// The page itself, compiled in. A page read from flash would be a page a bad write
/// can lose, and there is nothing here large enough for that to be worth its weight.
///
/// It carries the list the survey had when it was sent — see [`SEED_AT`] — so that opening the
/// page is one connection rather than a document followed by a request for what to draw in it.
const PAGE: &str = include_str!("assets/portal.html");
/// What the phone is shown once a credential has been handed over. It has to be
/// served *before* the access point goes away, which is why the form posts here and
/// is answered here rather than being redirected to.
const DONE: &str = include_str!("assets/done.html");
/// The page's own icon, for the browsers and probes that still ask for it by name: the pages
/// themselves carry it as a `data:` URI, so a reload does not open a connection for it at all.
/// Served under its own path rather than through the catch-all, and cached, for whoever asks
/// anyway (`@/records/iot/wifi.md`).
const FAVICON: &str = include_str!("assets/favicon.svg");
/// The longest field name the page posts (`password`), which is all the room decoding
/// one needs.
const KEY_BYTES: usize = 8;

/// Where a page writes the board's address, and how long that address is spelled. The caller
/// substitutes in place — same length in and out — so the `Content-Length` already named stays
/// true and nothing here allocates (`@/records/iot/wifi.md`).
pub const MAC_TOKEN: &str = "00:00:00:00:00:00";
/// Bytes [`mac_text`] returns, which is what [`MAC_TOKEN`] occupies.
pub const MAC_TEXT_BYTES: usize = MAC_TOKEN.len();

/// Where the page holds the list it was sent with. A slot, because a list changes length: the
/// caller writes it at [`SEED_AT`], the head names the whole document's length ([`page_len`]), and
/// the phone gets one body on one connection. The page's `[]` is a working empty list, not padding.
pub const SEED_AT: &str = "/*scan*/";
/// The bytes in the page that the list replaces, and the reason it is a slot: the page is a valid
/// document before anything is written into it.
pub const SEED_SLOT: usize = 2;

/// The length of the page once `aps` is written into its slot, for the head that has to name it.
pub fn page_len(aps: &ApList) -> usize {
    PAGE.len() + scan_json_len(aps) - SEED_SLOT
}

/// The board's address as a label reads it: `AA:BB:CC:DD:EE:FF`, the same address the access
/// point's name is cut from. Upper case is what both the panel and the page print.
pub fn mac_text(mac: [u8; 6], out: &mut [u8; MAC_TEXT_BYTES]) -> &[u8] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut at = 0;
    for (index, octet) in mac.into_iter().enumerate() {
        if index > 0 {
            out[at] = b':';
            at += 1;
        }
        out[at] = HEX[usize::from(octet >> 4)];
        out[at + 1] = HEX[usize::from(octet & 0x0f)];
        at += 2;
    }
    &out[..at]
}

/// What the router decided a request asked for.
#[derive(Debug)]
pub enum Action {
    /// Write this answer and go on listening.
    Respond(Response),
    /// The scan list: the head to answer with, and the rows for the caller to write.
    Scan(Response),
    /// The form named a network. The caller joins it; the response is what the phone
    /// is told while that happens.
    Join {
        credentials: Credentials,
        response: Response,
    },
}

/// Route one request. Anything not named here goes to the page, which is the captive-portal
/// feature: a phone asks for other people's paths, and the answer that reaches its user is the
/// page, not a 404.
pub fn route(request: &Request<'_>, aps: &ApList) -> Action {
    match (request.method, request.path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => Action::Respond(Response {
            content_length: page_len(aps),
            ..Response::html(PAGE)
        }),
        // The list as the survey last published it, and — at `/rescan` — the same list after
        // asking the radio for a fresh round. Reading the list is free; asking the radio is not,
        // and a page that read it every few seconds would keep the radio scanning
        // (`@/records/iot/wifi.md`).
        (Method::Get, "/scan") | (Method::Get, "/rescan") => {
            Action::Scan(Response::json_list(scan_json_len(aps)))
        }
        (Method::Post, "/submit") => submit(request),
        // Where a reload of the form's own URL lands, and what a phone that kept
        // the tab open comes back to.
        (Method::Get, "/done") | (Method::Get, "/done.html") => {
            Action::Respond(Response::html(DONE))
        }
        // The icon the pages carry inline, for whoever asks for it by name anyway, and the name
        // a browser asks for when it read no icon at all. Nothing is a better answer to the
        // second than another copy of the page.
        (Method::Get, "/favicon.svg") => Action::Respond(Response::asset("image/svg+xml", FAVICON)),
        (Method::Get, "/favicon.ico") => Action::Respond(Response::no_content()),
        _ => Action::Respond(Response::redirect(BASE_URL)),
    }
}

/// The form the page posts, as `application/x-www-form-urlencoded`.
fn submit(request: &Request<'_>) -> Action {
    // Decoded straight into the sizes the radio accepts, rather than into an allocation this
    // device has no heap to make: what would not fit is what `from_text` refuses anyway.
    let mut ssid = [0u8; SSID_BYTES];
    let mut password = [0u8; PASSWORD_BYTES - 1];
    let (ssid, password) = match (
        form_field(request.body, "ssid", &mut ssid),
        form_field(request.body, "password", &mut password),
    ) {
        (Field::Text(ssid), Field::Text(password)) => (ssid, password),
        // Absent is the open network: the page hides the field for a row with no lock on
        // it, and an empty passphrase is how the radio is told there is nothing to ask.
        (Field::Text(ssid), Field::Absent) => (ssid, ""),
        (Field::Absent, _) => {
            return Action::Respond(Response::text(Status::BadRequest, "no network was chosen"));
        }
        (Field::Refused, _) | (_, Field::Refused) => return not_one_the_radio_takes(),
    };
    match Credentials::from_text(ssid, password) {
        Ok(credentials) => Action::Join {
            credentials,
            response: Response::html(DONE),
        },
        Err(_) => not_one_the_radio_takes(),
    }
}

fn not_one_the_radio_takes() -> Action {
    Action::Respond(Response::text(
        Status::BadRequest,
        "that name or passphrase is not one the radio accepts",
    ))
}

/// What a form body said about one named field.
///
/// `Refused` is separate from `Absent` on purpose: a passphrase longer than the radio takes,
/// or one whose encoding no browser wrote, must not be read as "no passphrase" — that joins
/// an open network the room is not offering.
enum Field<'a> {
    Absent,
    Text(&'a str),
    Refused,
}

/// One named field out of a form body, decoded into `out`.
fn form_field<'a>(body: &[u8], name: &str, out: &'a mut [u8]) -> Field<'a> {
    let mut key = [0u8; KEY_BYTES];
    for pair in body.split(|byte| *byte == b'&') {
        let Some((raw_key, value)) = split_at(pair, b'=') else {
            continue;
        };
        let Some(key_len) = percent_decode(raw_key, &mut key) else {
            continue;
        };
        if &key[..key_len] != name.as_bytes() {
            continue;
        }
        return match percent_decode(value, out) {
            Some(len) => core::str::from_utf8(&out[..len]).map_or(Field::Refused, Field::Text),
            None => Field::Refused,
        };
    }
    Field::Absent
}

fn split_at(haystack: &[u8], needle: u8) -> Option<(&[u8], &[u8])> {
    let index = haystack.iter().position(|byte| *byte == needle)?;
    Some((&haystack[..index], &haystack[index + 1..]))
}

/// Undo the form encoding into `out`: `+` is a space, `%xx` one octet. `None` means the body was
/// malformed or did not fit, so nothing is taken.
///
/// `#[inline(never)]` keeps one copy: both call sites inlining it costs the C6 1,314 B against
/// 568 B for the copy plus call (`@/records/iot/wifi.md`).
#[inline(never)]
fn percent_decode(input: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut len = 0;
    let mut index = 0;
    while index < input.len() {
        let byte = match input[index] {
            b'+' => {
                index += 1;
                b' '
            }
            b'%' => {
                let high = hex(*input.get(index + 1)?)?;
                let low = hex(*input.get(index + 2)?)?;
                index += 3;
                (high << 4) | low
            }
            byte => {
                index += 1;
                byte
            }
        };
        *out.get_mut(len)? = byte;
        len += 1;
    }
    Some(len)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provision::Cache;
    use crate::provision::scan::{json, list};

    fn get(path: &str) -> Response {
        let request = Request::parse(path.as_bytes()).expect("a request line and a blank line");
        match route(&request, &ApList::empty()) {
            Action::Respond(response) => response,
            Action::Scan(_) => panic!("{path} is not the scan list"),
            Action::Join { .. } => panic!("{path} is not a form post"),
        }
    }

    fn post(body: &str) -> Action {
        let raw = alloc::format!(
            "POST /submit HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let request = Request::parse(raw.as_bytes()).expect("a form post");
        route(&request, &ApList::empty())
    }

    #[test]
    fn the_root_serves_the_page() {
        let response = get("GET / HTTP/1.1\r\n\r\n");
        assert_eq!(response.status, Status::Ok);
        assert!(response.content_type.starts_with("text/html"));
        assert!(response.body().starts_with(b"<!doctype html>"));
    }

    #[test]
    fn the_head_of_the_page_names_the_page_with_the_list_in_it() {
        // The page is sent with the list in its slot, so the length in the head has to be the
        // filled document's rather than the file's: a list that would not fit is a client that
        // waits for bytes that are not coming.
        let aps = list(&[("living", -55, true), ("caf\u{e9}", -70, false)]);
        let request = Request::parse(b"GET / HTTP/1.1\r\n\r\n").expect("a request");
        let Action::Respond(response) = route(&request, &aps) else {
            panic!("the root is a page");
        };
        let filled = PAGE.len() - SEED_SLOT + json(&aps).len();
        assert_eq!(response.content_length, filled);
        assert!(filled > PAGE.len(), "the list takes up the slot and more");
        assert_eq!(
            page_len(&ApList::empty()),
            PAGE.len(),
            "an empty list is the page as it stands"
        );
    }

    #[test]
    fn a_path_nobody_serves_is_answered_with_the_page() {
        // This is the captive-portal behaviour itself: a phone's own connectivity
        // check must be sent to the page rather than answered with an error it
        // would report as a broken network.
        let response = get("GET /generate_204 HTTP/1.1\r\n\r\n");
        assert_eq!(response.status, Status::Redirect);
        assert_eq!(response.location, Some(BASE_URL));
    }

    #[test]
    fn the_form_hands_a_credential_over_and_answers_the_phone() {
        match post("ssid=home+net&password=se%2Fcret") {
            Action::Join {
                credentials,
                response,
            } => {
                assert_eq!(
                    credentials.ssid.as_display_text(),
                    Some("home net"),
                    "the plus is a space and the escape is one octet"
                );
                assert_eq!(credentials.password.as_bytes(), b"se/cret");
                assert_eq!(response.status, Status::Ok);
            }
            Action::Respond(_) => panic!("a named network is a join"),
            Action::Scan(_) => panic!("a form post is not the scan list"),
        }
    }

    #[test]
    fn an_open_network_is_posted_with_no_passphrase() {
        match post("ssid=guest") {
            Action::Join { credentials, .. } => assert!(credentials.password.is_empty()),
            Action::Respond(_) => panic!("an open network is a join"),
            Action::Scan(_) => panic!("a form post is not the scan list"),
        }
    }

    #[test]
    fn a_form_with_no_network_is_refused() {
        match post("") {
            Action::Respond(response) => assert_eq!(response.status, Status::BadRequest),
            Action::Join { .. } => panic!("nothing was named"),
            Action::Scan(_) => panic!("a form post is not the scan list"),
        }
    }

    #[test]
    fn a_field_at_the_limit_is_taken_and_one_octet_more_is_refused() {
        // The fields are decoded into the sizes the radio accepts, so the boundary itself is
        // what has to hold: `SSID_BYTES` of name and one short of `PASSWORD_BYTES` of
        // passphrase are credentials, and a byte more of either is not.
        let name = |len| alloc::format!("ssid={}", "n".repeat(len));
        let passphrase = |len| alloc::format!("ssid=home&password={}", "x".repeat(len));
        assert!(matches!(post(&name(SSID_BYTES)), Action::Join { .. }));
        assert!(matches!(
            post(&passphrase(PASSWORD_BYTES - 1)),
            Action::Join { .. }
        ));
        for over in [name(SSID_BYTES + 1), passphrase(PASSWORD_BYTES)] {
            match post(&over) {
                Action::Respond(response) => assert_eq!(response.status, Status::BadRequest),
                Action::Join { .. } => panic!("{over} is not a credential"),
                Action::Scan(_) => panic!("a form post is not the scan list"),
            }
        }
    }

    #[test]
    fn a_passphrase_that_cannot_be_decoded_is_refused_rather_than_absent() {
        // Read as "no passphrase", an undecodable value would send the radio after an open
        // network the room is not offering, which is a different request than the one posted.
        match post("ssid=home&password=%zz") {
            Action::Respond(response) => assert_eq!(response.status, Status::BadRequest),
            Action::Join { .. } => panic!("a value that cannot be read is not a credential"),
            Action::Scan(_) => panic!("a form post is not the scan list"),
        }
    }

    #[test]
    fn the_scan_list_is_answered_with_its_head_and_not_its_bytes() {
        // The one answer the device writes itself. What comes back here is the head, and the
        // length in it has to be the list's own — nothing in this answer was built.
        let aps = list(&[("living", -55, true)]);
        let request = Request::parse(b"GET /scan HTTP/1.1\r\n\r\n").expect("a request");
        match route(&request, &aps) {
            Action::Scan(response) => {
                assert_eq!(response.status, Status::Ok);
                assert_eq!(response.content_type, "application/json");
                assert_eq!(response.content_length, json(&aps).len());
                assert!(
                    response.body().is_empty(),
                    "the rows are written by the caller"
                );
            }
            Action::Respond(_) => panic!("the list is not an answer that carries its own bytes"),
            Action::Join { .. } => panic!("/scan names no network"),
        }
    }

    #[test]
    fn the_two_documents_are_documents() {
        // Neither is generated, so the only thing a test can hold them to is being
        // pages at all — and the root serving the first of them byte for byte.
        assert!(PAGE.contains("<!doctype html>"));
        assert!(PAGE.contains("action=\"/submit\""));
        assert!(DONE.contains("<!doctype html>"));
        assert!(
            get("GET / HTTP/1.1\r\n\r\n")
                .body()
                .starts_with(b"<!doctype html>")
        );
    }

    #[test]
    fn the_icon_is_served_once_and_the_default_name_is_answered_with_nothing() {
        // Two names for one question, and neither is the page: the pages carry the icon inline, so
        // this is for whoever asks for the file by name, and the name a browser asks for when it
        // read no icon at all gets a status rather than a second document.
        let icon = get("GET /favicon.svg HTTP/1.1\r\n\r\n");
        assert_eq!(icon.status, Status::Ok);
        assert_eq!(icon.content_type, "image/svg+xml");
        assert_eq!(icon.cache, Cache::ForADay);
        assert_eq!(icon.content_length, FAVICON.len());
        assert!(icon.body().starts_with(b"<svg"));

        let asked_by_default = get("GET /favicon.ico HTTP/1.1\r\n\r\n");
        assert_eq!(asked_by_default.status, Status::NoContent);
        assert_eq!(asked_by_default.content_length, 0);
        assert!(asked_by_default.body().is_empty());
    }

    #[test]
    fn both_pages_carry_the_icon_and_leave_one_place_for_the_address() {
        // The address is substituted as the body is written, which is only length-preserving
        // while the token is exactly as long as the text that replaces it — and a page with two
        // tokens would be two substitutions the caller does not make. The icon is inline so that
        // a reload does not open a connection for it.
        assert_eq!(MAC_TOKEN.len(), MAC_TEXT_BYTES);
        for page in [PAGE, DONE] {
            assert!(
                page.contains("rel=\"icon\" href=\"data:image/svg+xml,"),
                "no icon carried"
            );
            assert_eq!(page.matches(MAC_TOKEN).count(), 1, "one address per page");
        }
    }

    #[test]
    fn the_page_leaves_one_slot_for_the_list_and_it_stands_on_its_own() {
        // One place for the list, holding a document that parses before anything is written into
        // it: what the page draws when the device had nothing to say is an empty list, and a page
        // that had to be filled to be valid would be a page whose failure is a syntax error.
        assert_eq!(PAGE.matches(SEED_AT).count(), 1, "one slot");
        let at = PAGE.find(SEED_AT).expect("the slot") + SEED_AT.len();
        assert_eq!(&PAGE[at..at + SEED_SLOT], "[]");
    }

    #[test]
    fn asking_again_is_the_same_answer_as_reading_the_list() {
        // The two paths differ only in whether the caller wakes the radio; what a client gets is
        // the list, and a page that asked for a fresh round has to be able to read it afterwards.
        let aps = list(&[("living", -55, true)]);
        for path in ["GET /scan HTTP/1.1\r\n\r\n", "GET /rescan HTTP/1.1\r\n\r\n"] {
            let request = Request::parse(path.as_bytes()).expect("a request");
            match route(&request, &aps) {
                Action::Scan(response) => assert_eq!(response.content_length, json(&aps).len()),
                Action::Respond(_) | Action::Join { .. } => panic!("{path} is the list"),
            }
        }
    }

    #[test]
    fn an_address_is_written_the_way_a_label_holds_it() {
        let mut text = [0u8; MAC_TEXT_BYTES];
        let written = mac_text([0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef], &mut text);
        assert_eq!(written, b"02:1F:88:AB:CD:EF");
        assert_eq!(
            written.len(),
            MAC_TOKEN.len(),
            "a longer substitution would make the head's length a lie"
        );
    }

    #[test]
    fn the_pages_speak_more_than_one_language() {
        // Chosen by the browser rather than by the server: a page is scripted anyway, and a
        // document per language would be a second body on a link that has three connections to
        // spend. The setup page has the switch; the page after it reads the same key, so a phone
        // that was reading Chinese does not change language when the device leaves.
        for page in [PAGE, DONE] {
            assert!(page.contains("navigator.languages"), "no language detected");
            assert!(
                page.contains("localStorage.getItem('lang')"),
                "no choice kept"
            );
            assert!(page.contains("zh-Hans"), "no Chinese");
        }
        assert!(PAGE.contains("data-lang=\"zh\""), "the switch is missing");
    }
}
