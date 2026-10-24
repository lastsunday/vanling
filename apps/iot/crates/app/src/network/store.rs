//! The copy on the part: which networks are kept, writing the one that just worked, and putting the
//! list's marks back after a save or a forget.

use alloc::boxed::Box;

use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{Credentials, LinkHint, SavedNetworks, Ssid, SurveyHistory};

use super::link::Provenance;

/// Whether this device keeps a network to go back to, which is what makes setup mode's `CANCEL` a
/// way out rather than a door that opens onto the same room.
pub(super) fn has_saved<R: FlashRegion>(saved: &Option<Box<Store<R>>>) -> bool {
    keeps_any(saved.as_deref().map(|store| store.networks()))
}

/// The network reconnect tries first, or `None` when there is nothing saved.
pub(super) fn first_saved<R: FlashRegion>(
    saved: &Option<Box<Store<R>>>,
) -> Option<(Credentials, Provenance)> {
    saved
        .as_ref()
        .and_then(|store| store.networks().iter().next())
        .map(|credentials| (credentials, Provenance::Stored))
}

/// Whether the store holds at least one network.
pub(super) fn keeps_any(saved: Option<&SavedNetworks>) -> bool {
    saved.is_some_and(|networks| !networks.is_empty())
}

/// Write the network the link just joined to the part it is kept in, whichever path named it.
/// Best effort: the link is up either way, and a failure only costs the next boot's copy.
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

/// Erase one saved network, and tell the page what is left.
///
/// The store is optional because the part is: a board whose build wired a radio but no credential
/// partition has nothing to forget, and saying so is the whole of what this can do there.
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

/// Put the store's own marks back on the list the page is showing. The store is the authority, so a
/// row says what the next boot will do; a board with no partition is a no-op. The last list is used
/// rather than a new round, and equal marks publish nothing.
pub(super) fn remark<R: FlashRegion>(history: &mut SurveyHistory, saved: Option<&Store<R>>) {
    if let Some(list) = history.marked(saved.map(|store| store.networks())) {
        crate::render::SCAN.publish(&list);
        crate::provision::publish_scan(&list);
    }
}
