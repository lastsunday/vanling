//! The main flow: one frame is process_intents, a sweep over the systems, present, and wait.
//!
//! A frame's order is semantic, so it is written here rather than discovered at runtime:
//! intents are applied before anything reads state, systems run in their registered order, and
//! what the surfaces draw is the state this frame settled on.

mod clock;
mod systems;

use crate::state::DeviceManager;
use embassy_futures::select::select;
use embassy_time::{Instant, Timer};

use crate::render::Renderers;
use crate::runtime::feature::input::IntentBus;
use crate::runtime::system::Frame;
use crate::runtime::workers::WAKE;

use self::clock::{FrameClock, Wait};
pub(crate) use self::systems::{
    CaptureSystem, DemandSystem, GovernorSystem, NetworkSystem, NoticeSystem, PlaybackSystem,
    Schedule,
};

pub async fn run_loop(
    bus: &'static IntentBus,
    mut renderers: Renderers,
    manager: &mut DeviceManager,
    schedule: &mut Schedule<'_>,
) -> ! {
    let mut pacing = FrameClock::new(Instant::now());
    loop {
        let now_ms = Instant::now().as_millis() as u32;
        {
            let mut frame = Frame {
                bus,
                manager,
                now_ms,
            };
            schedule.process_intents(&mut frame);
            schedule.tick(&mut frame);
        }
        let active = renderers.tick(manager.state_ref(), manager.demand(), pacing.elapsed_ms())
            || manager.notice_active();
        renderers.flush().await;
        match pacing.tick(active, Instant::now()) {
            Wait::At(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select(bus.ready_to_receive(), WAKE.wait()).await;
                pacing.reanchor(Instant::now());
            }
        }
    }
}
