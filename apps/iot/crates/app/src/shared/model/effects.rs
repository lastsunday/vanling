//! What a service asked the rest of the device for, applied by the manager once that service's
//! own record has settled. A service never reaches into another's state to get it done.
//!
//! The command that leaves the core is not here: [`Service::apply`](crate::services::Service::apply)
//! returns it beside these, so an intent has one path to the worker rather than two.

use iot_core::drivers::playback::Sound;

use super::NoticeKind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    pub sound: Option<Sound>,
    pub notice: Option<NoticeKind>,
}
