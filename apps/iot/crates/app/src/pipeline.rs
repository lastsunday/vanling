//! The intent pipeline: one task that turns intents into device state, hands that state to the
//! surfaces and to the tasks that cannot see it, and sleeps until the next frame or event.

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
use iot_core::state::{DeviceManager, LampLine, NetworkCommand};

pub async fn run_loop<C: ClockSink>(
    bus: &'static IntentBus,
    workers: Workers<'_>,
    mut renderers: Renderers,
    manager: &mut DeviceManager,
    clock: &mut C,
) -> ! {
    let mut last_page = manager.state().page;
    let mut last_front = manager.state().network.link.host;
    let mut last_lamp = manager.state().link_notice();
    let mut frame = FrameClock::new(Instant::now());
    // Loop-local: a second `run` in one process must not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    let mut bench = Box::new(PanelBench::new());
    // The command the worker had no room for last pass; offered again before anything new.
    let mut unsent: Option<NetworkCommand> = None;
    loop {
        while let Ok(intent) = bus.try_receive() {
            match intent {
                Intent::Input(op) => {
                    manager.apply_input(op);
                    let business = iot_core::intent::translate(&op, &manager.state());
                    manager.apply_action(business);
                }
                Intent::Action(business) => manager.apply_action(business),
            }
            if manager.state().page != last_page {
                log::info!("[INPUT] page is now {:?}", manager.state().page);
                last_page = manager.state().page;
            }
            playback.dispatch(workers.playback.to, manager.playback_state());
        }
        if manager.state().network.link.host != last_front {
            last_front = manager.state().network.link.host;
            log::info!(
                "[WIFI] setup mode is offered on the {:?} host",
                manager.state().network.link.host
            );
        }
        let lamp = manager.state().link_notice();
        if lamp != last_lamp {
            log::info!("{}", LampLine(lamp));
            last_lamp = lamp;
        }
        workers.net.page.publish(manager.state().page);
        if let Some(setup) = manager.take_splash_end() {
            log::info!(
                "[INPUT] the splash is over; setup mode {}",
                if setup {
                    "was asked for"
                } else {
                    "was not asked for"
                }
            );
            workers.net.page.end_splash(setup);
        }
        bench.step(manager, bus);
        if let Some(sample) = workers.input.from.take() {
            manager.apply_capture(sample);
        }
        if let Some(list) = workers.net.scan.take() {
            manager.apply_scan(list);
        }
        // The state machine names the commands; this pass hands over as many as the worker
        // has room for, and keeps the one that found none for the next pass.
        let mut held = unsent.take();
        while let Some(command) = held.take().or_else(|| manager.take_command()) {
            match workers.net.commands.try_send(command) {
                Ok(()) => {}
                Err(embassy_sync::channel::TrySendError::Full(command)) => {
                    unsent = Some(command);
                    break;
                }
            }
        }
        // Wall clock, not the frame counter: a frame clock is logical, and a held executor makes
        // the loop replay skipped steps. `[实测]` on the C6, three 100 ms flashes collapsed into
        // 19 ms right after an association.
        manager.tick_notice(Instant::now().as_millis() as u32);
        let state = manager.state();
        // Compare outside the critical section, and move the copy in rather than cloning
        // inside: the feed interrupt takes the same lock.
        let next = manager.demand().clone();
        workers.input.to.lock(|current| {
            if *current.borrow() != next {
                *current.borrow_mut() = next;
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
            renderers.tick(&state, manager.demand(), frame.elapsed_ms()) || manager.notice_active();
        match frame.tick(active) {
            Wait::At(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select3(
                    bus.ready_to_receive(),
                    workers.input.from.ready(),
                    workers.net.scan.ready(),
                )
                .await;
                frame.reanchor(Instant::now());
            }
        }
    }
}
