//! Echo cancellation on a real room: the speaker playing while the microphone
//! listens, and the canceller fed both.
//!
//! Not `aec-probe` with a codec attached: that one asks what the arithmetic
//! costs, this one asks whether the wiring works — the pairing, which a mispaired
//! reference survives as quietly as a paired one, only with a worse number. So the
//! pairing's own figures are reported beside the ERLE. See
//! [`iot_app::probe::echo_room`] for what each column answers.
//!
//! It drives the product's own feed loop and capture poll, and takes the feed on
//! the interrupt executor exactly as the product does — the feed is the
//! microphone's clock source, so a probe that fed it inline would be measuring a
//! topology the product never runs.
#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::Heap;

type Board = iot_bsp_esp::Board<'static>;

/// The product's heap, and the reason it has to be the product's.
///
/// This was 32 KB on the argument that the probe paints no panel and so reserves
/// none of the frame buffer. That argument is about `.bss` and says nothing about
/// the heap — and `Board::new` brings up the panel and the camera whether or not
/// anything ever paints them. The panel takes 8 KB of SPI scratch and the camera
/// is boxed whole, so the 32 KB build died in `Board::new` with
/// `memory allocation of 9048 bytes failed`, before this probe ran a line.
const HEAP_BYTES: usize = 64 * 1024;

static mut HEAP_MEM: [u8; HEAP_BYTES] = [0; HEAP_BYTES];

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; HEAP_BYTES]>(),
        );
    }

    // 240 MHz for the same reason the product does it: the canceller's delay
    // search alone busts a 30 ms poll at 80 MHz, and a probe measuring the
    // canceller at a clock the product does not run would be measuring something
    // else.
    let peripherals = iot_chip_esp::chip_init_at(esp_hal::clock::CpuClock::_240MHz);
    iot_chip_esp::init_logging();
    log::info!("[ECHO] room probe: speaker and microphone together, no panel or motion");

    let (mut board, timg0, from_cpu_intr, feed_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[ECHO] bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);

    // The capture is taken directly rather than through `take_audio`, which would
    // box it behind the product's input pipeline and hide the canceller's own
    // numbers behind an envelope.
    let Some(capture) = board.take_capture() else {
        log::error!("[ECHO] no microphone wired, nothing to cancel from; resetting");
        esp_hal::system::software_reset();
    };

    // The feed runs on the interrupt executor, as the product runs it. Feeding
    // inline would be the one change that made this probe cheap and
    // untrustworthy: the feed *is* the microphone's clock source, so the
    // pairing's own timing is part of what is being measured.
    let feed = iot_chip_esp::start_feed_executor(feed_intr);
    iot_app::probe::echo_room::run(board, capture, feed).await
}
