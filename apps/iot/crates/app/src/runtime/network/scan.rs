//! One scan round and the rest after it: what the list is, and what cuts the pause short.

use core::future::pending;

use crate::model::state::{DisplayPage, NetworkCommand};
use embassy_futures::select::{Either3, select3};
use embassy_time::{Duration, Timer};
use iot_core::drivers::network::{
    NetworkMode, SavedNetworks, ScanHistory, Station, scan_pause_after,
};

use super::store::any_marked;
use super::{NETWORK, publish_link};

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Rest {
    /// The pause is over or the panel came back: scan again.
    Again,
    /// A command the caller must act on; the queue does not keep a copy.
    Command(NetworkCommand),
    /// The panel left the list.
    Left,
}

pub(super) async fn scan<S>(
    station: &mut S,
    history: &mut ScanHistory,
    saved: Option<&SavedNetworks>,
    empty_rounds: &mut u8,
    idle_rounds: &mut u8,
    mode: NetworkMode,
) -> Rest
where
    S: Station,
{
    let empty = match station.scan().await {
        Ok(ranked) => {
            let empty = ranked.is_empty();
            publish_link(station.status().with_mode(mode), any_marked(saved));
            if let Some(list) = history.observe(&ranked, saved) {
                log::info!("[WIFI] {} access points, the page redraws", list.len());
                NETWORK.scan.publish(&list);
            } else {
                log::debug!("[WIFI] {} access points, unchanged", ranked.len());
            }
            NETWORK.scan.complete();
            empty
        }
        Err(error) => {
            log::error!("[WIFI] the radio would not scan: {error:?}");
            publish_link(station.status().with_mode(mode), any_marked(saved));
            false
        }
    };
    *empty_rounds = if empty {
        empty_rounds.saturating_add(1)
    } else {
        0
    };
    *idle_rounds = idle_rounds.saturating_add(1);
    let pause = scan_pause_after(mode, *empty_rounds, *idle_rounds);
    let clock = async {
        match pause {
            Some(ms) => Timer::after(Duration::from_millis(ms)).await,
            None => pending().await,
        }
    };
    match select3(clock, NETWORK.commands.receive(), async {
        if mode.is_setup() {
            NETWORK.page.wait_for_return(DisplayPage::Network).await;
        } else {
            NETWORK.page.wait_until_away(DisplayPage::Network).await;
        }
    })
    .await
    {
        Either3::Second(command) => Rest::Command(command),
        Either3::Third(()) if !mode.is_setup() => Rest::Left,
        _ => Rest::Again,
    }
}
