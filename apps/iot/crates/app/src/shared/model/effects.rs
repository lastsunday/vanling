//! What a domain asked the rest of the device for, applied by the manager once that domain's
//! own state has settled. A domain never reaches into another's state to get it done.
//!
//! The command that leaves the core is not here: the page's asks answer with it directly, so
//! there is one path from an intent to the worker rather than two.

use iot_core::drivers::playback::Sound;

use super::NoticeKind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    pub sound: Option<Sound>,
    pub notice: Option<NoticeKind>,
}
