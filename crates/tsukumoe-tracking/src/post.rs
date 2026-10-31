// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Adapter-independent output stage: per-item sensitivity, time-based
//! exponential smoothing, and dropout detection with recovery to the neutral
//! pose. Time is always passed in, so this layer is deterministic and I/O-free.

use crate::frame::{ARKIT_COUNT, TrackingFrame};

#[derive(Debug, Clone, PartialEq)]
pub struct PostConfig {
    /// Smoothing time constant in seconds per blendshape (`<= 0`: no smoothing).
    pub blend_tau: [f32; ARKIT_COUNT],
    /// Head rotation / position time constant in seconds.
    pub head_tau: f32,
    /// Sensitivity: `clamp(value * gain + offset, 0, 1)` per blendshape.
    pub gain: [f32; ARKIT_COUNT],
    pub offset: [f32; ARKIT_COUNT],
    /// Seconds without input before the stream counts as dropped.
    pub timeout: f32,
    /// Seconds to fade from the last value to the neutral pose.
    pub recover: f32,
}

impl Default for PostConfig {
    fn default() -> Self {
        Self {
            blend_tau: [0.05; ARKIT_COUNT],
            head_tau: 0.05,
            gain: [1.0; ARKIT_COUNT],
            offset: [0.0; ARKIT_COUNT],
            timeout: 0.5,
            recover: 0.3,
        }
    }
}

#[derive(Debug)]
pub struct Postprocess {
    pub config: PostConfig,
    target: TrackingFrame,
    current: TrackingFrame,
    last_rx: Option<f64>,
    last_sample: Option<f64>,
    /// Value when the current dropout began; `Some` only while dropped.
    fade_from: Option<TrackingFrame>,
}

