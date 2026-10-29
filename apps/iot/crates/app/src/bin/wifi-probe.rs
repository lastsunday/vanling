//! The radio and the IP stack, and nothing else — no panel, no touch, no motion,
//! no audio *task*. Every number printed here measures a path the product will run,
//! without the product's other tasks contending for the same executor, so a fault
//! belongs to the network path rather than to whatever else happened to be awake.
//!
//! What it answers, in the order it answers them:
//!
//! * **Does the link come up, and how long does each stage take**: scan, then
//!   association, then DHCP. Three numbers because they are three different
//!   budgets — the first is the radio's, the second the access point's reply and
//!   the retries, the third the network's lease offer — and a page showing
//!   "connecting" across all three hides which one is slow.
//! * **Whether this build's memory budget is right.** The driver's heap is
//!   reported every few seconds, so a thin margin shows up while the link is up
//!   rather than as an unexplained failure much later.
//! * **Whether the driver's PHY-calibration restore path is reachable on this
//!   chip.** esp-radio implements that hook as `todo!()` under `cfg(xtensa)` and
//!   this part is Xtensa; whether the driver calls it depends on whether a
//!   `phy_init` partition exists, which is this repository's choice. If it is
//!   called the panic is immediate and says so, which is the answer.
//!
//! The board's own bring-up still happens, because it arrives with the board feature and the
//! probe inherits it. How much that is depends on the board: the S3's brings up a panel, a
//! camera and an audio codec, the C6's a button and an LED. It costs DRAM and boot time
//! rather than accuracy — nothing here competes for the executor.
//!
//! Credentials come from the build. A probe whose input were a keystroke protocol
//! would be measuring that protocol as much as the link, and the credentials would
//! have to be typed on a panel this probe does not drive.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use embassy_net::{Config, Stack, StackResources};
use embassy_time::{Duration, Instant, Timer};
use embedded_alloc::LlffHeap as Heap;
use iot_core::drivers::board::HasWifi;
use iot_core::drivers::network::{Credentials, Ipv4, LinkPhase, Station};

const REPORT_MS: u64 = 2_000;

const ADDRESS_BUDGET_MS: u64 = 30_000;

const SCAN_ROUNDS: u8 = 6;
const SCAN_GAP_MS: u64 = 500;

const SOCKETS: usize = 1;

// Sized by the board's own bring-up: the S3's allocates an 11,728-byte `Box` inside
// `Board::new`, so anything under ~16 KiB panics there once the bring-up has already
// succeeded. The driver's allocations do not come from here — they come from the C heap the
// chip layer set up, and where *that* lives differs per part.
//
// The product's number, because the product is the one that has to keep working, and the one
// the chip layer offers a heap for on every part that carries a radio.
const HEAP_BYTES: usize = 64 * 1024;

#[global_allocator]
static HEAP: Heap = Heap::empty();

static RESOURCES: esp_hal::__macro_implementation::static_cell::StaticCell<
    StackResources<SOCKETS>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

#[esp_rtos::main]
#[cfg(feature = "esp32c6")]
async fn main(_spawner: Spawner) -> ! {
    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);

    let (board, timg0, from_cpu_intr) = match iot_bsp_esp::Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[PROBE] board bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };
    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    measure(board).await
}

#[esp_rtos::main]
#[cfg(feature = "esp32s3")]
async fn main(_spawner: Spawner) -> ! {
    // 240 MHz, like the product: a timing measured at a different clock is a timing of a
    // different machine.
    let peripherals = iot_chip_esp::chip_init_at(esp_hal::clock::CpuClock::_240MHz);
    iot_chip_esp::init_logging();
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);

    let (board, timg0, from_cpu_intr, _feed_intr) = match iot_bsp_esp::Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[PROBE] board bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };
    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    measure(board).await
}

async fn measure(mut board: iot_bsp_esp::Board<'static>) -> ! {
    log::warn!("[PROBE] radio and IP stack only: no panel, touch, motion or audio");

    let Some(station) = board.take_wifi() else {
        log::error!("[PROBE] this build wired no radio, so there is nothing to measure");
        park().await
    };
    run(*station).await
}

// What a probe with nothing to do waits on. Never returns: the alternative is a
// `loop` with no await in it, which panics as a blocking loop.
async fn park() -> ! {
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}

async fn run<S>(mut station: S) -> !
where
    S: Station,
    S::Device: embassy_net::driver::Driver,
{
    let booted = Instant::now();
    let device = station
        .take_device()
        .expect("the station hands its interface over on the first ask");

    let rng = esp_hal::rng::Rng::new();
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());

    let (stack, mut runner) = embassy_net::new(
        device,
        Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    log::info!(
        "[WIFI] driver up {} ms after boot",
        booted.elapsed().as_millis()
    );

    let (_, _) = join(runner.run(), exercise(&mut station, &stack, booted)).await;
    unreachable!("both arms of the join return `!`")
}

