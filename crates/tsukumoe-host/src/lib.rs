// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Tsukumoe host. `viewer` knows nothing about the UI toolkit: it loads a
//! project, holds the parameters, advances the model and draws it into a
//! texture view. `live` wires tracking input to the parameters, also without
//! the toolkit. The window and the sliders live in `app`, which only calls it.

pub mod app;
pub mod live;
pub mod options;
pub mod settings;
pub mod viewer;
