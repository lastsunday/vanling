use esp_alloc::{HeapRegion, MemoryCapability};
use esp_hal::ram;

const RECLAIMED_BYTES: usize = 32 * 1024;

#[ram(unstable(dcache_reclaimed))]
static mut RECLAIMED_MEM: [core::mem::MaybeUninit<u8>; RECLAIMED_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; RECLAIMED_BYTES];

// The driver's share of `dram_seg`: 16 KiB, 48 KiB with the reclaimed region above.
// `[实测]` 32,204 B free at 64 KiB across a scan, association and 40 s of leases; 15,820 B at
// 48 KiB. The ledger is in `@/records/iot/wifi.md`.
const INTERNAL_BYTES: usize = 16 * 1024;

static mut INTERNAL_MEM: [core::mem::MaybeUninit<u8>; INTERNAL_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; INTERNAL_BYTES];

pub(crate) const TOTAL_BYTES: usize = INTERNAL_BYTES + RECLAIMED_BYTES;

pub(crate) fn add_regions() -> (usize, Option<usize>) {
    // SAFETY: called exactly once, before anything allocates — each region is a private
    // static that nothing else names, and `esp-alloc`'s heap is the one the driver's
    // `malloc` resolves to. Their contents are uninitialised on purpose, which is what the
    // allocator is for.
    unsafe {
        esp_alloc::HEAP.add_region(HeapRegion::new(
            core::ptr::addr_of_mut!(INTERNAL_MEM) as *mut u8,
            INTERNAL_BYTES,
            MemoryCapability::Internal.into(),
        ));
        esp_alloc::HEAP.add_region(HeapRegion::new(
            core::ptr::addr_of_mut!(RECLAIMED_MEM) as *mut u8,
            RECLAIMED_BYTES,
            MemoryCapability::Internal.into(),
        ));
    }
    (INTERNAL_BYTES, Some(RECLAIMED_BYTES))
}

const SYSTEM_WIFI_CLK_EN: u32 = 0x00FB9FCF;

const WIFI_BT_SDIO_CLK: u32 = (1 << 5) | (1 << 12) | (1 << 13);

const MODEM_RESET_FIELD_WHEN_PU: u32 = (1 << 0) | (1 << 1) | (1 << 2);

/// Un-gates the modem clocks and powers the WiFi domain up, the way esp-radio's uncalled
/// `init()` would have.
///
/// Call once, after the scheduler is up and **before** the radio is built: the driver's own
/// `wifi_init` runs after these and expects the domain powered. This is the S3 arm of
/// upstream's `radio_clocks/clocks_ll/esp32s3.rs` and `common_adapter.rs`, 10 µs delay
/// included. It does not steady the scan — measured both ways — but no driver should run on a
/// radio whose power state nobody set.
pub fn ungate_radio() {
    use esp_hal::peripherals::{APB_CTRL, RTC_CNTL};

    let clocks = || APB_CTRL::regs().wifi_clk_en().read().bits();
    let pwc = || RTC_CNTL::regs().dig_pwc().read().bits();
    let (clocks_before, pwc_before) = (clocks(), pwc());

    APB_CTRL::regs()
        .wifi_clk_en()
        .modify(|r, w| unsafe { w.bits(r.bits() & !WIFI_BT_SDIO_CLK | SYSTEM_WIFI_CLK_EN) });
    RTC_CNTL::regs()
        .dig_pwc()
        .modify(|_, w| w.wifi_force_pd().clear_bit());
    esp_hal::rom::ets_delay_us(10);
    APB_CTRL::regs()
        .wifi_rst_en()
        .modify(|r, w| unsafe { w.bits(r.bits() | MODEM_RESET_FIELD_WHEN_PU) });
    APB_CTRL::regs()
        .wifi_rst_en()
        .modify(|r, w| unsafe { w.bits(r.bits() & !MODEM_RESET_FIELD_WHEN_PU) });
    RTC_CNTL::regs()
        .dig_iso()
        .modify(|_, w| w.wifi_force_iso().clear_bit());
    esp_hal::rtc_cntl::WakeLock::acquire();

    log::debug!(
        "[WIFI] modem clocks {clocks_before:#010x} -> {:#010x}, dig_pwc {pwc_before:#010x} -> {:#010x} \
         (esp-radio's uncalled init())",
        clocks(),
        pwc()
    );
}
