// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Rules bound to one model's parameter names. The output is a list of
//! `(parameter index, value)`; feeding it to the runtime is the host's job.

use crate::euler::head_angles;
use crate::frame::TrackingFrame;
use crate::rule::{Rule, Source, resolve_input};

#[derive(Debug, Clone)]
struct Bound {
    index: usize,
    terms: Vec<(Source, f32)>,
    offset: f32,
    min: f32,
    max: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Mapper {
    bound: Vec<Bound>,
}

impl Mapper {
    /// Binds `rules` to `param_names` (the model's parameters, in index
    /// order; exact match). Returns the mapper and the number of rules left
    /// out because the model has no such parameter or the rule has an
    /// unknown input.
    pub fn bind<S: AsRef<str>>(rules: &[Rule], param_names: &[S]) -> (Self, usize) {
        let mut bound = Vec::new();
        for r in rules {
            let index = param_names.iter().position(|n| n.as_ref() == r.output);
            let terms: Option<Vec<_>> = r
                .terms
                .iter()
                .map(|t| resolve_input(&t.input).map(|s| (s, t.weight)))
                .collect();
            if let (Some(index), Some(terms)) = (index, terms) {
                bound.push(Bound {
                    index,
                    terms,
                    offset: r.offset,
                    min: r.min.unwrap_or(f32::NEG_INFINITY),
                    max: r.max.unwrap_or(f32::INFINITY),
                });
            }
        }
        let skipped = rules.len() - bound.len();
        (Self { bound }, skipped)
    }

    /// Number of bound rules, which is the most `apply` writes.
    pub fn len(&self) -> usize {
        self.bound.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bound.is_empty()
    }

    /// Replaces `out` with `(parameter index, value)` pairs. Does not
    /// allocate when `out` has room for [`Mapper::len`] entries. A value that
    /// is not finite (bad input) is left out.
    pub fn apply(&self, frame: &TrackingFrame, out: &mut Vec<(usize, f32)>) {
        out.clear();
        let [yaw, pitch, roll] = head_angles(frame.head_rotation);
        for b in &self.bound {
            let mut v = b.offset;
            for &(s, w) in &b.terms {
                v += w * match s {
                    Source::Blend(i) => frame.blend[i],
                    Source::Yaw => yaw,
                    Source::Pitch => pitch,
                    Source::Roll => roll,
                };
            }
            if v.is_finite() {
                // `max`/`min`, not `clamp`: those never panic.
                out.push((b.index, v.max(b.min).min(b.max)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults::default_rules;
    use crate::frame::arkit_index;
    use crate::rule::Term;

    const MODEL: [&str; 6] = [
        "ParamAngleX",
        "ParamEyeLOpen",
        "ParamEyeROpen",
        "ParamMouthOpenY",
        "Unrelated",
        "ParamMouthForm",
    ];

    fn frame(set: &[(&str, f32)], yaw: f32) -> TrackingFrame {
        let mut f = TrackingFrame::default();
        for &(n, v) in set {
            f.blend[arkit_index(n).unwrap()] = v;
        }
        let (s, c) = (yaw.to_radians() / 2.0).sin_cos();
        f.head_rotation = [0.0, s, 0.0, c];
        f
    }

    fn run(m: &Mapper, f: &TrackingFrame) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        m.apply(f, &mut out);
        out
    }

    #[test]
    fn default_rules_drive_a_standard_model() {
        let (m, skipped) = Mapper::bind(&default_rules(), &MODEL);
        assert_eq!(m.len(), 5);
        assert_eq!(skipped, default_rules().len() - 5);
        let out = run(&m, &frame(&[("jawOpen", 1.0), ("eyeBlinkLeft", 1.0)], 20.0));
        let get = |i| out.iter().find(|p| p.0 == i).unwrap().1;
        assert!((get(0) - 20.0).abs() < 1e-3);
        assert_eq!(get(1), 0.0); // left eye closed
        assert_eq!(get(2), 1.0); // right eye open
        assert_eq!(get(3), 1.0); // mouth open
        assert_eq!(get(5), 0.0);
        assert!(out.iter().all(|p| p.0 != 4));
    }

    #[test]
    fn eye_gaze_and_mouth_form_combine_inputs() {
        let names = [
            "ParamEyeBallX",
            "ParamEyeBallY",
            "ParamMouthForm",
            "ParamBrowLY",
        ];
        let (m, _) = Mapper::bind(&default_rules(), &names);
        let f = frame(
            &[
                ("eyeLookInLeft", 1.0),
                ("eyeLookOutRight", 1.0),
                ("eyeLookDownLeft", 0.4),
                ("mouthSmileLeft", 1.0),
                ("mouthFrownRight", 0.2),
                ("browOuterUpLeft", 0.3),
                ("browDownLeft", 0.1),
            ],
            0.0,
        );
        let out = run(&m, &f);
        // Parameter index -> expected value.
        for (i, w) in [(0, 1.0), (1, -0.2), (2, 0.4), (3, 0.2)] {
            let v = out.iter().find(|p| p.0 == i).unwrap().1;
            assert!((v - w).abs() < 1e-5, "{i}: {v} != {w}");
        }
    }

    #[test]
    fn arkit_named_parameters_take_the_blendshape_directly() {
        let (m, skipped) = Mapper::bind(&default_rules(), &["eyeBlinkRight", "jawOpen"]);
        assert_eq!(m.len(), 2);
        assert_eq!(skipped, default_rules().len() - 2);
        let out = run(
            &m,
            &frame(&[("eyeBlinkRight", 0.7), ("jawOpen", 0.25)], 0.0),
        );
        assert_eq!(out, [(0, 0.7), (1, 0.25)]);
    }

    #[test]
    fn clamp_offset_and_bad_input() {
        let mut r = Rule::new("P", vec![Term::new("jawOpen", 10.0)], 0.5);
        r.min = Some(0.0);
        r.max = Some(1.0);
        let unknown = Rule::new("P", vec![Term::new("nope", 1.0)], 0.0);
        let (m, skipped) = Mapper::bind(&[r, unknown], &["P"]);
        assert_eq!((m.len(), skipped), (1, 1));
        assert_eq!(run(&m, &frame(&[("jawOpen", 1.0)], 0.0)), [(0, 1.0)]);
        let mut bad = TrackingFrame::default();
        bad.blend[arkit_index("jawOpen").unwrap()] = f32::NAN;
        assert!(run(&m, &bad).is_empty());
    }

    #[test]
    fn apply_does_not_allocate_when_the_output_has_room() {
        let (m, _) = Mapper::bind(&default_rules(), &MODEL);
        let mut out = Vec::with_capacity(m.len());
        let cap = out.capacity();
        let ptr = out.as_ptr();
        for i in 0..100 {
            m.apply(&frame(&[("jawOpen", i as f32 / 100.0)], i as f32), &mut out);
            assert_eq!(out.len(), m.len());
            assert_eq!(out.capacity(), cap);
            assert_eq!(out.as_ptr(), ptr);
        }
    }
}
