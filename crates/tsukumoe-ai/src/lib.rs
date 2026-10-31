// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Inference provider abstraction for Tsukumoe.
//!
//! [`provider`] holds the [`Provider`] trait and tensors. No inference
//! backend, HTTP client or weights live here: weights are fetched through a
//! caller-supplied fetcher and stored verified (added in later modules).

#![forbid(unsafe_code)]

pub mod error;
pub mod formant;
pub mod hash;
pub mod lipsync;
pub mod provider;
pub mod registry;
pub mod spec;
pub mod store;
pub mod wav;

pub use error::AiError;
pub use lipsync::{Lipsync, LipsyncConfig, MouthFrame};
pub use provider::{Disclosure, Locality, Provider, Tensor};
pub use registry::ProviderRegistry;
pub use spec::{License, LicenseNotice, ModelSpec, parse_manifest};
pub use store::{Fetcher, ModelStore, Status};
