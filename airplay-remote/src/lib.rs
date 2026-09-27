//! AirPlay Remote: what an iPhone is playing on the Deck's AirPlay speaker,
//! with play/pause, skip and volume buttons that act on the iPhone.
//!
//! The song comes from a small bridge on the Deck that follows the speaker
//! (shairport-sync); the buttons send DACP commands straight to the iPhone.
//! Everything but `live` is plain Rust that builds and tests natively.

#[cfg(target_arch = "wasm32")]
mod live;
mod manifest_params;
pub mod model;
pub mod view;

pub use manifest_params::{LightColor, Params, PlayingLight};
