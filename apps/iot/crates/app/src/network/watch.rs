//! The panel's page as the radio reads it. The worker cannot reach device state, and the
//! questions it asks of the page — is the list up, has the panel come back — belong beside
//! the worker rather than in the handle it borrows.

use core::sync::atomic::Ordering;

use iot_core::state::{DisplayPage, NetworkCommand};

use crate::workers::{COMMAND_SLOTS, NetworkQueue, PageWatch};

impl PageWatch {
    pub fn shows(&self, page: DisplayPage) -> bool {
        self.page.load(Ordering::Relaxed) == page as u8
    }

    /// Waits until the panel is on `page`.
    pub async fn wait_for(&self, page: DisplayPage) {
        while !self.shows(page) {
            self.changed.wait().await;
        }
    }

    /// Edge, not level: a panel that never left `page` must not satisfy it.
    pub async fn wait_for_return(&self, page: DisplayPage) {
        let wanted = page as u8;
        let mut seen = self.page.load(Ordering::Relaxed);
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                continue;
            }
            seen = page;
            if page == wanted {
                return;
            }
        }
    }

    pub async fn wait_until_away(&self, page: DisplayPage) {
        let unwanted = page as u8;
        let mut seen = self.page.load(Ordering::Relaxed);
        if seen != unwanted {
            return;
        }
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                continue;
            }
            seen = page;
            if page != unwanted {
                return;
            }
        }
    }
}

/// Drops setup requests left over from a previous session; every other command goes back.
///
/// A cancel made while the last session was up must not close the next one, and a queue has
/// no `clear()` of its own. The array is as deep as the channel, so it holds everything that
/// is not stale.
pub(crate) fn discard_setup(commands: &NetworkQueue) {
    let mut kept: [Option<NetworkCommand>; COMMAND_SLOTS] = core::array::from_fn(|_| None);
    let mut held = 0;
    while let Ok(command) = commands.try_receive() {
        if command.is_setup() {
            continue;
        }
        if let Some(slot) = kept.get_mut(held) {
            *slot = Some(command);
            held += 1;
        }
    }
    for command in kept.into_iter().flatten() {
        let _ = commands.try_send(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::{Context, Waker};
    use iot_core::drivers::network::{JoinRequest, SetupHost, Ssid};

    fn cx() -> Context<'static> {
        Context::from_waker(Waker::noop())
    }

    #[test]
    fn a_cancel_left_over_from_the_last_session_does_not_survive_into_the_next() {
        let commands = NetworkQueue::new();
        assert!(
            commands
                .try_send(NetworkCommand::LeaveSetup {
                    from: SetupHost::AccessPoint
                })
                .is_ok()
        );
        let join = NetworkCommand::Join(JoinRequest::Held(Ssid::from_octets(b"cafe")));
        assert!(commands.try_send(join.clone()).is_ok());

        discard_setup(&commands);

        assert_eq!(
            commands.try_receive(),
            Ok(join),
            "the join is not setup state and goes back untouched"
        );
        assert!(
            commands.try_receive().is_err(),
            "the previous session's cancel is gone"
        );
    }

    #[test]
    fn a_rest_on_the_list_is_not_cut_short_by_the_list_being_up() {
        let watch = PageWatch::new();
        watch.publish(DisplayPage::Network);
        let mut level = core::pin::pin!(watch.wait_for(DisplayPage::Network));
        let mut back = core::pin::pin!(watch.wait_for_return(DisplayPage::Network));
        assert!(
            level.as_mut().poll(&mut cx()).is_ready(),
            "the level check answers \"is the list up?\", and it is"
        );
        assert!(
            back.as_mut().poll(&mut cx()).is_pending(),
            "a return cannot be satisfied by never having left"
        );

        watch.publish(DisplayPage::Ambient);
        assert!(back.as_mut().poll(&mut cx()).is_pending());
        watch.publish(DisplayPage::Network);
        assert!(
            back.as_mut().poll(&mut cx()).is_ready(),
            "away and back is the return the pause waits on"
        );
    }

    #[test]
    fn the_same_watch_answers_for_whichever_page_the_caller_names() {
        let watch = PageWatch::new();
        watch.publish(DisplayPage::Camera);

        assert!(watch.shows(DisplayPage::Camera));
        assert!(!watch.shows(DisplayPage::Network));
        let mut away = core::pin::pin!(watch.wait_until_away(DisplayPage::Network));
        assert!(
            away.as_mut().poll(&mut cx()).is_ready(),
            "the camera is already away from the network page"
        );
        let mut list = core::pin::pin!(watch.wait_for(DisplayPage::Network));
        assert!(
            list.as_mut().poll(&mut cx()).is_pending(),
            "and the camera is not the page a caller waiting for the list is after"
        );
    }
}
