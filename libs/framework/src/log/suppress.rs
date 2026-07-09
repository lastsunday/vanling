use tracing_subscriber::EnvFilter;

use super::LogLevelReloadHandles;

pub struct Suppress {
    reload_handles: LogLevelReloadHandles,
    restore: EnvFilter,
}

impl Suppress {
    pub fn new(reload_handles: &LogLevelReloadHandles) -> Self {
        let handle = "console";
        let suppress = EnvFilter::default();
        let restore = reload_handles
            .current(handle)
            .unwrap_or_else(|| EnvFilter::try_new("").unwrap_or_default());

        reload_handles
            .reload(&suppress, Some(&[handle]))
            .expect("log filter reloaded");

        Self {
            reload_handles: reload_handles.clone(),
            restore,
        }
    }
}

impl Drop for Suppress {
    fn drop(&mut self) {
        self.reload_handles
            .reload(&self.restore, Some(&["console"]))
            .expect("log filter reloaded");
    }
}
