// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe lipsync`: speech audio to a mouth motion. The output is the
//! project's motion file (`motions/<name>.json`, [`MotionFile`]), whose curves
//! name their parameters, so it needs no model to be written.

use tsukumoe_ai::mouth::{PARAM_NAMES, mouth_params};
use tsukumoe_ai::wav::Wav;
use tsukumoe_ai::{Lipsync, LipsyncConfig};
use tsukumoe_format::dto::{CurveDto, CurveTargetDto, MotionFile, SegmentDto};

pub const DEFAULT_FPS: f32 = 30.0;
pub const DEFAULT_TOLERANCE: f32 = 0.01;

/// Values of the two mouth parameters per analysis hop.
fn series(wav: &Wav, config: LipsyncConfig) -> Result<(f32, f32, [Vec<f32>; 2]), String> {
    let mut lipsync = Lipsync::new(wav.sample_rate, config).map_err(|e| e.to_string())?;
    let mut values = [Vec::new(), Vec::new()];
    lipsync.push(&wav.samples, |f| {
        for (v, (_, x)) in values.iter_mut().zip(mouth_params(&f)) {
            v.push(x);
        }
    });
    Ok((lipsync.window_seconds(), lipsync.hop_seconds(), values))
}

/// `values` (one per `hop`, the first at `first`) at `t`, linear between
/// frames and held outside them.
fn at(values: &[f32], first: f32, hop: f32, t: f32) -> f32 {
    let (Some(&head), Some(&tail)) = (values.first(), values.last()) else {
        return 0.0;
    };
    let x = (t - first) / hop;
    if x <= 0.0 {
        return head;
    }
    let i = x as usize;
    if i + 1 >= values.len() {
        return tail;
    }
    values[i] + (values[i + 1] - values[i]) * (x - i as f32)
}

/// Keeps the points of a polyline that a straight line between the kept
/// neighbors would miss by more than `tolerance` (Douglas-Peucker, iterative).
fn simplify(points: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0, n - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (points[a], points[b]);
        let (mut worst, mut at) = (0.0f32, a);
        for (i, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let line = pa[1] + (pb[1] - pa[1]) * (p[0] - pa[0]) / (pb[0] - pa[0]);
            let err = (p[1] - line).abs();
            if err > worst {
                (worst, at) = (err, i);
            }
        }
        if worst > tolerance {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    points
        .iter()
        .zip(&keep)
        .filter_map(|(p, &k)| k.then_some(*p))
        .collect()
}

/// A motion named `name` with `ParamMouthOpenY` and `ParamMouthForm` curves
/// sampled at `fps` and thinned to within `tolerance` (parameter units).
pub fn motion(
    wav: &Wav,
    config: LipsyncConfig,
    fps: f32,
    tolerance: f32,
    name: &str,
) -> Result<MotionFile, String> {
    if !(fps.is_finite() && (1.0..=240.0).contains(&fps)) {
        return Err("fps must be between 1 and 240".into());
    }
    if !(tolerance.is_finite() && tolerance >= 0.0) {
        return Err("tolerance must be a number of at least 0".into());
    }
    let (window, hop, values) = series(wav, config)?;
    let duration = wav.samples.len() as f32 / wav.sample_rate as f32;
    // A frame describes the middle of its window.
    let first = window / 2.0;
    let steps = (duration * fps).floor() as usize;
    let mut times: Vec<f32> = (0..=steps).map(|j| j as f32 / fps).collect();
    if duration - times[steps] > 1e-4 {
        times.push(duration);
    }
    let curves = values
        .iter()
        .zip(PARAM_NAMES)
        .map(|(v, param)| {
            let points: Vec<[f32; 2]> = times.iter().map(|&t| [t, at(v, first, hop, t)]).collect();
            let points = simplify(&points, tolerance);
            CurveDto {
                target: CurveTargetDto::Parameter { name: param.into() },
                start: points[0],
                segments: points[1..]
                    .iter()
                    .map(|&point| SegmentDto::Linear { point })
                    .collect(),
                fade_in: None,
                fade_out: None,
            }
        })
        .collect();
    Ok(MotionFile {
        name: name.into(),
        duration,
        fps,
        looped: false,
        fade_in: 0.0,
        fade_out: 0.0,
        curves,
    })
}
