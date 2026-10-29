//! Takes a finished frame and writes it to the panel, unmodified, over and over.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_time::Instant;
use embedded_alloc::LlffHeap as Heap;
use iot_bsp_esp::SharedI2cDevice;
use iot_bsp_esp::components::gc2145::Gc2145;
use iot_bsp_esp::components::st7789::St7789;
use iot_bsp_esp::virtual_components::camera::{DESCRIPTOR_COUNT, FRAME_BYTES, Gc2145Capture};

const REPORT_MS: u64 = 2_000;

const HEAP_BYTES: usize = 64 * 1024;

static mut HEAP_MEM: [u8; HEAP_BYTES] = [0; HEAP_BYTES];

#[global_allocator]
static HEAP: Heap = Heap::empty();

const FINGERPRINT_STRIDE: usize = 997;

struct Fingerprint {
    last: Option<u32>,
    changed: u32,
    repeated: u32,
}

impl Fingerprint {
    const fn new() -> Self {
        Self {
            last: None,
            changed: 0,
            repeated: 0,
        }
    }

    fn observe(&mut self, frame: &[u8]) {
        let mut hash = 0x811c_9dc5u32;
        for &byte in frame.iter().step_by(FINGERPRINT_STRIDE) {
            hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
        }
        match self.last {
            Some(last) if last == hash => self.repeated = self.repeated.saturating_add(1),
            Some(_) => self.changed = self.changed.saturating_add(1),
            None => {}
        }
        self.last = Some(hash);
    }
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a private
    // static never aliased elsewhere.
    unsafe {
        HEAP.init(core::ptr::addr_of_mut!(HEAP_MEM) as usize, HEAP_BYTES);
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::warn!("[PROBE] camera on panel: no audio, touch, motion or buttons");

    let (panel, mut capture, timg0, from_cpu_intr) =
        match iot_bsp_esp::Board::new_camera_only(peripherals) {
            Ok(startup) => startup,
            Err(error) => {
                log::error!("[PROBE] camera bring-up failed, resetting: {error:?}");
                esp_hal::system::software_reset();
            }
        };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    run(panel, &mut capture).await
}

async fn run(mut panel: St7789, capture: &mut Gc2145Capture<Gc2145<SharedI2cDevice>>) -> ! {
    let start = Instant::now();
    let mut fingerprint = Fingerprint::new();

    match capture.sensor().read_window_geometry() {
        Ok(geometry) => log::info!(
            "[PROBE] part holds out={}x{} read={}x{} 0x99=0x{:02x} 0x9a=0x{:02x} \
             0xfd=0x{:02x} bins={:02x?}",
            geometry.out_width,
            geometry.out_height,
            geometry.win_width,
            geometry.win_height,
            geometry.subsample,
            geometry.subsample_mode,
            geometry.scalar,
            geometry.sub_bins,
        ),
        Err(error) => log::error!("[PROBE] geometry unreadable: {error:?}"),
    }

    let mut painted = 0u32;
    let mut last_report = Instant::now();
    let mut last_changed = 0u32;
    let mut last_repeated = 0u32;

    loop {
        let sample = capture.poll(start.elapsed().as_millis());
        if let Some(pixels) = capture.latest_frame() {
            fingerprint.observe(pixels);
            match panel.write_frame(pixels) {
                Ok(()) => painted = painted.saturating_add(1),
                Err(error) => log::error!("[PROBE] panel write failed: {error:?}"),
            }
        }

        let now = Instant::now();
        let window_ms = now.duration_since(last_report).as_millis();
        if window_ms >= REPORT_MS {
            if let Ok(exposure) = capture.sensor().read_exposure() {
                log::info!("[PROBE] sensor exposure {exposure} lines");
            }
            if let Ok((horizontal, vertical)) = capture.sensor().read_blanking() {
                log::info!("[PROBE] blanking h={horizontal} v={vertical} lines");
            }
            let new_frames = fingerprint.changed.saturating_sub(last_changed);
            let new_kb_s = u64::from(new_frames) * FRAME_BYTES as u64 / window_ms.max(1);
            log::info!(
                "[PROBE] painted {painted}, {new_frames} new in {window_ms} ms = {new_kb_s} KB/s of \
                 new sensor data (changed {}/{}, repeats {}/{} since boot)",
                fingerprint.changed,
                last_changed,
                fingerprint.repeated,
                last_repeated,
            );
            log::info!(
                "[PROBE] capture: {} frames, {} repeated polls, {} re-arms, {}/{} descriptors done",
                sample.frames,
                sample.repeated,
                sample.restarts,
                sample.finished,
                DESCRIPTOR_COUNT
            );
            last_changed = fingerprint.changed;
            last_repeated = fingerprint.repeated;
            last_report = now;
            painted = 0;
        }
    }
}
