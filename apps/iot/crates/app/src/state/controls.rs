//! The Network page's controls as behaviour: what each box does, named by its outcome rather
//! than by the word the view puts on it.

use super::{Confirm, NetworkPage};
use crate::controls::ToolbarCell;
use crate::intent::{ActionIntent, NetworkPageIntent};
use iot_core::drivers::network::{LinkStatus, NetworkMode, SetupHost};

/// What a box does, which is also what the view has to call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Cancel,
    Scan,
    Wait,
    Leave,
    Forget,
    Keep,
    AccessPoint,
    Panel,
    Pass,
    NoPass,
}

/// One box: the outcome it asks for and the command a tap sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    pub kind: ControlKind,
    pub command: ActionIntent,
}

/// The action strip: one box, or two while setup mode has two verbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    left: Option<Control>,
    right: Control,
}

impl Strip {
    /// The command a tap on `cell` sends.
    pub fn command(self, cell: ToolbarCell) -> ActionIntent {
        match (cell, self.left) {
            (ToolbarCell::Left, Some(left)) => left.command,
            _ => self.right.command,
        }
    }

    /// What `cell` does.
    pub fn kind(self, cell: ToolbarCell) -> ControlKind {
        match (cell, self.left) {
            (ToolbarCell::Left, Some(left)) => left.kind,
            _ => self.right.kind,
        }
    }

    /// Whether the strip is split, which is also whether the left box is drawn.
    pub fn is_split(self) -> bool {
        self.left.is_some()
    }
}

/// The strip the page is offering right now.
pub fn strip(draft: NetworkPage, link: LinkStatus) -> Strip {
    if let NetworkMode::Setup { host, cancellable } = link.mode {
        let right = Control {
            kind: match host {
                SetupHost::Panel => ControlKind::AccessPoint,
                SetupHost::AccessPoint => ControlKind::Panel,
            },
            command: ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost),
        };
        let left = match (host, cancellable) {
            (_, true) => Some(Control {
                kind: ControlKind::Cancel,
                command: ActionIntent::NetworkPage(NetworkPageIntent::CloseSetup),
            }),
            (SetupHost::Panel, false) => Some(Control {
                kind: if draft.rescan_wait {
                    ControlKind::Wait
                } else {
                    ControlKind::Scan
                },
                command: ActionIntent::NetworkPage(NetworkPageIntent::ScanNow),
            }),
            (SetupHost::AccessPoint, false) => None,
        };
        return Strip { left, right };
    }
    if link.mode.is_browsing() {
        let left = Control {
            kind: if draft.rescan_wait {
                ControlKind::Wait
            } else {
                ControlKind::Scan
            },
            command: ActionIntent::NetworkPage(NetworkPageIntent::ScanNow),
        };
        let right = Control {
            kind: ControlKind::AccessPoint,
            command: ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost),
        };
        return Strip {
            left: Some(left),
            right,
        };
    }
    let right = if link.is_leavable() {
        Control {
            kind: ControlKind::Leave,
            command: ActionIntent::NetworkPage(NetworkPageIntent::Leave),
        }
    } else {
        Control {
            kind: if draft.rescan_wait {
                ControlKind::Wait
            } else {
                ControlKind::Scan
            },
            command: ActionIntent::NetworkPage(NetworkPageIntent::ScanNow),
        }
    };
    Strip { left: None, right }
}

/// The setup box's two answers: what each does, and what each asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answers {
    pub keep: Control,
    pub act: Control,
}

/// The answers to the box a phone's setup is read in.
pub fn answers(link: LinkStatus) -> Answers {
    let keep = if link.setup_password.is_some() {
        Control {
            kind: ControlKind::NoPass,
            command: ActionIntent::NetworkPage(NetworkPageIntent::SetOpenAccessPoint(true)),
        }
    } else {
        Control {
            kind: ControlKind::Pass,
            command: ActionIntent::NetworkPage(NetworkPageIntent::SetOpenAccessPoint(false)),
        }
    };
    Answers {
        keep,
        act: Control {
            kind: ControlKind::Panel,
            command: ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost),
        },
    }
}

