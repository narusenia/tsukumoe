// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Tsukumoe host. `viewer` knows nothing about the UI toolkit: it loads a
//! project, holds the parameters, advances the model and draws it into a
//! texture view. The window and the sliders live in `app`, which only calls it.

pub mod viewer;
