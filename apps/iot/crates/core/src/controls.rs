//! The Network page's controls as behaviour: what each box does, named beside the word on it.
//!
//! A box is drawn, a finger lands on it, and the state layer is asked to do something. One row says
//! both the word and the command, [`crate::intent::translate`] reads the command off it, and the
//! panel draws the same label — a box cannot offer a word the tap does not carry.
//!
//! The table is a value rather than a `static` because the link decides which rows exist: setup
//! mode has a way back only when there is a network to go back to, and the passphrase answer names
//! the state it moves to rather than a toggle.

use crate::drivers::network::{Front, LinkStatus};
use crate::intent::{BusinessIntent, BusinessIntent::*};
use crate::keyboard::ToolbarCell;
use crate::state::{Confirm, NetworkSetup};

pub const CANCEL_LABEL: &[u8] = b"CANCEL";
pub const SCAN_LABEL: &[u8] = b"SCAN";
pub const WAIT_LABEL: &[u8] = b"WAIT";
pub const AP_LABEL: &[u8] = b"AP";
pub const PANEL_LABEL: &[u8] = b"PANEL";
pub const PASS_LABEL: &[u8] = b"PASS";
pub const NO_PASS_LABEL: &[u8] = b"NO PASS";
pub const LEAVE_LABEL: &[u8] = b"LEAVE";
pub const FORGET_LABEL: &[u8] = b"FORGET";
pub const KEEP_LABEL: &[u8] = b"KEEP";

/// One box: the word on it and the command a tap sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    pub label: &'static [u8],
    pub command: BusinessIntent,
}

/// The action strip: one box, or two while setup mode has two verbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    left: Option<Control>,
    right: Control,
}

impl Strip {
    /// The command a tap on `cell` sends. A one-box strip takes either half: the geometry has no
    /// dead side.
    pub fn command(self, cell: ToolbarCell) -> BusinessIntent {
        match (cell, self.left) {
            (ToolbarCell::Left, Some(left)) => left.command,
            _ => self.right.command,
        }
    }

    /// The word `cell` draws. A one-box strip prints the same word in both halves.
    pub fn label(self, cell: ToolbarCell) -> &'static [u8] {
        match (cell, self.left) {
            (ToolbarCell::Left, Some(left)) => left.label,
            _ => self.right.label,
        }
    }

    /// Whether the strip is split, which is also whether the left box is drawn.
    pub fn is_split(self) -> bool {
        self.left.is_some()
    }
}

/// The strip the page is offering right now.
pub fn strip(setup: NetworkSetup, link: LinkStatus) -> Strip {
    if link.setup {
        let right = Control {
            label: match link.front {
                Front::Panel => AP_LABEL,
                Front::AccessPoint => PANEL_LABEL,
            },
            command: SwitchFront,
        };
        let left = match (link.front, link.setup_cancellable) {
            (_, true) => Some(Control {
                label: CANCEL_LABEL,
                command: CloseSetup,
            }),
            // With no way back the panel's left half is still the live list's next round.
            (Front::Panel, false) => Some(Control {
                label: if setup.rescan_wait {
                    WAIT_LABEL
                } else {
                    SCAN_LABEL
                },
                command: ScanNow,
            }),
            // The phone's strip is one box, and either half of it is the switch.
            (Front::AccessPoint, false) => None,
        };
        return Strip { left, right };
    }
    let right = if link.is_leavable() {
        Control {
            label: LEAVE_LABEL,
            command: Leave,
        }
    } else {
        Control {
            label: if setup.rescan_wait {
                WAIT_LABEL
            } else {
                SCAN_LABEL
            },
            command: ScanNow,
        }
    };
    Strip { left: None, right }
}

/// The setup box's two answers: what each does, and what each is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answers {
    pub keep: Control,
    pub act: Control,
}

/// The answers to the box a phone's setup is read in.
pub fn answers(link: LinkStatus) -> Answers {
    let keep = if link.setup_password.is_some() {
        Control {
            label: NO_PASS_LABEL,
            command: SetOpenAccessPoint(true),
        }
    } else {
        Control {
            label: PASS_LABEL,
            command: SetOpenAccessPoint(false),
        }
    };
    Answers {
        keep,
        act: Control {
            label: PANEL_LABEL,
            command: SwitchFront,
        },
    }
}

