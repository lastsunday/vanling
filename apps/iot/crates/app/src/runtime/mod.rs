//! The tasks beside the model: the coordinator, the workers it drives, and the I/O each worker
//! owns.
//!
//! A worker reports upward as an intent and is asked downward by a command; nothing here
//! defines a page or paints a pixel.

pub(crate) mod coordinator;
pub mod feature;
pub mod workers;
