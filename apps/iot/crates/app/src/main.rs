#![no_std]
#![no_main]

extern crate alloc;

#[path = "bin/common/mod.rs"]
mod common;

use embassy_executor::Spawner;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let (mut board, feed, io) = common::boot_board().await;
    iot_chip_esp::start_watchdog(spawner);
    let wiring = common::wiring(&mut board, spawner, feed, io);
    iot_app::run(board, wiring).await
}