async fn exercise<S>(station: &mut S, stack: &Stack<'_>, booted: Instant) -> !
where
    S: Station,
{
    for round in 1..=SCAN_ROUNDS {
        if round > 1 {
            Timer::after(Duration::from_millis(SCAN_GAP_MS)).await;
        }
        let scan_start = Instant::now();
        match station.scan().await {
            Ok(found) => {
                log::info!(
                    "[WIFI] scan {round} finished in {} ms with {} access points",
                    scan_start.elapsed().as_millis(),
                    found.len()
                );
                for ap in &found {
                    let name = match ap.ssid.as_display_text() {
                        Some(text) if !text.is_empty() => alloc::string::ToString::to_string(text),
                        _ => bssid_text(ap.bssid),
                    };
                    log::info!(
                        "[WIFI]     {name} ch{} {} dBm bars{} {}",
                        ap.channel,
                        ap.rssi,
                        ap.bars(),
                        if ap.secured { "secured" } else { "open" }
                    );
                }
            }
            Err(error) => log::error!("[WIFI] scan {round} failed: {error:?}"),
        }
    }

    let Some((ssid, password)) = credentials_from_build() else {
        log::warn!(
            "[PROBE] no IOT_WIFI_SSID/IOT_WIFI_PSK in this build: the scan above is the whole \
             answer, and this probe will not guess a network"
        );
        park().await
    };
    let credentials = match Credentials::from_text(ssid, password) {
        Ok(credentials) => credentials,
        Err(error) => {
            log::error!("[PROBE] the credentials this build was given are unusable: {error:?}");
            park().await
        }
    };

    let connect_start = Instant::now();
    let outcome = match station.begin_connect(&credentials) {
        Ok(()) => station.await_connected().await,
        Err(error) => Err(error),
    };
    match outcome {
        Ok(()) => log::info!(
            "[WIFI] associated in {} ms",
            connect_start.elapsed().as_millis()
        ),
        Err(error) => {
            log::error!(
                "[WIFI] association failed after {} ms: {error:?}",
                connect_start.elapsed().as_millis()
            );
            park().await
        }
    }

    let dhcp_start = Instant::now();
    let address = match select(
        wait_for_address(stack),
        Timer::after(Duration::from_millis(ADDRESS_BUDGET_MS)),
    )
    .await
    {
        Either::First(address) => address,
        Either::Second(()) => None,
    };
    station.note_address(address);

    match address {
        Some(address) => {
            let octets = address.octets();
            log::info!(
                "[WIFI] address {}.{}.{}.{} after {} ms (link up {} ms after boot)",
                octets[0],
                octets[1],
                octets[2],
                octets[3],
                dhcp_start.elapsed().as_millis(),
                booted.elapsed().as_millis()
            );
        }
        None => log::error!(
            "[WIFI] no address within {ADDRESS_BUDGET_MS} ms — associated but not connected, \
             which is a network answering nothing rather than a radio that failed"
        ),
    }

    let mut last_report = Instant::now();
    loop {
        Timer::after(Duration::from_millis(REPORT_MS)).await;
        station.note_address(
            stack
                .config_v4()
                .map(|cidr| Ipv4::from_octets(cidr.address.address().octets())),
        );
        let status = station.status();
        let window_ms = last_report.elapsed().as_millis();
        last_report = Instant::now();
        log::info!(
            "[WIFI] phase {:?}, address {}, heap {} B free of {} B, up {} ms, window {window_ms} ms",
            status.phase,
            match address {
                Some(address) => {
                    let octets = address.octets();
                    alloc::format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
                }
                None => alloc::string::String::from("none"),
            },
            iot_chip_esp::radio_heap_free(),
            iot_chip_esp::radio_heap_total(),
            booted.elapsed().as_millis()
        );
        if matches!(status.phase, LinkPhase::Failed(_)) {
            log::error!("[WIFI] the link failed and this probe does not reconnect; stop here");
            park().await
        }
    }
}

fn bssid_text(bssid: [u8; 6]) -> alloc::string::String {
    use core::fmt::Write;
    let mut text = alloc::string::String::with_capacity(17);
    for (index, octet) in bssid.iter().enumerate() {
        if index > 0 {
            text.push(':');
        }
        let _ = write!(text, "{octet:02x}");
    }
    text
}

async fn wait_for_address(stack: &Stack<'_>) -> Option<Ipv4> {
    loop {
        if let Some(cidr) = stack.config_v4() {
            return Some(Ipv4::from_octets(cidr.address.address().octets()));
        }
        stack.wait_config_up().await;
    }
}

fn credentials_from_build() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("IOT_WIFI_SSID")?;
    let password = option_env!("IOT_WIFI_PSK")?;
    log::info!("[PROBE] this build joins the network named {ssid:?}");
    Some((ssid, password))
}
