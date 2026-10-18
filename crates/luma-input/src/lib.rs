//! Luma Blocks input: normalized motion samples, calibration, gesture recognizers, arbitration,
//! remote-button mapping and trace record/replay. Platform-independent and fully testable on
//! a desktop; the webOS adapter only produces [`sample::MotionSample`]s and button events.
//!
//! Pipeline: acquisition (platform) -> [`mrcu`] normalization -> [`frame`] calibrated frames
//! -> [`recognize`] state machines -> [`engine`] arbitration -> game actions.

pub mod buttons;
pub mod calib;
pub mod config;
pub mod engine;
pub mod frame;
pub mod math;
pub mod mrcu;
pub mod recognize;
pub mod sample;
pub mod synth;
pub mod trace;

pub use engine::GestureEngine;
pub use recognize::{Commit, Gesture};
