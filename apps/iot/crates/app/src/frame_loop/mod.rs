//! The main flow: one frame is process_intents, a sweep over the systems, present, and wait.
//!
//! A frame's order is semantic, so it is written here rather than discovered at runtime:
//! intents are applied before anything reads state, systems run in their declared order, and
//! what the surfaces draw is the state this frame settled on.

mod clock;
mod systems;

use crate::state::DeviceManager;
use embassy_futures::select::select3;
use embassy_time::{Instant, Timer};
use iot_core::demand::ClockSink;

use crate::input::IntentBus;
use crate::surface::Renderers;
use crate::workers::Workers;

use self::clock::{FrameClock, Wait};
use self::systems::{Frame, Schedule};

pub async fn run_loop<C: ClockSink>(
    bus: &'static IntentBus,
    workers: Workers<'_>,
    mut renderers: Renderers,
    manager: &mut DeviceManager,
    clock: &mut C,
) -> ! {
    let mut pacing = FrameClock::new(Instant::now());
    let mut schedule = Schedule::new(manager.state_ref(), clock);
    loop {
        let now_ms = Instant::now().as_millis() as u32;
        schedule.process_intents(bus, workers, manager);
        schedule.tick(&mut Frame {
            bus,
            manager,
            workers: &workers,
            now_ms,
        });
        let active = renderers.tick(manager.state_ref(), manager.demand(), pacing.elapsed_ms())
            || manager.notice_active();
        renderers.flush().await;
        match pacing.tick(active) {
            Wait::At(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select3(
                    bus.ready_to_receive(),
                    workers.input.from.ready(),
                    workers.network.scan.ready(),
                )
                .await;
                pacing.reanchor(Instant::now());
            }
        }
    }
}
