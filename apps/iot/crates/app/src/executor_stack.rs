//! Headroom on the stack the driver half runs on when it is not on the coordinator's core.
//!
//! The size is a static in the chip layer, so it can only be answered there; a part with one
//! core has no second stack and answers nothing. Reporting it from the task that occupies it
//! keeps the number next to the hold it shares an executor with.

/// How deep the second core's stack has been (used, size), when this part has one and it was
/// started.
#[cfg(feature = "esp32s3")]
pub(crate) fn second_core_peak() -> Option<(usize, usize)> {
    iot_chip_esp::panel_core_stack_peak()
}

#[cfg(not(feature = "esp32s3"))]
pub(crate) fn second_core_peak() -> Option<(usize, usize)> {
    None
}
