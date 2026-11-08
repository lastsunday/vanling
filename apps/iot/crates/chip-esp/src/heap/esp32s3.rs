use super::Window;

pub(crate) fn try_window() -> Result<(Window, &'static str), super::HeapError> {
    match crate::chips::esp32s3::psram_window() {
        crate::chips::esp32s3::PsramWindow::Ready(window) => Ok((window, "PSRAM")),
        crate::chips::esp32s3::PsramWindow::Unusable => {
            log::error!("[HEAP] PSRAM mapped and did not read back");
            Err(super::HeapError::Unavailable { source: "PSRAM" })
        }
        crate::chips::esp32s3::PsramWindow::Absent => {
            log::error!("[HEAP] this module has no PSRAM to map");
            Err(super::HeapError::Unavailable { source: "PSRAM" })
        }
    }
}
