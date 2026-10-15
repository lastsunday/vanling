use super::Window;

/// This part's PSRAM, which [`crate::chip_init_at`] mapped and proved on the way past.
pub(crate) fn window() -> (Window, &'static str) {
    match crate::esp32s3::psram_window() {
        crate::esp32s3::PsramWindow::Ready(window) => (window, "PSRAM"),
        crate::esp32s3::PsramWindow::Unusable => {
            log::error!("[HEAP] PSRAM mapped and did not read back, resetting");
            esp_hal::system::software_reset();
        }
        crate::esp32s3::PsramWindow::Absent => {
            log::error!("[HEAP] this module has no PSRAM to map, resetting");
            esp_hal::system::software_reset();
        }
    }
}
