//! The device's own record: the words and the palette the panel prints with. A page names
//! either; nothing else writes them.

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
        match intent {
            BusinessIntent::Settings(SettingsIntent::SetLang { lang }) => state.lang = *lang,
            BusinessIntent::Settings(SettingsIntent::SetTheme { theme }) => state.theme = *theme,
            _ => return (None, Effects::default()),
        }
        (None, Effects::default())
    }
}
