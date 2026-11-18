//! The main flow: one frame is process_intents, a sweep over the systems, present, and wait.
//!
//! A frame's order is semantic, so it is written here rather than discovered at runtime:
//! intents are applied before anything reads state, systems run in their registered order, and
//! what the surfaces draw is the state this frame settled on.

mod pacing;
mod systems;

use crate::state::DeviceManager;
use embassy_futures::select::select3;
use embassy_time::{Instant, Timer};

use crate::render::Renderers;
use crate::runtime::feature::input::IntentBus;
use crate::runtime::workers::{CaptureWindow, Scan};
use iot_core::boot::Readiness;

use self::pacing::{FramePacer, Wait};
pub(crate) use self::systems::{
    CaptureSystem, DemandSystem, GovernorSystem, NetworkSystem, NoticeSystem, PlaybackSystem,
    Schedule,
};

pub async fn run_loop(
    bus: &'static IntentBus,
    capture: &'static CaptureWindow,
    scan: &'static Scan,
    readiness: &'static Readiness,
    mut renderers: Renderers,
    manager: &mut DeviceManager,
    schedule: &mut Schedule<'_>,
) -> ! {
    let mut pacing = FramePacer::new(Instant::now());
    let mut first_frame_reported = false;
    loop {
        let wall_ms = Instant::now().as_millis() as u32;
        schedule.process_intents(bus, manager);
        schedule.tick(manager, wall_ms);
        let active = renderers.tick(
            manager.state_ref(),
            manager.services().snapshot(),
            manager.demand(),
            pacing.frame_ms(),
        ) || manager.notice_active();
        renderers.flush().await;
        if !first_frame_reported {
            first_frame_reported = true;
            readiness.note_frame();
        }
        match pacing.end_frame(active, Instant::now()) {
            Wait::Until(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select3(bus.ready_to_receive(), capture.ready(), scan.ready()).await;
                pacing.reanchor(Instant::now());
            }
        }
    }
}
