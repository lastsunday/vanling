use alloc::vec::Vec;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Ticker};
use iot_core::drivers::input::{INPUT_BASE_MS, PollEntry};
use iot_core::intent::{Intent, recognize};

/// Intent channel: input task writes recognized operations, the render loop
/// drains and interprets.
pub type IntentBus = Channel<CriticalSectionRawMutex, Intent, 8>;

/// Intent bus storage; passed as `&'static` into the tasks so no consumer
/// reaches the global by name.
pub static INTENT_BUS: IntentBus = IntentBus::new();

/// Drains every input source at the shared base tick, gating each source to
/// its own cadence, and forwards recognized operations onto the intent bus.
/// `try_send` drops intents when the bus is full: dropping a press is
/// preferable to blocking the scan loop, which would skew debounce timing.
pub async fn input_task(intent_bus: &'static IntentBus, mut sources: Vec<PollEntry>) -> ! {
    let mut ticker = Ticker::every(Duration::from_millis(INPUT_BASE_MS));
    let mut now_ms: u64 = 0;
    loop {
        // Ticks stay anchored to the base clock even when a source takes
        // inconsistent time to sample, so slow devices never alias.
        ticker.next().await;
        now_ms = now_ms.wrapping_add(INPUT_BASE_MS);
        for entry in sources.iter_mut() {
            if entry.next_at_ms() > now_ms {
                continue;
            }
            // Recognition is stateless: it packages the raw event plus the
            // wiring-order source, never reading device state. The render loop
            // interprets the operation against the freshest snapshot, so
            // nothing stale ever gets re-applied.
            if let Some(event) = entry.poll(now_ms) {
                let intent = Intent::Operation(recognize(event, entry.source_id()));
                let _ = intent_bus.try_send(intent);
            }
            // Advance past the current time, keeping the source on its own
            // cadence grid with no catch-up burst.
            entry.advance_past(now_ms);
        }
    }
}
