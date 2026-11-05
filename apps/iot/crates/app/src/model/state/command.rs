//! What an applied intent asked the runtime to do outside the model. One variant per
//! boundary; the system that owns that boundary takes its own and leaves the rest.

use iot_core::drivers::network::NetworkCommand;

/// The outbound half of an applied intent: everything the model cannot do itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// The network worker has a command waiting.
    Network(NetworkCommand),
}