/// What a confirmation's action button does: the thing itself, not the button it is.
pub fn confirm_verb(confirm: Confirm) -> ControlKind {
    match confirm {
        Confirm::Leave => ControlKind::Leave,
        Confirm::Forget(_) => ControlKind::Forget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::network::{LinkPhase, SetupPass};

    fn in_setup(host: SetupHost, cancellable: bool) -> LinkStatus {
        LinkStatus {
            mode: NetworkMode::Setup { host, cancellable },
            ..LinkStatus::default()
        }
    }

    #[test]
    fn a_link_offers_leave_and_an_idle_one_offers_the_next_round() {
        let linked = LinkStatus {
            phase: LinkPhase::Connected,
            ..LinkStatus::default()
        };
        let linked = strip(NetworkPage::default(), linked);
        assert!(!linked.is_split());
        assert_eq!(linked.kind(ToolbarCell::Left), ControlKind::Leave);
        assert_eq!(
            linked.command(ToolbarCell::Left),
            ActionIntent::NetworkPage(NetworkPageIntent::Leave)
        );
        assert_eq!(
            linked.command(ToolbarCell::Right),
            ActionIntent::NetworkPage(NetworkPageIntent::Leave)
        );

        let idle = strip(NetworkPage::default(), LinkStatus::default());
        assert_eq!(idle.kind(ToolbarCell::Right), ControlKind::Scan);
        assert_eq!(
            idle.command(ToolbarCell::Right),
            ActionIntent::NetworkPage(NetworkPageIntent::ScanNow)
        );

        let waiting = NetworkPage {
            rescan_wait: true,
            ..NetworkPage::default()
        };
        let waiting = strip(waiting, LinkStatus::default());
        assert_eq!(waiting.kind(ToolbarCell::Right), ControlKind::Wait);
        assert_eq!(
            waiting.command(ToolbarCell::Right),
            ActionIntent::NetworkPage(NetworkPageIntent::ScanNow)
        );
    }

    #[test]
    fn setup_offers_the_way_back_beside_the_front_switch() {
        let split = strip(NetworkPage::default(), in_setup(SetupHost::Panel, true));
        assert!(split.is_split());
        assert_eq!(split.kind(ToolbarCell::Left), ControlKind::Cancel);
        assert_eq!(
            split.command(ToolbarCell::Left),
            ActionIntent::NetworkPage(NetworkPageIntent::CloseSetup)
        );
        assert_eq!(split.kind(ToolbarCell::Right), ControlKind::AccessPoint);
        assert_eq!(
            split.command(ToolbarCell::Right),
            ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)
        );
    }

    #[test]
    fn the_offline_list_keeps_both_cells() {
        let browsing = LinkStatus {
            mode: NetworkMode::Browsing,
            ..LinkStatus::default()
        };
        let split = strip(NetworkPage::default(), browsing);
        assert!(split.is_split());
        assert_eq!(split.kind(ToolbarCell::Left), ControlKind::Scan);
        assert_eq!(
            split.command(ToolbarCell::Left),
            ActionIntent::NetworkPage(NetworkPageIntent::ScanNow)
        );
        assert_eq!(split.kind(ToolbarCell::Right), ControlKind::AccessPoint);
        assert_eq!(
            split.command(ToolbarCell::Right),
            ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)
        );
    }

    #[test]
    fn a_panel_with_nothing_to_go_back_to_still_has_the_next_round() {
        let split = strip(NetworkPage::default(), in_setup(SetupHost::Panel, false));
        assert!(split.is_split());
        assert_eq!(split.kind(ToolbarCell::Left), ControlKind::Scan);
        assert_eq!(
            split.command(ToolbarCell::Left),
            ActionIntent::NetworkPage(NetworkPageIntent::ScanNow)
        );
        assert_eq!(split.kind(ToolbarCell::Right), ControlKind::AccessPoint);

        let waiting = NetworkPage {
            rescan_wait: true,
            ..NetworkPage::default()
        };
        let waiting = strip(waiting, in_setup(SetupHost::Panel, false));
        assert_eq!(waiting.kind(ToolbarCell::Left), ControlKind::Wait);
    }

    #[test]
    fn the_phones_one_box_switches_on_either_half() {
        let one = strip(
            NetworkPage::default(),
            in_setup(SetupHost::AccessPoint, false),
        );
        assert!(!one.is_split());
        for cell in [ToolbarCell::Left, ToolbarCell::Right] {
            assert_eq!(one.kind(cell), ControlKind::Panel);
            assert_eq!(
                one.command(cell),
                ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)
            );
        }
    }

    #[test]
    fn the_passphrase_answer_names_the_state_it_moves_to() {
        let secured = LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::AccessPoint,
                cancellable: false,
            },
            setup_password: Some(SetupPass::from_array(*b"k3mq9t7w2x")),
            ..LinkStatus::default()
        };
        let secured = answers(secured);
        assert_eq!(secured.keep.kind, ControlKind::NoPass);
        assert_eq!(
            secured.keep.command,
            ActionIntent::NetworkPage(NetworkPageIntent::SetOpenAccessPoint(true))
        );
        assert_eq!(secured.act.kind, ControlKind::Panel);
        assert_eq!(
            secured.act.command,
            ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)
        );

        let open = LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::AccessPoint,
                cancellable: false,
            },
            ..LinkStatus::default()
        };
        let open = answers(open);
        assert_eq!(open.keep.kind, ControlKind::Pass);
        assert_eq!(
            open.keep.command,
            ActionIntent::NetworkPage(NetworkPageIntent::SetOpenAccessPoint(false))
        );
    }

    #[test]
    fn a_confirmation_is_named_by_what_it_does() {
        assert_eq!(confirm_verb(Confirm::Leave), ControlKind::Leave);
        assert_eq!(
            confirm_verb(Confirm::Forget(
                iot_core::drivers::network::AccessPoint::default()
            )),
            ControlKind::Forget
        );
    }
}
