//! What an applied intent asked the runtime to do outside the model. One variant per
//! boundary; the system that owns that boundary takes its own and leaves the rest.

use core::any::TypeId;

use iot_core::drivers::network::NetworkCommand;

/// The outbound half of an applied intent: everything the model cannot do itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// The network worker has a command waiting.
    Network(NetworkCommand),
}

impl Command {
    /// The payload's type, which is the key a system declares with
    /// [`System::takes`](crate::runtime::system::System::takes).
    pub(crate) fn type_id(&self) -> TypeId {
        match self {
            Self::Network(_) => TypeId::of::<NetworkCommand>(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_the_payload_type() {
        assert_eq!(
            Command::Network(NetworkCommand::Leave).type_id(),
            TypeId::of::<NetworkCommand>()
        );
    }
}
