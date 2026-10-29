//! What a request to the setup access point means.

use crate::drivers::network::{ApList, Credentials, PASSWORD_BYTES, SSID_BYTES};

use super::BASE_URL;
use super::http::{Method, Request, Response, Status};
use super::scan::scan_json_len;

const PAGE: &str = include_str!("assets/portal.html");
const DONE: &str = include_str!("assets/done.html");
const FAVICON: &str = include_str!("assets/favicon.svg");
const KEY_BYTES: usize = 8;

/// Where a page writes the board's address, and how long that address is spelled.
pub const MAC_TOKEN: &str = "00:00:00:00:00:00";
/// Bytes [`mac_text`] returns, which is what [`MAC_TOKEN`] occupies.
pub const MAC_TEXT_BYTES: usize = MAC_TOKEN.len();

/// Where the page holds the list it was sent with.
pub const SEED_AT: &str = "/*scan*/";
/// The bytes in the page that the list replaces, and the reason it is a slot: the page is a
/// valid document before anything is written into it.
pub const SEED_SLOT: usize = 2;

/// The length of the page once `aps` is written into its slot, for the head that has to
/// name it.
pub fn page_len(aps: &ApList) -> usize {
    PAGE.len() + scan_json_len(aps) - SEED_SLOT
}

/// The board's address as a label reads it: `AA:BB:CC:DD:EE:FF`, the same address the
/// access point's name is cut from.
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
    Respond(Response),
    Scan(Response),
    Join {
        credentials: Credentials,
        response: Response,
    },
}

/// Route one request.
pub fn route(request: &Request<'_>, aps: &ApList) -> Action {
    match (request.method, request.path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => Action::Respond(Response {
            content_length: page_len(aps),
            ..Response::html(PAGE)
        }),
        (Method::Get, "/scan") | (Method::Get, "/rescan") => {
            Action::Scan(Response::json_list(scan_json_len(aps)))
        }
        (Method::Post, "/submit") => submit(request),
        (Method::Get, "/done") | (Method::Get, "/done.html") => {
            Action::Respond(Response::html(DONE))
        }
        (Method::Get, "/favicon.svg") => Action::Respond(Response::asset("image/svg+xml", FAVICON)),
        (Method::Get, "/favicon.ico") => Action::Respond(Response::no_content()),
        _ => Action::Respond(Response::redirect(BASE_URL)),
    }
}

fn submit(request: &Request<'_>) -> Action {
    let mut ssid = [0u8; SSID_BYTES];
    let mut password = [0u8; PASSWORD_BYTES - 1];
    let (ssid, password) = match (
        form_field(request.body, "ssid", &mut ssid),
        form_field(request.body, "password", &mut password),
    ) {
        (Field::Text(ssid), Field::Text(password)) => (ssid, password),
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

enum Field<'a> {
    Absent,
    Text(&'a str),
    Refused,
}

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

// Undo the form encoding into `out`: `+` is a space, `%xx` one octet. `None` means the body was
// malformed or did not fit, so nothing is taken.
//
// `#[inline(never)]` keeps one copy: both call sites inlining it costs the C6 1,314 B against
// 568 B for the copy plus call (`@/records/iot/wifi.md`).
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
        match post("ssid=home&password=%zz") {
            Action::Respond(response) => assert_eq!(response.status, Status::BadRequest),
            Action::Join { .. } => panic!("a value that cannot be read is not a credential"),
            Action::Scan(_) => panic!("a form post is not the scan list"),
        }
    }

    #[test]
    fn the_scan_list_is_answered_with_its_head_and_not_its_bytes() {
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
        assert_eq!(PAGE.matches(SEED_AT).count(), 1, "one slot");
        let at = PAGE.find(SEED_AT).expect("the slot") + SEED_AT.len();
        assert_eq!(&PAGE[at..at + SEED_SLOT], "[]");
    }

    #[test]
    fn asking_again_is_the_same_answer_as_reading_the_list() {
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
