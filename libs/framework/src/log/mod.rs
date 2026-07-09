pub mod capture;
pub mod color;
pub mod console;
pub mod fmt;
pub mod fmt_span;
mod reload;
mod suppress;

use std::sync::Arc;

pub use crate::logging::LoggingHandle;
pub use capture::Capture;
pub use console::{ConsoleFormat, ConsoleWriter, is_systemd_mode};
pub use reload::{LogLevelReloadHandles, ReloadHandle};
pub use suppress::Suppress;
pub use tracing::Level;
pub use tracing_core::{Event, Metadata};
pub use tracing_subscriber::EnvFilter;

pub struct Log {
    pub reload: LogLevelReloadHandles,
    pub capture: Arc<capture::State>,
}
