//! The intent pipeline: one task that turns intents into device state, hands that state to the
//! surfaces and to the tasks that cannot see it, and sleeps until the next frame or event.

use alloc::boxed::Box;

use crate::SharedDemand;
use crate::audio::{CaptureWindow, PlaybackBus, PlaybackDispatch};
use crate::bench::replay::PanelBench;
use crate::bridge::{Inbound, Outbound, SetupAsk};
use crate::frame::{FrameClock, Wait};
use crate::input::IntentBus;
use crate::surface::Render;
use embassy_futures::select::select3;
use embassy_time::{Instant, Timer};
use iot_core::demand::{ClockGovernor, ClockSink};
use iot_core::drivers::network::JoinRequest;
use iot_core::intent::Intent;
use iot_core::state::{DeviceManager, LampLine, NetworkCommand};

#[allow(clippy::too_many_arguments)]
pub async fn run_loop<C: ClockSink>(
    intent_bus: &'static IntentBus,
    capture: &'static CaptureWindow,
    outbound: &'static Outbound,
    inbound: &'static Inbound,
    playback_bus: &'static PlaybackBus,
    mut render: Render,
    manager: &mut DeviceManager,
    clock: &mut C,
    demand: &SharedDemand,
) -> ! {
    let mut last_page = manager.state().page;
    let mut last_front = manager.state().link.front;
    let mut last_lamp = manager.state().link_notice();
    let mut frame = FrameClock::new(Instant::now());
    // Loop-local: a second `run` in one process must not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    let mut bench = Box::new(PanelBench::new());
    loop {
        while let Ok(intent) = intent_bus.try_receive() {
            match intent {
                Intent::Operation(op) => {
                    manager.apply_operation(op);
                    let business = iot_core::intent::translate(&op, &manager.state());
                    manager.apply_business(business);
                }
                Intent::Business(business) => manager.apply_business(business),
            }
            if manager.state().page != last_page {
                log::info!("[INPUT] page is now {:?}", manager.state().page);
                last_page = manager.state().page;
            }
            playback.dispatch(playback_bus, manager.playback_state());
        }
        if manager.state().link.front != last_front {
            last_front = manager.state().link.front;
            log::info!(
                "[WIFI] setup mode is offered on the {:?} front",
                manager.state().link.front
            );
        }
        let lamp = manager.state().link_notice();
        if lamp != last_lamp {
            log::info!("{}", LampLine(lamp));
            last_lamp = lamp;
        }
        inbound.page.publish(manager.state().page);
        if let Some(setup) = manager.take_splash_end() {
            log::info!(
                "[INPUT] the splash is over; setup mode {}",
                if setup {
                    "was asked for"
                } else {
                    "was not asked for"
                }
            );
            inbound.page.end_splash(setup);
        }
        bench.step(manager, intent_bus);
        if let Some(sample) = capture.take() {
            manager.apply_capture(sample);
        }
        if let Some(list) = inbound.scan.take() {
            manager.apply_scan(list);
        }
        let asked_on = manager.state().link.front;
        while let Some(command) = manager.take_command() {
            match command {
                NetworkCommand::Join(asked) => {
                    match asked {
                        JoinRequest::Typed(credentials) => log::info!(
                            "[WIFI] the panel named a network: {:?}",
                            credentials
                                .ssid
                                .as_display_text()
                                .unwrap_or("<unprintable>"),
                        ),
                        JoinRequest::Held(ssid) => log::info!(
                            "[WIFI] the panel asked for the copy this device keeps of {:?}",
                            ssid.as_display_text().unwrap_or("<unprintable>"),
                        ),
                    }
                    outbound.joins.publish(asked);
                }
                NetworkCommand::Leave => {
                    log::info!("[WIFI] leaving the link to offer the setup access point");
                    outbound.leave.set(());
                }
                NetworkCommand::LeaveSetup => {
                    log::info!("[WIFI] the panel asked to close setup mode");
                    outbound.setup_ask.set(SetupAsk::Cancel { from: asked_on });
                }
                NetworkCommand::SwitchFront => {
                    log::info!("[WIFI] the panel asked for setup mode's other front end");
                    outbound.setup_ask.set(SetupAsk::Front { from: asked_on });
                }
                NetworkCommand::SetupOpen(open) => {
                    log::info!(
                        "[WIFI] the panel asked for the setup access point to be {}",
                        if open { "open" } else { "under a passphrase" },
                    );
                    outbound.setup_ask.set(SetupAsk::Pass {
                        from: asked_on,
                        open,
                    });
                }
                NetworkCommand::ScanNow => {
                    log::info!("[WIFI] the panel asked to look around now");
                    outbound.rescan.set(());
                }
                NetworkCommand::Forget(ssid) => {
                    log::info!(
                        "[WIFI] the panel asked to forget {:?}",
                        ssid.as_display_text().unwrap_or("<unprintable>"),
                    );
                    outbound.forget.set(ssid);
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
        demand.lock(|current| {
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
            render.tick(&state, manager.demand(), frame.elapsed_ms()) || manager.notice_active();
        match frame.tick(active) {
            Wait::At(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select3(
                    intent_bus.ready_to_receive(),
                    capture.ready(),
                    inbound.scan.ready(),
                )
                .await;
                frame.reanchor(Instant::now());
            }
        }
    }
}
