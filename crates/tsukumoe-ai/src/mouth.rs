// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! From a [`MouthFrame`] to the two standard mouth parameters.

use crate::lipsync::{MouthFrame, VOWEL_COUNT};

/// The parameters written, in output order. As with the tracking mapper, the
/// host resolves names to indices.
pub const PARAM_NAMES: [&str; 2] = ["ParamMouthOpenY", "ParamMouthForm"];

/// (opening, form) of あ い う え お.
const SHAPES: [[f32; 2]; VOWEL_COUNT] =
    [[1.0, 0.0], [0.3, 1.0], [0.3, -1.0], [0.6, 0.5], [0.7, -0.6]];

/// The vowel shapes mixed by weight, scaled by `open`.
pub fn mouth_params(frame: &MouthFrame) -> [(&'static str, f32); 2] {
    let mut mix = [0.0f32; 2];
    for (w, shape) in frame.vowels.iter().zip(&SHAPES) {
        mix[0] += w * shape[0];
        mix[1] += w * shape[1];
    }
    [
        (PARAM_NAMES[0], mix[0] * frame.open),
        (PARAM_NAMES[1], mix[1] * frame.open),
    ]
}
