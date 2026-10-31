// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Formant estimation: LPC by autocorrelation and Levinson-Durbin, then the
//! roots of the LPC polynomial (Durand-Kerner). Poles are used rather than
//! envelope peaks so that two close formants (あ, お) stay separate. All work
//! buffers are allocated once in [`FormantFinder::new`].

use std::f64::consts::{PI, TAU};

/// Poles below this frequency are the glottal and DC residue, not formants.
const MIN_FREQ: f64 = 150.0;
/// Poles with a wider bandwidth than this are spectral tilt, not formants.
const MAX_BANDWIDTH: f64 = 400.0;
const ROOT_ITERATIONS: usize = 80;

pub struct FormantFinder {
    order: usize,
    sample_rate: f64,
    r: Vec<f64>,
    a: Vec<f64>,
    prev: Vec<f64>,
    roots: Vec<[f64; 2]>,
}

fn mul(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}

fn div(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = b[0] * b[0] + b[1] * b[1];
    [
        (a[0] * b[0] + a[1] * b[1]) / d,
        (a[1] * b[0] - a[0] * b[1]) / d,
    ]
}

impl FormantFinder {
    /// The LPC order is `2 + sample_rate / 1000`; meant for rates up to about
    /// 24 kHz (the analyzer decimates above that).
    pub fn new(sample_rate: f32) -> Self {
        let order = 2 + (sample_rate / 1000.0) as usize;
        Self {
            order,
            sample_rate: f64::from(sample_rate),
            r: vec![0.0; order + 1],
            a: vec![0.0; order + 1],
            prev: vec![0.0; order + 1],
            roots: vec![[0.0; 2]; order],
        }
    }

    pub fn order(&self) -> usize {
        self.order
    }

    /// `[F1, F2]` in Hz from a windowed, pre-emphasized frame, or `None` when
    /// the frame is silent or has fewer than two formant-like poles.
    pub fn find(&mut self, frame: &[f32]) -> Option<[f32; 2]> {
        let p = self.order;
        if frame.len() <= p {
            return None;
        }
        for lag in 0..=p {
            self.r[lag] = frame[lag..]
                .iter()
                .zip(frame)
                .map(|(&x, &y)| f64::from(x) * f64::from(y))
                .sum();
        }
        if self.r[0].is_nan() || self.r[0] <= 1e-12 {
            return None;
        }
        self.r[0] *= 1.0 + 1e-5; // white-noise correction keeps the recursion stable
        self.levinson()?;
        self.solve();
        self.formants()
    }

    /// Fills `a[1..=p]` so that `1 + sum a[k] z^-k` whitens the frame.
    fn levinson(&mut self) -> Option<()> {
        self.a.fill(0.0);
        self.a[0] = 1.0;
        let mut err = self.r[0];
        for i in 1..=self.order {
            let acc: f64 = (0..i).map(|j| self.a[j] * self.r[i - j]).sum();
            let k = -acc / err;
            self.prev.copy_from_slice(&self.a);
            for j in 1..i {
                self.a[j] = self.prev[j] + k * self.prev[i - j];
            }
            self.a[i] = k;
            err *= 1.0 - k * k;
            if err.is_nan() || err <= 0.0 || !k.is_finite() {
                return None;
            }
        }
        Some(())
    }

    /// Roots of `z^p + a1 z^(p-1) + ... + ap` by Durand-Kerner, from a fixed
    /// start, so the result is deterministic.
    fn solve(&mut self) {
        let p = self.order;
        for (i, z) in self.roots.iter_mut().enumerate() {
            let w = TAU * (i as f64 + 0.25) / p as f64;
            *z = [0.9 * w.cos(), 0.9 * w.sin()];
        }
        for _ in 0..ROOT_ITERATIONS {
            let mut moved = 0.0f64;
            for i in 0..p {
                let z = self.roots[i];
                let mut f = [1.0, 0.0];
                for k in 1..=p {
                    f = mul(f, z);
                    f[0] += self.a[k];
                }
                let mut d = [1.0, 0.0];
                for (j, &o) in self.roots.iter().enumerate() {
                    if j != i {
                        d = mul(d, [z[0] - o[0], z[1] - o[1]]);
                    }
                }
                if d[0] == 0.0 && d[1] == 0.0 {
                    continue;
                }
                let step = div(f, d);
                self.roots[i] = [z[0] - step[0], z[1] - step[1]];
                moved = moved.max(step[0].abs() + step[1].abs());
            }
            if moved < 1e-9 {
                break;
            }
        }
    }

    fn formants(&self) -> Option<[f32; 2]> {
        let mut found = [f64::MAX; 2];
        for z in &self.roots {
            let radius = z[0].hypot(z[1]);
            if z[1] <= 0.0 || radius <= 0.0 || !radius.is_finite() {
                continue;
            }
            let freq = z[1].atan2(z[0]) * self.sample_rate / TAU;
            let bandwidth = -radius.ln() * self.sample_rate / PI;
            if freq < MIN_FREQ || bandwidth > MAX_BANDWIDTH {
                continue;
            }
            // Keep the two lowest.
            if freq < found[0] {
                found = [freq, found[0]];
            } else if freq < found[1] {
                found[1] = freq;
            }
        }
        (found[1] != f64::MAX).then(|| [found[0] as f32, found[1] as f32])
    }
}
