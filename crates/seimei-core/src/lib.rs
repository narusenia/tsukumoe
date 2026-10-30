//! GPU-independent rig model, evaluation pipeline and edit commands.
//!
//! This crate must not depend on `wgpu` or `gpui*`, and must build for
//! `wasm32-unknown-unknown` (see `docs/specifications/architecture.md`).
