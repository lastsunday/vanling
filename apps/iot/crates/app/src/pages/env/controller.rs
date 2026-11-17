//! The Env page takes no gesture of its own yet: its bindings land here.

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::model::DeviceState;

pub(crate) fn bind(_op: &InputIntent, _current: &DeviceState) -> BusinessIntent {
    BusinessIntent::Invalid
}
