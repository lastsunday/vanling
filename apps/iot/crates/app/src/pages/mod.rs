//! The product's pages, one vertical slice each: the page's model, controller, layout and
//! view live together, and the directory is the unit of cohesion.
//!
//! [`Page`] is what a page answers when the device asks — its ink, what a gesture means on it,
//! and what the renderer must do for it. Every answer is a match here rather than a table in
//! the module that asks, so adding a page names it in this one file.

pub mod ambient;
pub mod attitude;
pub mod audio;
pub mod camera;
pub mod env;
pub mod network;
pub mod speaker;
pub mod splash;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent, LightAction, NavigationIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::model::DeviceState;
use crate::shared::route::PageId;
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::paint::Rgb;

/// What a page answers when the device asks: its ink, what a gesture means on it, and what the
/// renderer must do for it. A page that leaves an answer out does not compile.
pub trait Page {
    /// Draws the page into `surface`, and the debug badge every page but the camera carries.
    fn stamp(self, surface: &mut Surface<'_>);

    /// What one gesture means on this page.
    fn bind(self, op: &InputIntent, state: &DeviceState) -> BusinessIntent;

    /// The fill the renderer covers the panel with first, or `None` when the page draws over
    /// what the light was already showing.
    fn ground(self) -> Option<Rgb>;

    /// Whether this page's frames arrive from the camera instead of being drawn.
    fn draws_frames(self) -> bool;

    /// Whether this pair of diagnostics snapshots earns a repaint. Called once the renderer's
    /// repaint step has elapsed.
    fn repaints_on(self, prev: &Diagnostics, next: &Diagnostics, overlay: bool) -> bool;
}

impl Page for PageId {
    fn stamp(self, surface: &mut Surface<'_>) {
        match self {
            Self::Splash => surface.stamp_splash(),
            Self::Attitude => surface.stamp_attitude(),
            Self::Audio => surface.stamp_audio(),
            Self::Speaker => surface.stamp_speaker(),
            Self::Camera => surface.stamp_camera(CameraSnapshot::STARTING),
            Self::Network => surface.stamp_network(),
            Self::Env => surface.stamp_env(),
            Self::Ambient if surface.debug => surface.stamp_ambient(),
            Self::Ambient => {}
        }
        if surface.debug && self != PageId::Camera {
            surface.stamp_fps();
        }
    }

    fn bind(self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        match self {
            Self::Audio => audio::controller::bind(op, state),
            Self::Speaker => speaker::controller::bind(op, state),
            Self::Network => network::controller::bind(op, state),
            Self::Env => env::controller::bind(op, state),
            Self::Splash => match op.event {
                InputEvent::Button(ButtonEvent::LongPress) => {
                    BusinessIntent::Navigation(NavigationIntent::RequestSetup)
                }
                _ => BusinessIntent::Invalid,
            },
            Self::Ambient | Self::Attitude | Self::Camera => lamp(op, state),
        }
    }

    fn ground(self) -> Option<Rgb> {
        match self {
            Self::Camera | Self::Network => Some(Rgb(0, 0, 0)),
            Self::Ambient
            | Self::Attitude
            | Self::Audio
            | Self::Speaker
            | Self::Splash
            | Self::Env => None,
        }
    }

    fn draws_frames(self) -> bool {
        matches!(self, Self::Camera)
    }

    fn repaints_on(self, prev: &Diagnostics, next: &Diagnostics, overlay: bool) -> bool {
        match self {
            // A frame arriving is a data event and not anything a snapshot diff can see.
            // Forcing a repaint here would ship a full frame that is a copy of the last one —
            // ~15 ms of blocking SPI.
            Self::Camera | Self::Splash => false,
            Self::Ambient | Self::Env => overlay,
            Self::Attitude => true,
            Self::Audio => {
                prev.audio.phase != next.audio.phase
                    || prev.audio.envelope != next.audio.envelope
                    || prev.audio.restarts != next.audio.restarts
                    || prev.audio.dba_lsb != next.audio.dba_lsb
            }
            Self::Speaker => prev.playback != next.playback,
            Self::Network => {
                prev.link != next.link
                    || prev.aps != next.aps
                    || prev.draft != next.draft
                    || prev.mac != next.mac
            }
        }
    }
}

/// The lamp's table, for the pages whose only verb is the light.
fn lamp(op: &InputIntent, state: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            state.light_intent(op, LightAction::NextColor { forward: true })
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => state.light_intent(op, LightAction::Step),
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            state.light_intent(op, LightAction::Cycle)
        }
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            state.light_swipe(op, direction)
        }
        _ => BusinessIntent::Invalid,
    }
}

#[cfg(test)]
mod tests;
