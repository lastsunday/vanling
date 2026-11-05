//! The panel's page as the radio reads it. The worker cannot reach device state, and the
//! questions it asks of the page — is the list up, has the panel come back — belong beside
//! the worker rather than in the handle it borrows.

use core::sync::atomic::Ordering;

use crate::model::state::DisplayPage;

use crate::runtime::workers::{NetworkInbox, PageWatch};

impl PageWatch {
    pub fn shows(&self, page: DisplayPage) -> bool {
        self.page.load(Ordering::Relaxed) == page.code()
    }

    /// Waits until the panel is on `page`.
    pub async fn wait_for(&self, page: DisplayPage) {
        while !self.shows(page) {
            self.changed.wait().await;
        }
    }

    /// A panel that never left `page` must not satisfy it: the wait starts from the page the
    /// panel is on now and returns only after a change brings it back.
    pub async fn wait_for_return(&self, page: DisplayPage) {
        let wanted = page.code();
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
        let unwanted = page.code();
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
/// A cancel made while the last session was up must not close the next one.
pub(crate) fn discard_setup(commands: &NetworkInbox) {
    commands.retain(|command| !command.is_setup());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::state::NetworkCommand;
    use core::task::{Context, Waker};
    use iot_core::drivers::network::{JoinRequest, SetupHost, Ssid};

    fn cx() -> Context<'static> {
        Context::from_waker(Waker::noop())
    }

    #[test]
    fn a_cancel_left_over_from_the_last_session_does_not_survive_into_the_next() {
        let commands = NetworkInbox::new();
        commands.publish(NetworkCommand::LeaveSetup {
            from: SetupHost::AccessPoint,
        });
        let join = NetworkCommand::Join(JoinRequest::Held(Ssid::from_octets(b"cafe")));
        commands.publish(join.clone());

        discard_setup(&commands);

        assert_eq!(
            commands.try_receive(),
            Some(join),
            "the join is not setup state and goes back untouched"
        );
        assert!(
            commands.try_receive().is_none(),
            "the previous session's cancel is gone"
        );
    }

    #[test]
    fn a_fresh_watch_starts_on_the_manifest_default_page() {
        let watch = PageWatch::new();
        assert!(watch.shows(DisplayPage::Ambient), "the declared default");
        assert!(!watch.shows(DisplayPage::Network));
    }

    #[test]
    fn a_rest_on_the_list_is_not_cut_short_by_the_list_being_up() {
        let watch = PageWatch::new();
        watch.publish(DisplayPage::Network);
        let mut up = core::pin::pin!(watch.wait_for(DisplayPage::Network));
        let mut back = core::pin::pin!(watch.wait_for_return(DisplayPage::Network));
        assert!(
            up.as_mut().poll(&mut cx()).is_ready(),
            "the page check answers \"is the list up?\", and it is"
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
