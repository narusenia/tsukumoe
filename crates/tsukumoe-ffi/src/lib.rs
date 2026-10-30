// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The C ABI of the playback runtime. The rules (lifetimes, threads, errors,
//! versions) are in `docs/specifications/c-abi.md`.

// Removed once the API uses the wrapper.
#![cfg_attr(not(test), allow(dead_code))]

mod guard;

pub use guard::{TsukumoeStatus, tsukumoe_last_error_message};
