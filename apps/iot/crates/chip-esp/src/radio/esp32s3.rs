use esp_alloc::{HeapRegion, MemoryCapability};
use esp_hal::ram;

/// The space the bootloader's flash cache gave back.
const RECLAIMED_BYTES: usize = 32 * 1024;

/// The bootloader's flash cache, given back.
///
/// `unstable(dcache_reclaimed)`, **not** `reclaimed`, and the two are not neighbours:
/// `reclaimed` places a static in `.dram2_uninit`, which lands in `dram2_seg` — the region
/// this repository's main-stack script reaches into — so a region placed there pushes the
/// stack out and the link fails with "cannot move location counter backwards".
///
/// Uninitialised rather than zeroed, for the same reason as the static beside it: the loader
/// does not write that region, so `[0; N]` would claim a zeroing that never happens and leave
/// the driver reading whatever the bootloader left there.
#[ram(unstable(dcache_reclaimed))]
static mut RECLAIMED_MEM: [core::mem::MaybeUninit<u8>; RECLAIMED_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; RECLAIMED_BYTES];

/// The driver's share of `dram_seg`, the only internal DRAM left that nothing has to be
/// traded for.
///
/// **32 KiB, not the documented 47–57 KiB**, which the reclaimed region above makes up. This
/// part is where that split was measured: the app's own heap moved to PSRAM, which took the
/// main stack from 63,788 B (1,748 B under the asserted floor) to 125,788 B with 39,809 B in
/// use — and the stack is the one thing on this part whose peak is instrumented, so giving
/// away half of it is a measurement rather than a gamble.
const INTERNAL_BYTES: usize = 32 * 1024;

/// The `dram_seg` half, a plain static: `dram_seg` is internal DRAM like any other, and this
/// is the only place left on this part that is not already spoken for.
static mut INTERNAL_MEM: [core::mem::MaybeUninit<u8>; INTERNAL_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; INTERNAL_BYTES];

/// What this part's regions add up to, for [`super::radio_heap_total`].
pub(crate) const TOTAL_BYTES: usize = INTERNAL_BYTES + RECLAIMED_BYTES;

/// Hands this part's regions to `esp-alloc` and reports what they were: `dram_seg` bytes, and
/// the reclaimed region this part has above them.
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

/// The modem clock mask esp-radio would have written, had anything called its `init()`.
/// Before [`ungate_radio`] this part sits at the ROM's `0xfffce030` and bit 17 of
/// `dig_pwc` (`wifi_force_pd`) is set; the radio works anyway.
const SYSTEM_WIFI_CLK_EN: u32 = 0x00FB9FCF;

/// The WiFi bits of the SDIO/I2C clock group, which the WiFi mask clears.
const WIFI_BT_SDIO_CLK: u32 = (1 << 5) | (1 << 12) | (1 << 13);

/// WiFi baseband, RF frontend and WiFi MAC, the three esp-radio pulses a reset through.
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
    // The wait esp-idf's `esp_wifi_bt_power_domain_on` puts between ungating the clocks
    // and touching the domain.
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

    log::info!(
        "[RADIO] modem clocks {clocks_before:#010x} -> {:#010x}, dig_pwc {pwc_before:#010x} -> {:#010x} \
         (esp-radio's uncalled init())",
        clocks(),
        pwc()
    );
}
