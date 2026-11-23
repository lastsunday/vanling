//! Build-time product configuration: the page the firmware comes up on, the demand it starts
//! with, and whether this build is a panel bench.
//!
//! They come from `option_env!` and are resolved by `const fn`, so the run-time path never
//! turns them into a string — `@/development/iot/flashing.md` has the measurement behind that.
//! The bench flag lives here rather than in `bench` so the network worker never names the
//! harness that suppresses its build credential.

use crate::shared::i18n::Lang;
use crate::state::PageId;
use iot_core::demand::{Override, parse_override};

pub(crate) fn lang() -> Lang {
    if let Some(name) = UNKNOWN_LANG {
        log::warn!("[IOT] IOT_LANG={name:?} is not a language, printing English");
    }
    LANG
}

pub(crate) fn boot_page() -> PageId {
    if let Some(name) = UNKNOWN_BOOT_PAGE {
        log::warn!("[IOT] IOT_BOOT_PAGE={name:?} is not a page, booting on Input");
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
/// panel; the product's own path decides separately (`network::link`).
pub fn build_wifi_credentials() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("IOT_WIFI_SSID")?;
    let password = option_env!("IOT_WIFI_PSK")?;
    Some((ssid, password))
}

/// Whether this build replays a credential through the panel rather than being handed one.
/// Only the credential choice below asks, so the flag exists only where the radio does.
#[cfg(feature = "wifi")]
const BENCH: bool = option_env!("IOT_PANEL_BENCH").is_some();

/// The network this build was named, unless the build is a bench run: a bench image has to join
/// whatever the panel names, not the credential its build happens to carry.
#[cfg(feature = "wifi")]
pub(crate) fn build_credentials() -> Option<(&'static str, &'static str)> {
    if BENCH {
        return None;
    }
    build_wifi_credentials()
}

const BOOT_PAGE: PageId = boot_page_from_env(option_env!("IOT_BOOT_PAGE"));

const LANG: Lang = lang_from_env(option_env!("IOT_LANG"));

const UNKNOWN_LANG: Option<&str> = unknown_lang(option_env!("IOT_LANG"));

const fn lang_from_env(name: Option<&str>) -> Lang {
    let Some(name) = name else {
        return Lang::En;
    };
    match Lang::from_name(name) {
        Some(lang) => lang,
        None => Lang::En,
    }
}

const fn unknown_lang(name: Option<&str>) -> Option<&str> {
    let Some(name) = name else {
        return None;
    };
    let trimmed = name.trim_ascii();
    if trimmed.is_empty() || Lang::from_name(trimmed).is_some() {
        return None;
    }
    Some(name)
}

const UNKNOWN_BOOT_PAGE: Option<&str> = unknown_page(option_env!("IOT_BOOT_PAGE"));

const fn boot_page_from_env(name: Option<&str>) -> PageId {
    let Some(name) = name else {
        return PageId::Menu;
    };
    match bootable_page(name) {
        Some(page) => page,
        None => PageId::Input,
    }
}

/// `Splash` is a page and has a name, but it is the transient the boot lands *through*, never
/// on, so its manifest row is not bootable.
const fn bootable_page(name: &str) -> Option<PageId> {
    match PageId::from_path(name) {
        Some(page) if page.bootable() => Some(page),
        _ => None,
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
    use super::{boot_page_from_env, lang_from_env, unknown_lang, unknown_page};
    use crate::shared::i18n::Lang;
    use crate::shared::route::SPLASH_MS;
    use crate::state::PageId;

    #[test]
    fn every_bootable_page_answers_to_its_name() {
        for page in PageId::ALL {
            if !page.bootable() {
                continue;
            }
            assert_eq!(boot_page_from_env(Some(page.path())), page, "{page:?}");
        }
    }

    #[test]
    fn a_name_survives_case_and_stray_whitespace() {
        for name in ["CAMERA", "Camera", "  camera  ", "\tcamera\n"] {
            assert_eq!(
                boot_page_from_env(Some(name)),
                PageId::Camera,
                "{name:?} should resolve"
            );
        }
    }

    #[test]
    fn an_unnamed_build_comes_up_on_the_menu() {
        assert_eq!(boot_page_from_env(None), PageId::Menu, "unset");
    }

    #[test]
    fn anything_unusable_lands_on_input() {
        assert_eq!(boot_page_from_env(Some("")), PageId::Input, "empty");
        assert_eq!(boot_page_from_env(Some("   ")), PageId::Input, "blank");
        assert_eq!(
            boot_page_from_env(Some("photograph")),
            PageId::Input,
            "a name that is not a page"
        );
    }

    #[test]
    fn only_a_name_no_page_answers_to_is_reported() {
        for name in ["INPUT", "  attitude  ", "\tnetwork\n"] {
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
        assert!(!PageId::Splash.bootable());
        assert_eq!(PageId::Splash.timeout_ms(), Some(SPLASH_MS));
        assert_eq!(boot_page_from_env(Some("splash")), PageId::Input);
        assert_eq!(unknown_page(Some("splash")), Some("splash"));
    }

    #[test]
    fn a_language_name_survives_case_and_space() {
        assert_eq!(lang_from_env(None), Lang::En, "unset prints English");
        assert_eq!(lang_from_env(Some(" zh ")), Lang::ZhHans);
        assert_eq!(lang_from_env(Some("EN-us")), Lang::En);
        assert_eq!(
            lang_from_env(Some("fr")),
            Lang::En,
            "unknown prints English"
        );
        assert_eq!(unknown_lang(Some("fr")), Some("fr"));
        assert_eq!(unknown_lang(Some("  ")), None);
        assert_eq!(unknown_lang(Some("zh")), None);
    }
}
