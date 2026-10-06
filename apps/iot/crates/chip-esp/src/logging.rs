//! Uptime-timestamped `log` logger.
//!
//! esp-println's built-in logger prints bare `<level> - <msg>`; the stall
//! diagnostics need wall-clock correlation with a human watching the panel, so
//! every line gets an `hh:mm:ss.mmm` prefix from the HAL's system timer since
//! boot (no RTC on the boards — "now" is uptime). Output still goes through
//! the esp-println stream. One static instance is shared by all tasks; records
//! are formatted inline, never buffered.
//!
//! The stream does not serialise writers. `esp-println` wraps writes in a lock
//! only under its `critical-section` feature, which this crate does not enable
//! (`Cargo.toml`), so a line from the cooperative task can be interleaved by one
//! from an interrupt executor. Every task here that must keep its timing
//! therefore publishes counters and leaves the printing to the cooperative side
//! — see `FeedStats`. A line is also flushed in 32-byte chunks, so it is not
//! atomic even against a writer that does not interrupt.
//!
//! The HAL's `Instant` does not advance in light sleep, so the prefix
//! under-counts if an image is ever slept.

use log::{Level, LevelFilter, Log, Metadata, Record};

pub static LOGGER: UptimeLogger = UptimeLogger(LevelFilter::Info);

pub struct UptimeLogger(LevelFilter);

impl Log for UptimeLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.0
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let total_ms = esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_millis();
        let (hours, rem) = (total_ms / 3_600_000, total_ms % 3_600_000);
        let (minutes, rem) = (rem / 60_000, rem % 60_000);
        let (seconds, millis) = (rem / 1_000, rem % 1_000);
        esp_println::print!(
            "[{hours:02}:{minutes:02}:{seconds:02}.{millis:03}] {} - {}\n",
            level_tag(record.level()),
            record.args()
        );
    }

    fn flush(&self) {}
}

fn level_tag(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}
