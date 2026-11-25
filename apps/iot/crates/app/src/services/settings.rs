//! The device's own record: the words and the palette the panel prints with. A page names
//! either; nothing else writes them.

use crate::shared::intent::{BusinessIntent, SettingsIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use iot_core::drivers::input::clicks;

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
            BusinessIntent::Settings(SettingsIntent::SetClicks { pattern, window_ms }) => {
                clicks().set(*pattern, *window_ms);
            }
            _ => return (None, Effects::default()),
        }
        (None, Effects::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Service;
    use iot_core::drivers::input::{ClickPattern, clicks};

    #[test]
    fn a_settings_intent_sets_the_button_policy() {
        let before = (clicks().pattern(), clicks().window_ms() as u16);
        let mut state = DeviceState::default();
        SettingsService.apply(
            &BusinessIntent::Settings(SettingsIntent::SetClicks {
                pattern: ClickPattern::Triple,
                window_ms: 180,
            }),
            &mut state,
        );
        assert_eq!(clicks().pattern(), ClickPattern::Triple);
        assert_eq!(clicks().window_ms(), 180);
        clicks().set(before.0, before.1);
    }
}
