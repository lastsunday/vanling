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
    let mut last_front = manager.state().network.link.mode.host();
    let mut last_lamp = manager.state().link_notice();
    let mut frame = FrameClock::new(Instant::now());
    // Loop-local: a second `run` in one process must not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    let mut bench = Box::new(PanelBench::new());
    // The command the worker had no room for last pass; offered again before anything new.
    let mut unsent: Option<NetworkCommand> = None;
    loop {
        // A command the worker had no room for goes first. If it still does not fit, this
        // pass takes no new intent: the bus is the backlog, so a busy worker costs a frame
        // rather than a request the panel asked for and never sent.
        if let Some(command) = unsent.take()
            && let Err(embassy_sync::channel::TrySendError::Full(command)) =
                workers.net.commands.try_send(command)
        {
            unsent = Some(command);
        }
        while unsent.is_none() {
            let Ok(intent) = bus.try_receive() else {
                break;
            };
            let command = match intent {
                Intent::Input(op) => {
                    manager.apply_input(op);
                    let business = iot_core::intent::translate(&op, manager.state_ref());
                    manager.apply_action(business)
                }
                Intent::Action(business) => manager.apply_action(business),
            };
            if let Some(command) = command
                && let Err(embassy_sync::channel::TrySendError::Full(command)) =
                    workers.net.commands.try_send(command)
            {
                unsent = Some(command);
            }
            if manager.state_ref().page != last_page {
                log::info!("[INPUT] page is now {:?}", manager.state_ref().page);
                last_page = manager.state_ref().page;
            }
            playback.dispatch(workers.playback.to, manager.playback_state());
        }
        let front = manager.state_ref().network.link.mode.host();
        if front != last_front {
            last_front = front;
            if let Some(host) = front {
                log::info!("[WIFI] setup mode is offered on the {host:?} host");
            }
        }
        let lamp = manager.state_ref().link_notice();
        if lamp != last_lamp {
            log::info!("{}", LampLine(lamp));
            last_lamp = lamp;
        }
        workers.net.page.publish(manager.state_ref().page);
        // Only taken when the queue can hold it, so the verdict is neither lost nor able to
        // displace a command that is already waiting for room.
        if unsent.is_none()
            && let Some(setup) = manager.take_splash_end()
        {
            log::info!(
                "[INPUT] the splash is over; setup mode {}",
                if setup {
                    "was asked for"
                } else {
                    "was not asked for"
                }
            );
            if let Err(embassy_sync::channel::TrySendError::Full(command)) = workers
                .net
                .commands
                .try_send(NetworkCommand::SplashOver { setup })
            {
                unsent = Some(command);
            }
        }
        bench.step(manager, bus);
        if let Some(sample) = workers.input.from.take() {
            manager.apply_capture(sample);
        }
        if let Some(list) = workers.net.scan.take() {
            manager.apply_scan(list);
        }
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
                    workers.net.scan.ready(),
                )
                .await;
                frame.reanchor(Instant::now());
            }
        }
    }
}
