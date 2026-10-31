// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tracking input for Tsukumoe.
//!
//! I/O-free layers: [`frame`] (common input type), [`osc`] (parser), [`vmc`]
//! (message decoding) and [`post`] (smoothing, sensitivity, dropout recovery).
//! The socket layer is [`udp`].

#![forbid(unsafe_code)]

pub mod euler;
pub mod frame;
pub mod osc;
pub mod post;
pub mod udp;
pub mod vmc;

pub use frame::{ARKIT_COUNT, ARKIT_NAMES, TrackingFrame, arkit_index};
pub use post::{PostConfig, Postprocess};
pub use udp::{TrackingAdapter, VmcReceiver};
