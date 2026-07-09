use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use tracing_subscriber::{EnvFilter, reload};

pub trait ReloadHandle<L> {
    fn current(&self) -> Option<L>;

    fn reload(&self, new_value: L) -> Result<(), reload::Error>;
}

impl<L: Clone, S> ReloadHandle<L> for reload::Handle<L, S> {
    fn current(&self) -> Option<L> {
        Self::clone_current(self)
    }

    fn reload(&self, new_value: L) -> Result<(), reload::Error> {
        Self::reload(self, new_value)
    }
}

#[derive(Clone)]
pub struct LogLevelReloadHandles {
    handles: Arc<Mutex<HandleMap>>,
}

type HandleMap = HashMap<String, Handle>;
type Handle = Box<dyn ReloadHandle<EnvFilter> + Send + Sync>;

impl LogLevelReloadHandles {
    pub fn add(&self, name: &str, handle: Handle) {
        self.handles.lock().insert(name.into(), handle);
    }

    pub fn reload(&self, new_value: &EnvFilter, names: Option<&[&str]>) -> anyhow::Result<()> {
        self.handles
            .lock()
            .iter()
            .filter(|(name, _)| names.is_some_and(|names| names.contains(&name.as_str())))
            .for_each(|(_, handle)| {
                _ = handle.reload(new_value.clone()).inspect_err(|e| {
                    tracing::error!("reload filter: {e}");
                });
            });

        Ok(())
    }

    #[must_use]
    pub fn current(&self, name: &str) -> Option<EnvFilter> {
        self.handles
            .lock()
            .get(name)
            .map(|handle| handle.current())?
    }
}

impl Default for LogLevelReloadHandles {
    fn default() -> Self {
        Self {
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
