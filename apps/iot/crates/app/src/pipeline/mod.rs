//! The intent pipeline: one task that turns intents into device state, hands that state to the
//! surfaces and to the tasks that cannot see it, and sleeps until the next frame or event.

mod network;

use alloc::boxed::Box;

use crate::bench::replay::PanelBench;
use crate::frame::{FrameClock, Wait};
use crate::input::IntentBus;
use crate::playback::PlaybackDispatch;
use crate::surface::Renderers;
use crate::workers::Workers;
use embassy_futures::select::select3;
use embassy_time::{Instant, Timer};
use iot_core::demand::{ClockGovernor, ClockSink};
use iot_core::intent::Intent;
use iot_core::state::DeviceManager;

use self::network::{NetworkMirror, NetworkOutbox};

pub async fn run_loop<C: ClockSink>(
    bus: &'static IntentBus,
    workers: Workers<'_>,
    mut renderers: Renderers,
    manager: &mut DeviceManager,
    clock: &mut C,
) -> ! {
    let mut mirror = NetworkMirror::new(manager.state_ref());
    let mut frame = FrameClock::new(Instant::now());
    // Loop-local: a second `run` in one process must not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    let mut bench = Box::new(PanelBench::new());
    let mut outbox = NetworkOutbox::new();
    loop {
        // A command the worker had no room for goes first. If it still does not fit, this
        // pass takes no new intent: the bus is the backlog.
        outbox.retry(workers.network.commands);
        while !outbox.pending() {
            let Ok(intent) = bus.try_receive() else {
                break;
            };
            let business = match intent {
                Intent::Input(op) => {
                    manager.apply_input(op);
                    iot_core::state::translate(&op, manager.state_ref())
                }
                Intent::Action(business) => business,
            };
            let command = manager.apply_action(business);
            outbox.send(workers.network.commands, command);
            playback.dispatch(workers.playback.to, manager.playback_state());
        }
        mirror.observe(&workers.network, manager.state_ref());
        bench.step(manager, bus);
        if let Some(sample) = workers.input.from.take() {
            manager.apply_capture(sample);
        }
        mirror.drain(&workers.network, manager);
        // Wall clock, not the frame counter: a frame clock is logical, and a held executor makes
        // the loop replay skipped steps. `[实测]` on the C6, three 100 ms flashes collapsed into
        // 19 ms right after an association.
        manager.tick_notice(Instant::now().as_millis() as u32);
        let state = manager.state_ref();
        // The cell holds a `Copy` record, so a change costs one bounded copy rather than a
        // map clone. The feed interrupt takes the same lock, so the section only compares and
        // stores.
        let wanted = *manager.demand();
        workers.input.to.lock(|current| {
            if current.get() != wanted {
                current.set(wanted);
            }
        });
        if let Some(applied) = governor.apply(manager.demand(), clock)
            && let Some(short) = applied.undercuts
        {
            log::warn!(
                "[CLOCK] a page asked for {:?}, but {} is running and needs {:?} \
                 — applying the page's choice",
                applied.asked,
                short.key,
                short.needed,
            );
        }
        let active =
            renderers.tick(state, manager.demand(), frame.elapsed_ms()) || manager.notice_active();
        renderers.flush().await;
        match frame.tick(active) {
            Wait::At(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select3(
                    bus.ready_to_receive(),
                    workers.input.from.ready(),
                    workers.network.scan.ready(),
                )
                .await;
                frame.reanchor(Instant::now());
            }
        }
    }
}
