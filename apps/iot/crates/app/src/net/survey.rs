//! One survey round and the rest after it: what the list is, and what cuts the pause short.

use core::future::pending;

use embassy_futures::select::{Either4, select4};
use embassy_time::{Duration, Timer};
use iot_core::drivers::network::{SavedNetworks, Station, SurveyHistory, survey_pause_after};
use iot_core::state::{DisplayPage, NetworkCommand};

use super::link::Panel;
use super::publish;
use super::setup::Face;
use super::store::keeps_any;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Rest {
    /// The pause is over, the panel came back, or the phone read the page: scan again.
    Again,
    /// A command the caller must act on; the queue does not keep a copy.
    Command(NetworkCommand),
    /// The panel left the list.
    Left,
}

pub(super) async fn survey<S>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: Option<&SavedNetworks>,
    empty_rounds: &mut u8,
    idle_rounds: &mut u8,
    face: Face,
    panel: Panel,
) -> Rest
where
    S: Station,
{
    let empty = match station.scan().await {
        Ok(ranked) => {
            let empty = ranked.is_empty();
            publish(panel.bus, face.status(station.status()), keeps_any(saved));
            if let Some(list) = history.observe(&ranked, saved) {
                log::info!("[WIFI] {} access points, the page redraws", list.len());
                super::publish_scan(panel.net, &list);
            } else {
                log::debug!("[WIFI] {} access points, unchanged", ranked.len());
            }
            empty
        }
        Err(error) => {
            log::error!("[WIFI] the radio would not scan: {error:?}");
            publish(panel.bus, face.status(station.status()), keeps_any(saved));
            false
        }
    };
    *empty_rounds = if empty {
        empty_rounds.saturating_add(1)
    } else {
        0
    };
    *idle_rounds = idle_rounds.saturating_add(1);
    let pause = survey_pause_after(face.front, *empty_rounds, *idle_rounds);
    let clock = async {
        match pause {
            Some(ms) => Timer::after(Duration::from_millis(ms)).await,
            None => pending().await,
        }
    };
    match select4(
        clock,
        panel.net.commands.receive(),
        async {
            if face.setup {
                panel.net.page.wait_for_return(DisplayPage::Network).await;
            } else {
                panel.net.page.wait_until_away(DisplayPage::Network).await;
            }
        },
        crate::net::provision::page_read(),
    )
    .await
    {
        Either4::Second(command) => Rest::Command(command),
        Either4::Third(()) if !face.setup => Rest::Left,
        _ => Rest::Again,
    }
}
