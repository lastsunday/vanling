//! The copy on the part: which networks are kept, writing the one that just worked, and
//! putting the list's marks back after a save or a forget.

use alloc::boxed::Box;

use iot_core::drivers::credentials::{FlashRegion, Store};
use iot_core::drivers::network::{Credentials, LinkHint, SavedNetworks, ScanHistory, Ssid};

use super::link::Origin;
use crate::workers::NETWORK;

/// Whether the part holds any network at all. No store — a board with no partition — is the
/// same answer as an empty one.
pub(super) fn any<R: FlashRegion>(saved: Option<&Store<R>>) -> bool {
    saved.is_some_and(|store| !store.networks().is_empty())
}

/// Whether a scan's view of the store holds any network; the list carries the marks, not the
/// records.
pub(super) fn any_marked(saved: Option<&SavedNetworks>) -> bool {
    saved.is_some_and(|networks| !networks.is_empty())
}

pub(super) fn first_saved<R: FlashRegion>(
    saved: &Option<Box<Store<R>>>,
) -> Option<(Credentials, Origin)> {
    saved
        .as_ref()
        .and_then(|store| store.networks().iter().next())
        .map(|credentials| (credentials, Origin::Stored))
}

pub(super) fn save<R: FlashRegion>(
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

/// Forgets a network and hands the re-marked list back: the panel and the phone draw the same
/// marks, so neither may be updated without the other.
pub(super) fn forget<R: FlashRegion>(
    mut saved: Option<&mut Store<R>>,
    history: Option<&mut ScanHistory>,
    ssid: &Ssid,
) {
    match saved.as_deref_mut() {
        Some(store) => match store.forget(ssid) {
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
        },
        None => log::warn!("[WIFI] the panel asked to forget a network, but this board keeps none"),
    }
    if let Some(history) = history {
        remark_and_publish(history, saved.as_deref());
    }
}

/// Hands the re-marked list to both consumers at once. A refresh that reached one and not the
/// other would leave the phone and the panel showing different lists.
pub(super) fn remark_and_publish<R: FlashRegion>(
    history: &mut ScanHistory,
    saved: Option<&Store<R>>,
) {
    if let Some(list) = history.marked(saved.map(|store| store.networks())) {
        NETWORK.scan.publish(&list);
    }
}
