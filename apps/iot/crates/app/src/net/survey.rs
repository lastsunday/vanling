//! One survey round and the rest after it: what the list is, and what cuts the pause short.

use core::future::pending;

use embassy_futures::select::{Either6, select6};
use embassy_time::{Duration, Timer};
use iot_core::drivers::network::{SavedNetworks, Ssid, Station, SurveyHistory, survey_pause_after};
use iot_core::state::DisplayPage;

use super::publish;
use super::setup::Face;
use super::store::keeps_any;
use crate::bridge::{FORGET, JOIN, PageWatch, RESCAN};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Rest {
    Woken,
    Forget(Ssid),
    Left,
}

pub(super) async fn survey<S>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: Option<&SavedNetworks>,
    empty_rounds: &mut u8,
    idle_rounds: &mut u8,
    face: Face,
    page: &'static PageWatch,
) -> Rest
where
    S: Station,
{
    let empty = match station.scan().await {
        Ok(ranked) => {
            let empty = ranked.is_empty();
            publish(face.status(station.status()), keeps_any(saved));
            if let Some(list) = history.observe(&ranked, saved) {
                log::info!("[WIFI] {} access points, the page redraws", list.len());
                super::publish_scan(&list);
            } else {
                log::debug!("[WIFI] {} access points, unchanged", ranked.len());
            }
            empty
        }
        Err(error) => {
            log::error!("[WIFI] the radio would not scan: {error:?}");
            publish(face.status(station.status()), keeps_any(saved));
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
    loop {
        let clock = async {
            match pause {
                Some(ms) => Timer::after(Duration::from_millis(ms)).await,
                None => pending().await,
            }
        };
        match select6(
            clock,
            JOIN.ready(),
            async {
                if face.setup {
                    page.wait_for_return(DisplayPage::Network).await;
                } else {
                    page.wait_until_away(DisplayPage::Network).await;
                }
            },
            RESCAN.ready(),
            FORGET.ready(),
            crate::net::provision::page_read(),
        )
        .await
        {
            Either6::Third(()) if !face.setup => return Rest::Left,
            Either6::Fifth(ssid) => return Rest::Forget(ssid),
            Either6::Sixth(()) => {}
            _ => return Rest::Woken,
        }
    }
}
