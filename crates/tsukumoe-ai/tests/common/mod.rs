// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A source-filter vowel synthesizer: a glottal pulse train through formant
//! resonators.

use std::f32::consts::TAU;

/// あ い う え お: F1, F2 in Hz.
pub const VOWELS: [[f32; 2]; 5] = [
    [800.0, 1200.0],
    [300.0, 2300.0],
    [350.0, 1300.0],
    [500.0, 1900.0],
    [500.0, 900.0],
];
pub const NAMES: [&str; 5] = ["a", "i", "u", "e", "o"];

/// A Klatt two-pole resonator.
struct Resonator {
    a: f32,
    b: f32,
    c: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn new(f: f32, bw: f32, sr: f32) -> Self {
        let c = -(-TAU * bw / sr).exp();
        let b = 2.0 * (-TAU * bw / (2.0 * sr)).exp() * (TAU * f / sr).cos();
        Self {
            a: 1.0 - b - c,
            b,
            c,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn step(&mut self, x: f32) -> f32 {
        let y = self.a * x + self.b * self.y1 + self.c * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// `seconds` of a vowel with formants `f` (F1, F2) at fundamental `f0`,
/// scaled so the peak is `peak`.
pub fn vowel(sr: f32, f0: f32, f: [f32; 2], seconds: f32, peak: f32) -> Vec<f32> {
    let mut res = [
        Resonator::new(f[0], 80.0, sr),
        Resonator::new(f[1], 100.0, sr),
        Resonator::new(2800.0, 120.0, sr),
        Resonator::new(3500.0, 150.0, sr),
    ];
    let (mut phase, mut tilt) = (0.0f32, 0.0f32);
    let mut out: Vec<f32> = (0..(seconds * sr) as usize)
        .map(|_| {
            phase += f0 / sr;
            let pulse = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            tilt = pulse + 0.9 * tilt;
            res.iter_mut().fold(tilt, |x, r| r.step(x))
        })
        .collect();
    let max = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    out.iter_mut().for_each(|v| *v *= peak / max);
    out
}
