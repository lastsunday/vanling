//! The network worker's side of a frame: the page it watches, the scan list coming back, and
//! the changes worth a log line.

use iot_core::drivers::network::SetupHost;
use iot_core::state::{DeviceManager, DeviceState, DisplayPage, LampLine, LightState};

use crate::workers::Network;

use super::{Frame, System};

/// What the coordinator publishes to the network worker and the changes worth a log line, in
/// the order a frame sees them.
pub(crate) struct NetworkSystem {
    page: DisplayPage,
    front: Option<SetupHost>,
    lamp: Option<LightState>,
}

impl NetworkSystem {
    pub(crate) fn new(state: &DeviceState) -> Self {
        Self {
            page: state.page(),
            front: state.network.link.mode.host(),
            lamp: state.link_notice(),
        }
    }

    /// Runs once per frame: the page goes out, and the page it changed to, the setup host and
    /// the lamp say so once.
    fn observe(&mut self, network: &Network, state: &DeviceState) {
        let page = state.page();
        network.page.publish(page);
        if page != self.page {
            log::info!("[INPUT] page is now {page:?}");
            self.page = page;
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
    fn drain(&mut self, network: &Network, manager: &mut DeviceManager) {
        if let Some(list) = network.scan.take() {
            manager.apply_scan(list);
        }
    }
}

impl System for NetworkSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        self.observe(&frame.workers.network, frame.manager.state_ref());
        self.drain(&frame.workers.network, frame.manager);
    }
}
