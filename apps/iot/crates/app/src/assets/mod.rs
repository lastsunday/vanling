//! The product's own bytes: the name its setup access point carries and the portal it serves.

#[cfg(any(test, feature = "wifi"))]
use iot_core::provision::Site;

pub const AP_PREFIX: &[u8] = b"vanling-";

/// Safe mode's own name: it must not be mistaken for the setup portal, because the page it
/// serves is read-only and the access point is open.
#[cfg(any(test, feature = "wifi"))]
pub const SAFE_AP_PREFIX: &[u8] = b"vanling-safe-";

#[cfg(any(test, feature = "wifi"))]
pub const SITE: Site = Site {
    page: include_str!("portal.html"),
    done: include_str!("done.html"),
    favicon: include_str!("favicon.svg"),
};

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::provision::{MAC_TEXT_BYTES, MAC_TOKEN, SEED_AT, SEED_SLOT, setup_ap_ssid};

    #[test]
    fn the_access_point_is_named_for_the_product() {
        assert_eq!(
            setup_ap_ssid(AP_PREFIX, [0xaa, 0x00, 0x00, 0x00, 0x1f, 0xc2]).as_display_text(),
            Some("vanling-1fc2")
        );
    }

    #[test]
    fn safe_mode_does_not_answer_to_the_setup_name() {
        assert_eq!(
            setup_ap_ssid(SAFE_AP_PREFIX, [0xaa, 0x00, 0x00, 0x00, 0x1f, 0xc2]).as_display_text(),
            Some("vanling-safe-1fc2")
        );
    }

    #[test]
    fn the_two_documents_are_documents() {
        assert!(SITE.page.contains("<!doctype html>"));
        assert!(SITE.page.contains("action=\"/submit\""));
        assert!(SITE.done.contains("<!doctype html>"));
    }

    #[test]
    fn both_pages_carry_the_icon_and_leave_one_place_for_the_address() {
        assert_eq!(MAC_TOKEN.len(), MAC_TEXT_BYTES);
        for page in [SITE.page, SITE.done] {
            assert!(
                page.contains("rel=\"icon\" href=\"data:image/svg+xml,"),
                "no icon carried"
            );
            assert_eq!(page.matches(MAC_TOKEN).count(), 1, "one address per page");
        }
    }

    #[test]
    fn the_page_waits_for_the_round_instead_of_the_clock() {
        assert!(
            SITE.page.contains("X-Scan-Generation"),
            "the page reads the round the device named"
        );
        assert!(
            SITE.page.contains("fetch('/rescan'"),
            "the page asks for a round before polling"
        );
        assert!(
            SITE.page.contains("503"),
            "an unfinished survey is not read as an empty room"
        );
        assert!(
            !SITE.page.contains("wait(1800)"),
            "the fixed wait was a guess at the radio's timing"
        );
    }

    #[test]
    fn the_page_leaves_one_slot_for_the_list_and_it_stands_on_its_own() {
        assert_eq!(SITE.page.matches(SEED_AT).count(), 1, "one slot");
        let at = SITE.page.find(SEED_AT).expect("the slot") + SEED_AT.len();
        assert_eq!(&SITE.page[at..at + SEED_SLOT], "[]");
    }

    #[test]
    fn the_pages_speak_more_than_one_language() {
        for page in [SITE.page, SITE.done] {
            assert!(page.contains("navigator.languages"), "no language detected");
            assert!(
                page.contains("localStorage.getItem('lang')"),
                "no choice kept"
            );
            assert!(page.contains("zh-Hans"), "no Chinese");
        }
        assert!(
            SITE.page.contains("data-lang=\"zh\""),
            "the switch is missing"
        );
    }
}
