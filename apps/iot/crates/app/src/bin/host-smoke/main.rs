//! Host smoke for the app composition: boots [`iot_app::run`] on a fake board under the std
//! backend and asserts the real intent → render pipeline paints a click target on the
//! surface its source drives, with two surfaces and two buttons so multi-instance routing
//! is exercised.

mod board;
mod scenario;

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use iot_app::{Feed, IoPlacement, Wiring, run};
use iot_core::drivers::board::HasSurface;

use self::board::{HostBoard, HostClock};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut board = HostBoard::new();
    let surfaces = board
        .take_surfaces()
        .expect("the host board wires two surfaces");
    match select(
        run(
            board,
            Wiring {
                clock: HostClock,
                feed: Feed::Inline,
                io: IoPlacement::Inline,
                spawner: _spawner,
                wifi: iot_app::Wifi::Unattended,
                surfaces,
                second_core_peak: || None,
            },
        ),
        scenario::scenario(),
    )
    .await
    {
        Either::First(never) => match never {},
        Either::Second(()) => {}
    }
}
