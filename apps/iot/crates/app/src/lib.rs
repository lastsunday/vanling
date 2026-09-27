#![no_std]

extern crate alloc;

use alloc::boxed::Box;

use embassy_futures::join::join;
use iot_core::drivers::board::{Board as BoardTrait, HasAudio, HasInput, HasLight, HasMotion};
use iot_core::state::DeviceManager;

pub mod input;
pub mod render;

use input::{INTENT_BUS, input_task};
use render::{LightRenderer, RENDER_BUS, Render, render_loop};

/// Application composition: takes the board's light and input sources,
/// registers one light renderer per wired surface, and runs the two persistent
/// tasks (input + render). Generic over capabilities, so every board wiring the
/// same capabilities is served by this single copy.
pub async fn run<B>(mut board: B) -> !
where
    B: BoardTrait + HasLight + HasInput + HasMotion + HasAudio + 'static,
{
    let lights = board.take_lights().expect("board has a wired light");
    let mut sources = board.take_input().unwrap_or_default();
    let motion = board.take_motion();
    let motion_enabled = motion.is_some();
    let motion_caps = board.motion_capabilities();
    log::info!(
        "[MOTION] declared capabilities 0x{:X}, source {}",
        motion_caps.bits(),
        if motion_enabled { "wired" } else { "absent" }
    );
    // The capture is just another polled source: it joins the same tick rather
    // than a task of its own, so a slow poll costs envelope columns and nothing
    // else.
    let audio = board.take_audio();
    let audio_enabled = audio.is_some();
    sources.extend(motion);
    sources.extend(audio);

    let mut render = Render::new();
    for (instance, light) in lights.into_iter().enumerate() {
        let _ = render.register(Box::new(LightRenderer::new(instance as u8, light)), 0);
    }
    let never = join(
        input_task(&INTENT_BUS, sources),
        render_loop(
            &INTENT_BUS,
            &RENDER_BUS,
            render,
            DeviceManager::with_motion(motion_enabled, motion_caps).with_audio(audio_enabled),
        ),
    )
    .await;
    match never {}
}
