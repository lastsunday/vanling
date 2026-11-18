//! The device's own navigation: the page cycle, the setup request, and what the splash lands
//! on. It reads the wiring, the link and the boot decision together, so it is not a page's.

use crate::shared::intent::{BusinessIntent, NavigationIntent};
use crate::shared::model::{Command, DeviceState, Effects, NetworkCommand, NoticeKind};
use crate::shared::route::PageId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavService;

impl super::Service for NavService {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Navigation(intent) = intent else {
            return (None, Effects::default());
        };
        match intent {
            NavigationIntent::NextPage => {
                state.nav.advance(state.backed);
                (None, Effects::default())
            }
            NavigationIntent::RequestSetup => {
                if state.page() == PageId::Splash {
                    state.setup_requested = true;
                }
                (None, Effects::default())
            }
            NavigationIntent::SplashOver => land_after_splash(state),
        }
    }
}

/// A landing answers with the boot decision the runtime cannot observe, on the same return path
/// as every other command.
fn land_after_splash(state: &mut DeviceState) -> (Option<Command>, Effects) {
    if state.page() != PageId::Splash {
        return (None, Effects::default());
    }
    let setup = state.setup_requested;
    state.nav.pop();
    if setup && state.backed.network {
        state.nav.go(PageId::Network, state.backed);
    }
    // A device that boots with nothing to join has nothing to say after the splash except
    // that: one amber burst, then the page's own light. A hold that asked for setup is not
    // this — the access point is the answer to it, and the mode has its own colour.
    let notice = (!setup
        && state.backed.network
        && !state.network.link.saved
        && !state.network.link.mode.is_setup()
        && state.network.note_offline_once())
    .then_some(NoticeKind::Offline);
    (
        Some(Command::Network(NetworkCommand::SplashOver { setup })),
        Effects {
            notice,
            ..Effects::default()
        },
    )
}
