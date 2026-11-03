//! The network worker's side of the coordinator, kept out of the loop body: the commands that
//! had nowhere to go, the page level it watches, the scan list coming back, and the edges
//! worth a log line.

use embassy_sync::channel::TrySendError;
use iot_core::drivers::network::SetupHost;
use iot_core::state::{
    DeviceManager, DeviceState, DisplayPage, LampLine, LightState, NetworkCommand,
};

use crate::workers::{Network, NetworkQueue};

/// Commands on their way to the worker, with room for the one that did not fit. The queue is
/// slack rather than a depth the panel can fill, so a command refused this pass is offered
/// again before any new intent is taken: the bus is the backlog, and a busy worker costs a
/// frame instead of a request the panel asked for and never sent.
pub(crate) struct NetworkOutbox {
    held: Option<NetworkCommand>,
}

impl NetworkOutbox {
    pub(crate) const fn new() -> Self {
        Self { held: None }
    }

    /// Whether a command is still waiting for room. While it is, no new intent is taken.
    pub(crate) const fn pending(&self) -> bool {
        self.held.is_some()
    }

    /// Offers the held command again, oldest request first.
    pub(crate) fn retry(&mut self, to: &NetworkQueue) {
        let Some(command) = self.held.take() else {
            return;
        };
        if let Err(TrySendError::Full(command)) = to.try_send(command) {
            self.held = Some(command);
        }
    }

    /// Hands over what an intent produced, holding it when the queue has no room. The caller
    /// takes no new intent while [`Self::pending`], so nothing here can replace a held command.
    pub(crate) fn send(&mut self, to: &NetworkQueue, command: Option<NetworkCommand>) {
        let Some(command) = command else {
            return;
        };
        if let Err(TrySendError::Full(command)) = to.try_send(command) {
            self.held = Some(command);
        }
    }
}

/// What the coordinator publishes to the network worker and the edges worth a log line, in
/// the order a pass sees them.
pub(crate) struct NetworkMirror {
    page: DisplayPage,
    front: Option<SetupHost>,
    lamp: Option<LightState>,
}

impl NetworkMirror {
    pub(crate) fn new(state: &DeviceState) -> Self {
        Self {
            page: state.page,
            front: state.network.link.mode.host(),
            lamp: state.link_notice(),
        }
    }

    /// Runs once per pass: the page level goes out, and the page it changed to, the setup host
    /// and the lamp say so once.
    pub(crate) fn observe(&mut self, network: &Network, state: &DeviceState) {
        network.page.publish(state.page);
        if state.page != self.page {
            log::info!("[INPUT] page is now {:?}", state.page);
            self.page = state.page;
        }
        let front = state.network.link.mode.host();
        if front != self.front {
            self.front = front;
            if let Some(host) = front {
                log::info!("[WIFI] setup mode is offered on the {host:?} host");
            }
        }
        let lamp = state.link_notice();
        if lamp != self.lamp {
            log::info!("{}", LampLine(lamp));
            self.lamp = lamp;
        }
    }

    /// Hands the worker's newest scan list to state, when it published one.
    pub(crate) fn drain(&mut self, network: &Network, manager: &mut DeviceManager) {
        if let Some(list) = network.scan.take() {
            manager.apply_scan(list);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(queue: &NetworkQueue) {
        while queue.try_send(NetworkCommand::ScanNow).is_ok() {}
    }

    #[test]
    fn a_command_with_room_goes_out_and_leaves_nothing_held() {
        let queue = NetworkQueue::new();
        let mut outbox = NetworkOutbox::new();
        outbox.send(&queue, Some(NetworkCommand::Leave));
        assert!(!outbox.pending(), "the queue had room, so nothing is held");
        assert_eq!(
            queue.try_receive(),
            Ok(NetworkCommand::Leave),
            "the command reached the worker"
        );
    }

    #[test]
    fn nothing_to_send_is_not_a_command() {
        let queue = NetworkQueue::new();
        let mut outbox = NetworkOutbox::new();
        outbox.send(&queue, None);
        assert!(!outbox.pending(), "an intent with no command holds nothing");
        assert!(queue.try_receive().is_err(), "and the queue stays empty");
    }

    #[test]
    fn a_command_with_no_room_is_held_and_offered_again() {
        let queue = NetworkQueue::new();
        let mut outbox = NetworkOutbox::new();
        full(&queue);
        outbox.send(&queue, Some(NetworkCommand::Leave));
        assert!(outbox.pending(), "the full queue left the command held");

        outbox.retry(&queue);
        assert!(outbox.pending(), "still full, still held");
        while queue.try_receive().is_ok() {}
        outbox.retry(&queue);
        assert!(!outbox.pending(), "the held command finally went out");
        assert_eq!(
            queue.try_receive(),
            Ok(NetworkCommand::Leave),
            "and it is the command that was held"
        );
    }

    #[test]
    fn retrying_an_empty_outbox_asks_nothing_of_the_queue() {
        let queue = NetworkQueue::new();
        let mut outbox = NetworkOutbox::new();
        outbox.retry(&queue);
        assert!(!outbox.pending());
        assert!(queue.try_receive().is_err(), "no command was invented");
    }
}