/// The word on a confirmation's action button: the thing it does, not the button it is.
pub fn confirm_verb(confirm: Confirm) -> &'static [u8] {
    match confirm {
        Confirm::Leave => LEAVE_LABEL,
        Confirm::Forget(_) => FORGET_LABEL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::network::{LinkPhase, SetupPass};

    fn setup(front: Front, cancellable: bool) -> LinkStatus {
        LinkStatus {
            setup: true,
            front,
            setup_cancellable: cancellable,
            ..LinkStatus::default()
        }
    }

    #[test]
    fn a_link_offers_leave_and_an_idle_one_offers_the_next_round() {
        let linked = LinkStatus {
            phase: LinkPhase::Connected,
            ..LinkStatus::default()
        };
        let linked = strip(NetworkSetup::default(), linked);
        assert!(!linked.is_split());
        assert_eq!(linked.label(ToolbarCell::Left), LEAVE_LABEL);
        assert_eq!(linked.command(ToolbarCell::Left), Leave);
        assert_eq!(linked.command(ToolbarCell::Right), Leave);

        let idle = strip(NetworkSetup::default(), LinkStatus::default());
        assert_eq!(idle.label(ToolbarCell::Right), SCAN_LABEL);
        assert_eq!(idle.command(ToolbarCell::Right), ScanNow);

        let waiting = NetworkSetup {
            rescan_wait: true,
            ..NetworkSetup::default()
        };
        let waiting = strip(waiting, LinkStatus::default());
        assert_eq!(waiting.label(ToolbarCell::Right), WAIT_LABEL);
        assert_eq!(waiting.command(ToolbarCell::Right), ScanNow);
    }

    #[test]
    fn setup_offers_the_way_back_beside_the_front_switch() {
        let split = strip(NetworkSetup::default(), setup(Front::Panel, true));
        assert!(split.is_split());
        assert_eq!(split.label(ToolbarCell::Left), CANCEL_LABEL);
        assert_eq!(split.command(ToolbarCell::Left), CloseSetup);
        assert_eq!(split.label(ToolbarCell::Right), AP_LABEL);
        assert_eq!(split.command(ToolbarCell::Right), SwitchFront);
    }

    #[test]
    fn a_panel_with_nothing_to_go_back_to_still_has_the_next_round() {
        let split = strip(NetworkSetup::default(), setup(Front::Panel, false));
        assert!(split.is_split());
        assert_eq!(split.label(ToolbarCell::Left), SCAN_LABEL);
        assert_eq!(split.command(ToolbarCell::Left), ScanNow);
        assert_eq!(split.label(ToolbarCell::Right), AP_LABEL);

        let waiting = NetworkSetup {
            rescan_wait: true,
            ..NetworkSetup::default()
        };
        let waiting = strip(waiting, setup(Front::Panel, false));
        assert_eq!(waiting.label(ToolbarCell::Left), WAIT_LABEL);
    }

    #[test]
    fn the_phones_one_box_switches_on_either_half() {
        let one = strip(NetworkSetup::default(), setup(Front::AccessPoint, false));
        assert!(!one.is_split());
        for cell in [ToolbarCell::Left, ToolbarCell::Right] {
            assert_eq!(one.label(cell), PANEL_LABEL);
            assert_eq!(one.command(cell), SwitchFront);
        }
    }

    #[test]
    fn the_passphrase_answer_names_the_state_it_moves_to() {
        let secured = LinkStatus {
            setup: true,
            front: Front::AccessPoint,
            setup_password: Some(SetupPass::from_array(*b"k3mq9t7w2x")),
            ..LinkStatus::default()
        };
        let secured = answers(secured);
        assert_eq!(secured.keep.label, NO_PASS_LABEL);
        assert_eq!(secured.keep.command, SetOpenAccessPoint(true));
        assert_eq!(secured.act.label, PANEL_LABEL);
        assert_eq!(secured.act.command, SwitchFront);

        let open = LinkStatus {
            setup: true,
            front: Front::AccessPoint,
            ..LinkStatus::default()
        };
        let open = answers(open);
        assert_eq!(open.keep.label, PASS_LABEL);
        assert_eq!(open.keep.command, SetOpenAccessPoint(false));
    }

    #[test]
    fn a_confirmation_is_named_by_what_it_does() {
        assert_eq!(confirm_verb(Confirm::Leave), LEAVE_LABEL);
        assert_eq!(
            confirm_verb(Confirm::Forget(
                crate::drivers::network::AccessPoint::default()
            )),
            FORGET_LABEL
        );
    }
}
