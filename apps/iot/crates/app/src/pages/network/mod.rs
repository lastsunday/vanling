//! The Network page: the gestures that move the draft, the control geometry, and the drawing
//! that reads it. The draft itself lives in [`crate::shared::model`] because the device owns
//! it.

pub mod controller;
pub mod layout;
mod view;
