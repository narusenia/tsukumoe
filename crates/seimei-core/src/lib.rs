//! GPU-independent rig model, evaluation pipeline and edit commands.
//!
//! This crate must not depend on `wgpu` or `gpui*`, and must build for
//! `wasm32-unknown-unknown` (see `docs/specifications/architecture.md`).

pub mod arena;
pub mod driver;
pub mod elements;
pub mod keyform;
pub mod physics;
pub mod playback;
pub mod rig;
pub mod table;
