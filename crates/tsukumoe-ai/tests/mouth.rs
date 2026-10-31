// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The mouth parameter conversion.

use tsukumoe_ai::MouthFrame;
use tsukumoe_ai::mouth::{PARAM_NAMES, mouth_params};

fn frame(open: f32, vowels: [f32; 5]) -> MouthFrame {
    MouthFrame {
        open,
        vowels,
        voiced: true,
    }
}

#[test]
fn each_vowel_has_its_shape() {
    let expected = [(1.0, 0.0), (0.3, 1.0), (0.3, -1.0), (0.6, 0.5), (0.7, -0.6)];
    for (i, (y, form)) in expected.into_iter().enumerate() {
        let mut w = [0.0; 5];
        w[i] = 1.0;
        let p = mouth_params(&frame(1.0, w));
        assert_eq!((p[0].0, p[1].0), (PARAM_NAMES[0], PARAM_NAMES[1]));
        assert_eq!((p[0].0, p[1].0), ("ParamMouthOpenY", "ParamMouthForm"));
        assert!(
            (p[0].1 - y).abs() < 1e-6 && (p[1].1 - form).abs() < 1e-6,
            "vowel {i}: {p:?}"
        );
    }
}

#[test]
fn closed_mouth_and_no_vowel_give_zero_and_open_scales() {
    let a = [1.0, 0.0, 0.0, 0.0, 0.0];
    for p in mouth_params(&frame(0.0, a)) {
        assert_eq!(p.1, 0.0);
    }
    for p in mouth_params(&frame(1.0, [0.0; 5])) {
        assert_eq!(p.1, 0.0);
    }
    assert_eq!(mouth_params(&frame(0.5, a))[0].1, 0.5);
    // Weights mix linearly: half い, half う -> form cancels.
    let p = mouth_params(&frame(1.0, [0.0, 0.5, 0.5, 0.0, 0.0]));
    assert!((p[0].1 - 0.3).abs() < 1e-6 && p[1].1.abs() < 1e-6);
}
