//! Voice DSP shared by `crown-voiced` and its enhancement backends.
//!
//! Everything works on mono `f32` at [`voice_abi::SAMPLE_RATE`] and never allocates once warmed up.

mod chain;
pub mod dfn;
mod frame_accumulator;
mod frame_dispenser;
mod high_pass;
mod level_control;
mod limiter;
mod loudness;
mod suppressor;
mod util;

#[cfg(feature = "host")]
mod backend_suppressor;

pub use chain::Chain;
pub use frame_accumulator::FrameAccumulator;
pub use frame_dispenser::FrameDispenser;
pub use high_pass::{HIGH_PASS_CUTOFF_HZ, HighPass};
pub use level_control::LevelControl;
pub use limiter::Limiter;
pub use loudness::{Loudness, TARGET_LUFS};
pub use suppressor::Suppressor;
