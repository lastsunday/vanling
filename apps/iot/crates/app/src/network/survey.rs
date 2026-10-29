//! One survey round and the rest after it: what the list is, and what cuts the pause short.

use core::future::pending;

use embassy_futures::select::{Either6, select6};
use embassy_time::{Duration, Timer};
use iot_core::drivers::network::{SavedNetworks, Ssid, Station, SurveyHistory, survey_pause_after};

use super::publish;
use super::setup::Face;
use super::store::keeps_any;
use crate::render::{FORGET, JOIN, RESCAN};

/// What ended a survey round's rest, when it is something the caller has to act on.
///
/// A credential and a look-around request are both taken where they are read; only a forget names a
/// network the caller is the one holding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Rest {
    /// The rest ran out, or something asked for the next round early.
    Woken,
    /// The panel asked to forget a saved network.
    Forget(Ssid),
    /// The panel left the list's page. Only the browsing caller acts on it: in setup mode the
    /// list is behind a box the person may step away from without closing the mode.
    Left,
}

/// One survey round: look, publish through `history`, and rest.
///
/// Never ends; a credential or a "look now" cuts the rest short. `history` keeps rows aimable
/// against radio jitter, and an unchanged list is not republished — a redraw here is a whole frame.
pub(super) async fn survey<S>(
    station: &mut S,
    history: &mut SurveyHistory,
    saved: Option<&SavedNetworks>,
    empty_rounds: &mut u8,
    idle_rounds: &mut u8,
    face: Face,
) -> Rest
where
    S: Station,
{
    let empty = match station.scan().await {
        Ok(ranked) => {
            let empty = ranked.is_empty();
            // The phase first: the scan leaves the station `Off`, and the page draws the
            // list under that word, so a publish that carried only one of the two would
            // leave the page a round behind what it is showing. The mode is this loop's own
            // premise — it is only ever reached with the access point up — so it is said here
            // rather than left to a station that does not know about it.
            publish(face.status(station.status()), keeps_any(saved));
            if let Some(list) = history.observe(&ranked, saved) {
                log::info!("[WIFI] {} access points, the page redraws", list.len());
                crate::render::SCAN.publish(&list);
                crate::provision::publish_scan(&list);
            } else {
                // A round that changed nothing is the normal case while the page is up, and
                // the list it did not change is already on the panel.
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
    // How long that buys the radio is core's policy — see [`survey_pause_after`] — and this loop
    // is what waits it out. `None` is the access point's front, and it is not a pause of zero: no clock
    // asks for a round at all, so what is waited on is the page's own request.
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
            // Woken when the panel is put *back* on the list, so a page entered during a pause that has
            // grown to half a minute surveys at once. Not the level check: this pause is spent with the
            // list up, so that would be true on the first poll and the pause would be skipped every
            // time. The browsing end is the opposite question — the page leaving is what ends its
            // survey — and the two are one future because the face already says which end this is.
            async {
                if face.setup {
                    crate::render::PAGE.wait_for_return().await;
                } else {
                    crate::render::PAGE.wait_for_leave().await;
                }
            },
            // And woken by the strip's own "look around now", which is the same round this pause would
            // have started once it ran out: the panel asks because a person is looking at a list that
            // was read before they got there.
            RESCAN.ready(),
            FORGET.ready(),
            // A phone reading the page is the one thing a round must not interrupt: the scan is what
            // takes the access point off the air for its length, and a load landing in it is a load
            // the phone reports as no page at all. The same pause starts over.
            crate::provision::page_read(),
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
