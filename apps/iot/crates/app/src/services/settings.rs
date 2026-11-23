//! The device's own record: the words the panel prints. A page names the language; nothing
//! else writes it.

use crate::shared::intent::{BusinessIntent, SettingsIntent};
use crate::shared::model::{Command, DeviceState, Effects};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsService;

impl super::Service for SettingsService {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Settings(SettingsIntent::SetLang { lang }) = intent else {
            return (None, Effects::default());
        };
        state.lang = *lang;
        (None, Effects::default())
    }
}