/// Blend factor for a time constant: exact for any step size, so results do
/// not depend on the sampling rate.
fn alpha(dt: f64, tau: f32) -> f32 {
    if tau > 0.0 {
        (1.0 - (-dt / f64::from(tau)).exp()) as f32
    } else {
        1.0
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn nlerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    let mut q = [0.0; 4];
    for i in 0..4 {
        q[i] = lerp(a[i], sign * b[i], t);
    }
    normalize(q)
}

fn normalize(q: [f32; 4]) -> [f32; 4] {
    let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if n.is_finite() && n > 1e-6 {
        q.map(|v| v / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

impl Postprocess {
    pub fn new(config: PostConfig) -> Self {
        Self {
            config,
            target: TrackingFrame::default(),
            current: TrackingFrame::default(),
            last_rx: None,
            last_sample: None,
            fade_from: None,
        }
    }

    /// Feeds a new input frame received at `now` (apply sensitivity, set target).
    pub fn push(&mut self, frame: &TrackingFrame, now: f64) {
        let mut t = *frame;
        for (i, v) in t.blend.iter_mut().enumerate() {
            let x = frame.blend[i] * self.config.gain[i] + self.config.offset[i];
            *v = if x.is_finite() {
                x.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        t.head_rotation = normalize(frame.head_rotation);
        if self.last_rx.is_none() {
            self.current = t; // first input: no ramp from neutral
        }
        self.target = t;
        self.last_rx = Some(now);
    }

    /// Output at `now`. Call as often as needed; `now` must not go backwards.
    pub fn sample(&mut self, now: f64) -> TrackingFrame {
        if !now.is_finite() {
            return self.current;
        }
        let dt = self.last_sample.map_or(0.0, |p| (now - p).max(0.0));
        self.last_sample = Some(now);
        if let Some(rx) = self.last_rx {
            let elapsed = now - rx;
            if elapsed <= f64::from(self.config.timeout) {
                self.fade_from = None;
                self.smooth(dt);
            } else {
                self.fade(elapsed - f64::from(self.config.timeout));
            }
        }
        self.current.time = now;
        self.current
    }

    /// True while the input is considered dropped (fading or neutral).
    pub fn is_dropped(&self) -> bool {
        self.fade_from.is_some()
    }

    fn smooth(&mut self, dt: f64) {
        let (c, t) = (&mut self.current, &self.target);
        for i in 0..ARKIT_COUNT {
            c.blend[i] = lerp(c.blend[i], t.blend[i], alpha(dt, self.config.blend_tau[i]));
        }
        let a = alpha(dt, self.config.head_tau);
        c.head_rotation = nlerp(c.head_rotation, t.head_rotation, a);
        c.head_position = match (c.head_position, t.head_position) {
            (Some(p), Some(q)) => Some([0, 1, 2].map(|i| lerp(p[i], q[i], a))),
            (_, q) => q,
        };
    }

    /// Linear fade from the value at dropout start toward neutral.
    fn fade(&mut self, since_timeout: f64) {
        let from = *self.fade_from.get_or_insert(self.current);
        let t = if self.config.recover > 0.0 {
            (since_timeout / f64::from(self.config.recover)).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        let c = &mut self.current;
        for i in 0..ARKIT_COUNT {
            c.blend[i] = lerp(from.blend[i], 0.0, t);
        }
        c.head_rotation = nlerp(from.head_rotation, [0.0, 0.0, 0.0, 1.0], t);
        c.head_position = match from.head_position {
            Some(_) if t >= 1.0 => None,
            Some(p) => Some(p.map(|v| lerp(v, 0.0, t))),
            None => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step_frame(v: f32) -> TrackingFrame {
        TrackingFrame {
            blend: [v; ARKIT_COUNT],
            ..Default::default()
        }
    }

    /// Runs a step from 0 to 1 sampled every `dt` while input keeps arriving.
    fn run_step(dt: f64, until: f64) -> TrackingFrame {
        let mut p = Postprocess::new(PostConfig::default());
        p.push(&step_frame(0.0), 0.0);
        p.sample(0.0);
        let steps = (until / dt).round() as usize;
        let mut out = TrackingFrame::default();
        for k in 1..=steps {
            let now = k as f64 * dt;
            p.push(&step_frame(1.0), now);
            out = p.sample(now);
        }
        out
    }

    #[test]
    fn step_response_follows_time_constant() {
        let out = run_step(0.01, 0.1);
        let expect = 1.0 - (-0.1f32 / 0.05).exp();
        assert!((out.blend[0] - expect).abs() < 1e-4, "{}", out.blend[0]);
    }

    #[test]
    fn smoothing_is_independent_of_dt() {
        let a = run_step(0.005, 0.2).blend[0];
        let b = run_step(0.02, 0.2).blend[0];
        let c = run_step(0.1, 0.2).blend[0];
        assert!((a - b).abs() < 1e-5 && (a - c).abs() < 1e-5, "{a} {b} {c}");
    }

    #[test]
    fn zero_tau_passes_through() {
        let cfg = PostConfig {
            blend_tau: [0.0; ARKIT_COUNT],
            ..Default::default()
        };
        let mut p = Postprocess::new(cfg);
        p.push(&step_frame(0.0), 0.0);
        p.sample(0.0);
        p.push(&step_frame(0.7), 0.01);
        assert!((p.sample(0.01).blend[3] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn sensitivity_scales_offsets_and_clamps() {
        let mut cfg = PostConfig {
            blend_tau: [0.0; ARKIT_COUNT],
            ..Default::default()
        };
        cfg.gain[0] = 2.0;
        cfg.gain[1] = 1.0;
        cfg.offset[1] = 0.25;
        cfg.gain[2] = 4.0;
        cfg.offset[3] = -0.5;
        let mut p = Postprocess::new(cfg);
        let mut f = TrackingFrame::default();
        f.blend[..4].copy_from_slice(&[0.25, 0.5, 0.5, 0.25]);
        p.push(&f, 0.0);
        let o = p.sample(0.0);
        assert_eq!(&o.blend[..4], &[0.5, 0.75, 1.0, 0.0]);
    }

    fn live(rot: [f32; 4]) -> TrackingFrame {
        TrackingFrame {
            blend: [1.0; ARKIT_COUNT],
            head_rotation: rot,
            head_position: Some([1.0, 0.0, 0.0]),
            time: 0.0,
        }
    }

    fn settled() -> Postprocess {
        let mut p = Postprocess::new(PostConfig::default());
        p.push(&live([0.0, 1.0, 0.0, 0.0]), 0.0);
        p.sample(0.0);
        p
    }

    #[test]
    fn dropout_holds_then_fades_then_neutral() {
        let mut p = settled();
        // Within the timeout: held.
        assert_eq!(p.sample(0.5).blend[0], 1.0);
        assert!(!p.is_dropped());
        // Halfway through the fade (0.5 + 0.15 s).
        let mid = p.sample(0.65);
        assert!(p.is_dropped());
        assert!((mid.blend[0] - 0.5).abs() < 1e-5);
        assert!((mid.head_position.unwrap()[0] - 0.5).abs() < 1e-5);
        // Done.
        let end = p.sample(0.9);
        assert_eq!(end.blend, [0.0; ARKIT_COUNT]);
        assert_eq!(end.head_rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(end.head_position, None);
        // Stays neutral.
        assert_eq!(p.sample(5.0).blend[7], 0.0);
    }

    #[test]
    fn fade_is_independent_of_sample_rate() {
        let mut fine = settled();
        let mut coarse = settled();
        let mut a = TrackingFrame::default();
        for k in 1..=70 {
            a = fine.sample(k as f64 * 0.01);
        }
        let b = coarse.sample(0.7);
        assert!((a.blend[0] - b.blend[0]).abs() < 1e-5);
    }

    #[test]
    fn resume_after_full_recovery_and_mid_recovery() {
        // After neutral: input returns and smoothing climbs from 0.
        let mut p = settled();
        p.sample(1.0);
        p.push(&live([0.0, 1.0, 0.0, 0.0]), 1.0);
        let o = p.sample(1.05);
        assert!(!p.is_dropped());
        assert!(o.blend[0] > 0.5 && o.blend[0] < 1.0, "{}", o.blend[0]);

        // Mid-recovery: resume continues from the current value, no jump.
        let mut p = settled();
        let mid = p.sample(0.65).blend[0]; // 0.5
        p.push(&live([0.0, 1.0, 0.0, 0.0]), 0.65);
        let after = p.sample(0.66).blend[0];
        assert!(!p.is_dropped());
        assert!(after > mid && after < 0.7, "{mid} -> {after}");
        let later = p.sample(1.0).blend[0];
        assert!(later > 0.99);
    }

    #[test]
    fn untrusted_values_do_not_poison_state() {
        let mut p = Postprocess::new(PostConfig::default());
        let mut f = TrackingFrame::default();
        f.blend[0] = f32::NAN;
        f.head_rotation = [0.0; 4];
        p.push(&f, 0.0);
        let o = p.sample(f64::NAN);
        assert!(o.blend[0].is_finite());
        let o = p.sample(0.1);
        assert!(o.blend.iter().all(|v| v.is_finite()));
        assert_eq!(o.head_rotation, [0.0, 0.0, 0.0, 1.0]);
    }
}
