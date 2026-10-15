//! Nodes are individual signal processing units.
//!
//! There is no change in rate generally between audio, control, etc.
//! All of these run at a **sample accurate** rate.
//!
//! The various modulues here are more for semantics.
//!
//! Custom nodes can be added wtih the [`LegatoBuilder::register_node()`] API.

pub mod audio;
pub mod control;
pub mod midi;
pub mod modular;
