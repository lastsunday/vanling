//! The link: the phase the radio reports, the scan list it published, and the one-shot news
//! it owes. What the page is in the middle of drafting is [`NetworkPage`](super::NetworkPage)'s,
//! not this record's; this one is written by the worker's reports and read by everyone else.

use crate::intent::LinkIntent;
use iot_core::drivers::network::{
    AccessPoint, ApList, LinkPhase, LinkStatus, NetworkMode, SetupHost,
};
use iot_core::drivers::playback::Sound;

use super::NoticeKind;
use super::device::Effects;

/// What the radio has told the device, and nothing the page is doing with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkState {
    /// The link: the phase, the access point and the address the network task last saw.
    pub link: LinkStatus,
    /// The board's own address, as the network task reported it at boot: `None` until then,
    /// and for good with no radio.
    pub mac: Option<[u8; 6]>,
    /// The ranked scan list the network task last published; `None` means nothing has
    /// scanned.
    pub aps: Option<ApList>,
    /// Whether this boot has already announced that the link is down, so an outage says its
    /// piece once rather than on every failed attempt.
    pub outage_announced: bool,
}

impl NetworkState {
    /// The link's intents, matched here rather than in the manager: the group is this
    /// domain's, so a new variant cannot be left unanswered without this file failing to
    /// build. What the rest of the device must do about it comes back as [`Effects`].
    pub(super) fn apply(&mut self, intent: LinkIntent) -> Effects {
        match intent {
            LinkIntent::LinkReport(status) => self.report(status),
            LinkIntent::Mac(mac) => {
                self.mac = Some(mac);
                Effects::default()
            }
        }
    }

    /// Fold one status report in and say what it is worth hearing: the first of two rings
    /// wins, because a ring over a sounding one is a drop. The page reacts to the same report
    /// through [`NetworkPage::on_link`](super::NetworkPage::on_link); nothing here writes it.
    fn report(&mut self, status: LinkStatus) -> Effects {
        let sound = if status.mode.is_setup() && !self.link.mode.is_setup() {
            Some(Sound::Rise)
        } else if status.phase == LinkPhase::Connected && self.link.phase != LinkPhase::Connected {
            Some(Sound::Tap)
        } else if matches!(status.phase, LinkPhase::Failed(_)) && self.link.phase != status.phase {
            Some(Sound::Error)
        } else {
            None
        };
        let notice =
            if status.phase == LinkPhase::Connected && self.link.phase != LinkPhase::Connected {
                self.outage_announced = false;
                Some(NoticeKind::Joined)
            } else {
                match status.phase {
                    LinkPhase::Failed(kind)
                        if self.link.phase != status.phase && !self.outage_announced =>
                    {
                        self.outage_announced = true;
                        Some(NoticeKind::for_failure(kind))
                    }
                    _ => None,
                }
            };
        self.link = status;
        Effects { sound, notice }
    }

    pub(super) fn set_scan(&mut self, list: ApList) {
        self.aps = Some(list);
    }

    /// The position of the row in the published list, if it is in it.
    pub(super) fn index_of(&self, ap: AccessPoint) -> Option<usize> {
        let list = self.aps?;
        list.iter().position(|row| row.is_same_network(&ap))
    }

    /// Whether the row at `index` is one this device has saved.
    pub(super) fn is_saved(&self, index: usize) -> bool {
        self.aps.is_some_and(|list| list.is_saved(index))
    }

    pub(super) fn len(&self) -> usize {
        self.aps.map_or(0, |list| list.len())
    }

    /// Announces the first boot with nothing to join, once a boot.
    pub(super) fn note_offline_once(&mut self) -> bool {
        if self.outage_announced {
            return false;
        }
        self.outage_announced = true;
        true
    }

    pub(super) fn setup_host(&self) -> Option<SetupHost> {
        match self.link.mode {
            NetworkMode::Setup { host, .. } => Some(host),
            NetworkMode::Browsing => Some(SetupHost::Panel),
            NetworkMode::Link => None,
        }
    }
}
