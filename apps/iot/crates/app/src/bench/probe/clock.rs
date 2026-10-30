//! What changing the CPU clock costs, and what it leaves behind.
//!
//! **Outside esp-hal's contract**: its docs say the frequencies "become frozen and cannot
//! be changed" after `esp_hal::init`, so this writes the registers through
//! [`iot_chip_esp::set_cpu_clock`]. It never reads `cpu_clock()`, which reports the
//! configuration tree rather than the hardware, and it never lowers the voltage bias, so
//! **no figure here is a power figure**. Limits and results: `docs/content/records/iot/`.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Instant;
use esp_hal::clock::CpuClock;

const PRESETS: [CpuClock; 3] = [CpuClock::_80MHz, CpuClock::_160MHz, CpuClock::_240MHz];

const RAMP: usize = 128;

pub async fn measure() {
    log::info!("[CLOCK] starting at {:?}", current());

    ratio();
    cost();
    tick();
    drift();

    log::info!("[CLOCK] handing the second core its reading");
    second_core().await;
}

fn current() -> u32 {
    esp_hal::clock::cpu_clock().as_mhz()
}

#[inline(never)]
fn work() -> u32 {
    let mut samples = [0i32; 256];
    for (i, slot) in samples.iter_mut().enumerate() {
        let mut acc = 0i32;
        for tap in 0..64usize {
            acc = acc.wrapping_add(((i + tap) as i32).wrapping_mul(3) ^ (acc >> 3));
        }
        *slot = acc;
    }
    core::hint::black_box(samples.iter().fold(0i32, |a: i32, b| a.wrapping_add(*b)) as u32)
}

fn ratio() {
    let mut baseline = 0;
    for (index, preset) in PRESETS.iter().enumerate() {
        switch(*preset);
        let started = Instant::now();
        work();
        let elapsed = started.elapsed().as_micros();
        if index == 0 {
            baseline = elapsed;
        }
        log::info!(
            "[CLOCK] {preset:?}: esp-hal reads {} MHz, one work unit took {elapsed} us \
             ({:.2} x the first)",
            current(),
            baseline as f64 / elapsed.max(1) as f64,
        );
    }
    switch(CpuClock::_240MHz);
}

fn cost() {
    for preset in [CpuClock::_80MHz, CpuClock::_160MHz, CpuClock::_240MHz] {
        log::info!("[CLOCK] timing a switch to {preset:?}");
        let mut worst = 0u64;
        let mut total = 0u64;
        for _ in 0..16 {
            let started = Instant::now();
            switch(preset);
            let elapsed = started.elapsed().as_micros();
            worst = worst.max(elapsed);
            total += elapsed;
        }
        log::info!(
            "[CLOCK] switch to {preset:?}: mean {} us, worst {worst} us",
            total / 16,
        );
    }
}

fn tick() {
    for preset in [CpuClock::_80MHz, CpuClock::_160MHz, CpuClock::_240MHz] {
        let before = Instant::now();
        switch(preset);
        let after = Instant::now();
        let wall = after.duration_since(before).as_micros();
        let started = Instant::now();
        work();
        let busy = started.elapsed().as_micros();
        log::info!(
            "[CLOCK] across a switch to {preset:?}: timer advanced {wall} us, \
             and a work unit measured right after took {busy} us",
        );
    }
    switch(CpuClock::_240MHz);
}

fn drift() {
    for preset in [CpuClock::_160MHz, CpuClock::_80MHz, CpuClock::_240MHz] {
        let started = Instant::now();
        for _ in 0..RAMP {
            switch(preset);
        }
        let elapsed = started.elapsed().as_micros();
        log::info!(
            "[CLOCK] {RAMP} switches through {preset:?} took {elapsed} us, {} us each",
            elapsed / RAMP as u64,
        );
    }
    switch(CpuClock::_240MHz);
}

// `ets_update_cpu_frequency_rom` updates `g_ticks_per_us` "on the current core only", so
// core 1's ROM timing is scaled for a frequency it no longer runs at. Running the same work
// on both is the only way to know whether that matters.
async fn second_core() {
    let local = local_reading();
    REQ.signal(());
    let remote = DONE.wait().await;
    log::info!(
        "[CLOCK] the same work after core 0 switched away and back: \
         core 0 {local} us, core 1 {remote} us ({:.2} x)",
        local as f64 / remote.max(1) as f64,
    );
}

fn local_reading() -> u64 {
    let started = Instant::now();
    work();
    started.elapsed().as_micros()
}

static REQ: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static DONE: Signal<CriticalSectionRawMutex, u64> = Signal::new();

pub async fn core1_task() {
    REQ.wait().await;
    DONE.signal(local_reading());
}

// No logging: [`cost`] times this call, and a log line's USB-serial write costs more the
// higher the frequency being measured.
fn switch(clock: CpuClock) {
    // SAFETY: interrupts are disabled inside the call across the register
    // writes. The bias it declines to lower is whichever one esp-hal set at
    // bring-up; that is the conservative side at every frequency below 240 and
    // measured to hold at 240 as well.
    unsafe { iot_chip_esp::set_cpu_clock(clock) }
}
