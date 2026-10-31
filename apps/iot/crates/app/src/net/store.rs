//! The copy on the part: which networks are kept, writing the one that just worked, and
//! putting the list's marks back after a save or a forget.

use alloc::boxed::Box;

use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{ApList, Credentials, LinkHint, SavedNetworks, ScanHistory, Ssid};

use super::link::Provenance;

pub(super) fn has_saved<R: FlashRegion>(saved: &Option<Box<Store<R>>>) -> bool {
    keeps_any(saved.as_deref().map(|store| store.networks()))
}

pub(super) fn first_saved<R: FlashRegion>(
    saved: &Option<Box<Store<R>>>,
) -> Option<(Credentials, Provenance)> {
    saved
        .as_ref()
        .and_then(|store| store.networks().iter().next())
        .map(|credentials| (credentials, Provenance::Stored))
}

pub(super) fn keeps_any(saved: Option<&SavedNetworks>) -> bool {
    saved.is_some_and(|networks| !networks.is_empty())
}

pub(super) fn keep<R: FlashRegion>(
    store: &mut Store<R>,
    credentials: &Credentials,
    hint: LinkHint,
) {
    match store.remember_with_hint(credentials, hint) {
        Ok(true) => {
            log::info!("[WIFI] this network was already saved, so the saved copy was updated")
        }
        Ok(false) => log::info!("[WIFI] this network is saved for the next boot"),
        Err(error) => {
            log::warn!("[WIFI] the part would not keep the network ({error:?}); this session only")
        }
    }
}

pub(super) fn drop_saved<R: FlashRegion>(store: Option<&mut Store<R>>, ssid: &Ssid) {
    let Some(store) = store else {
        log::warn!("[WIFI] the panel asked to forget a network, but this board keeps none");
        return;
    };
    match store.forget(ssid) {
        Ok(true) => log::info!(
            "[WIFI] {:?} is forgotten; the next boot will not join it",
            ssid.as_display_text().unwrap_or("<unprintable>"),
        ),
        Ok(false) => log::info!(
            "[WIFI] the panel asked to forget {:?}, which this device had not saved",
            ssid.as_display_text().unwrap_or("<unprintable>"),
        ),
        Err(error) => log::warn!(
            "[WIFI] the part would not drop the saved network ({error:?}); this session only"
        ),
    }
}

/// Recomputes the list's saved marks after a save or a forget, for the caller to publish.
pub(super) fn remark<R: FlashRegion>(
    history: &mut ScanHistory,
    saved: Option<&Store<R>>,
) -> Option<ApList> {
    history.marked(saved.map(|store| store.networks()))
}
