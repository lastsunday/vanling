//! Build-time product configuration: the page the firmware comes up on and the demand it
//! starts with.
//!
//! Both come from `option_env!` and are resolved by `const fn`, so the run-time path never
//! turns them into a string — `@/development/iot/flashing.md` has the measurement behind that.

use iot_core::demand::{Override, parse_override};
use iot_core::state::DisplayPage;

pub(crate) fn boot_page() -> DisplayPage {
    if let Some(name) = UNKNOWN_BOOT_PAGE {
        log::warn!("[IOT] IOT_BOOT_PAGE={name:?} is not a page, booting on Ambient");
    }
    BOOT_PAGE
}

pub(crate) fn boot_override() -> Override {
    let parsed = parse_override(option_env!("IOT_DEMAND").unwrap_or(""));
    if option_env!("IOT_DEMAND").is_some() && !parsed.names_anything() {
        log::warn!("[IOT] IOT_DEMAND named nothing this firmware recognises: ignored");
    }
    parsed
}

/// The network this build was named, if it was named one. Probes join it regardless of the
/// panel; the product does not (`wifi_credentials`).
pub fn build_wifi_credentials() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("IOT_WIFI_SSID")?;
    let password = option_env!("IOT_WIFI_PSK")?;
    Some((ssid, password))
}

/// The network this build was named, unless the build is a bench run: a bench image has to
/// join whatever the panel names, not the credential its build happens to carry.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub(crate) fn wifi_credentials() -> Option<(&'static str, &'static str)> {
    if crate::bench::replay::BENCH {
        return None;
    }
    build_wifi_credentials()
}

const BOOT_PAGE: DisplayPage = boot_page_from_env(option_env!("IOT_BOOT_PAGE"));

const UNKNOWN_BOOT_PAGE: Option<&str> = unknown_page(option_env!("IOT_BOOT_PAGE"));

const fn boot_page_from_env(name: Option<&str>) -> DisplayPage {
    let Some(name) = name else {
        return DisplayPage::Ambient;
    };
    match bootable_page(name) {
        Some(page) => page,
        None => DisplayPage::Ambient,
    }
}

/// `Splash` is a page and has a name, but it is the transient the boot lands *through*, never
/// on, so it is not a boot page.
const fn bootable_page(name: &str) -> Option<DisplayPage> {
    match DisplayPage::named(name) {
        Some(DisplayPage::Splash) => None,
        other => other,
    }
}

const fn unknown_page(name: Option<&str>) -> Option<&str> {
    let Some(name) = name else {
        return None;
    };
    let trimmed = name.trim_ascii();
    if trimmed.is_empty() || bootable_page(trimmed).is_some() {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::{boot_page_from_env, unknown_page};
    use iot_core::state::DisplayPage;

    #[test]
    fn every_page_but_the_splash_answers_to_its_name() {
        for page in DisplayPage::ALL {
            if page == DisplayPage::Splash {
                continue;
            }
            assert_eq!(boot_page_from_env(Some(page.name())), page, "{page:?}");
        }
    }

    #[test]
    fn a_name_survives_case_and_stray_whitespace() {
        for name in ["CAMERA", "Camera", "  camera  ", "\tcamera\n"] {
            assert_eq!(
                boot_page_from_env(Some(name)),
                DisplayPage::Camera,
                "{name:?} should resolve"
            );
        }
    }

    #[test]
    fn anything_unusable_lands_on_ambient() {
        assert_eq!(boot_page_from_env(None), DisplayPage::Ambient, "unset");
        assert_eq!(boot_page_from_env(Some("")), DisplayPage::Ambient, "empty");
        assert_eq!(
            boot_page_from_env(Some("   ")),
            DisplayPage::Ambient,
            "blank"
        );
        assert_eq!(
            boot_page_from_env(Some("photograph")),
            DisplayPage::Ambient,
            "a name that is not a page"
        );
    }

    #[test]
    fn only_a_name_no_page_answers_to_is_reported() {
        for name in ["AMBIENT", "  attitude  ", "\tnetwork\n"] {
            assert_eq!(unknown_page(Some(name)), None, "{name:?}");
        }
        assert_eq!(unknown_page(None), None, "unset");
        assert_eq!(unknown_page(Some("   ")), None, "blank");
        assert_eq!(
            unknown_page(Some("photograph")),
            Some("photograph"),
            "an unknown name"
        );
    }

    #[test]
    fn the_splash_is_not_a_boot_page() {
        assert_eq!(boot_page_from_env(Some("splash")), DisplayPage::Ambient);
        assert_eq!(unknown_page(Some("splash")), Some("splash"));
    }
}
