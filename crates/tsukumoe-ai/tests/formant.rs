// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Formant estimation on synthetic vowels.

mod common;

use common::{NAMES, VOWELS, vowel};
use tsukumoe_ai::formant::FormantFinder;

fn frame(x: &[f32]) -> Vec<f32> {
    let n = x.len();
    let w: Vec<f32> = (0..n)
        .map(|i| {
            let h = 0.54 - 0.46 * (std::f32::consts::TAU * i as f32 / (n - 1) as f32).cos();
            h * x[i]
        })
        .collect();
    (0..n)
        .map(|i| w[i] - if i > 0 { 0.97 * w[i - 1] } else { 0.0 })
        .collect()
}

#[test]
fn formants_are_close_to_the_synthesis_targets() {
    for sr in [11_025.0f32, 16_000.0] {
        for f0 in [120.0, 220.0] {
            let mut finder = FormantFinder::new(sr);
            for (v, t) in VOWELS.iter().enumerate() {
                let x = vowel(sr, f0, *t, 0.3, 0.5);
                let n = (0.032 * sr) as usize;
                let got = finder
                    .find(&frame(&x[x.len() / 2..x.len() / 2 + n]))
                    .unwrap_or_else(|| panic!("sr {sr} f0 {f0} {}: no formants", NAMES[v]));
                assert!(
                    (got[0] - t[0]).abs() < 100.0 && (got[1] - t[1]).abs() < 150.0,
                    "sr {sr} f0 {f0} {}: target {t:?} got {got:?}",
                    NAMES[v]
                );
            }
        }
    }
}

#[test]
fn silence_and_noise_give_no_crash_and_short_frames_none() {
    let mut finder = FormantFinder::new(16_000.0);
    assert_eq!(finder.find(&[0.0; 512]), None);
    assert_eq!(finder.find(&[1.0; 5]), None);
    assert_eq!(finder.find(&[]), None);
    let _ = finder.find(&[f32::MAX; 512]); // overflow must not panic
    let mut s = 1u32;
    let noise: Vec<f32> = (0..512)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / 8_388_608.0 - 1.0
        })
        .collect();
    let _ = finder.find(&frame(&noise));
}
