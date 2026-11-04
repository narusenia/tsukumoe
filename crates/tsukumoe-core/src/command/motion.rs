// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The commands that make and change motions (M5-08). A curve is edited as a
//! whole: `SetMotionCurve` replaces its keys, and the key operations of
//! `Curve` (insert, remove, move) produce the new keys.

use crate::elements::{Parameter, Part};
use crate::model::Model;
use crate::playback::{Curve, CurveTarget, Motion};

use super::keyform::refused;
use super::{Change, Command, CommandError, MotionTarget};

fn finite_from(v: f32, min: f32) -> bool {
    v.is_finite() && v >= min
}

impl Model {
    pub(super) fn run_motion(
        &mut self,
        command: &Command,
        changes: &mut Vec<Change>,
    ) -> Result<(), CommandError> {
        match command {
            Command::AddMotion {
                name,
                duration,
                fps,
                looped,
            } => {
                if !finite_from(*duration, 0.0) || !finite_from(*fps, f32::MIN_POSITIVE) {
                    return Err(refused(name, "bad-range"));
                }
                let motion = Motion {
                    duration: *duration,
                    fps: *fps,
                    looped: *looped,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    curves: Vec::new(),
                };
                self.insert(changes, name, motion)?;
            }
            Command::RemoveMotion { name } => {
                let id = self.resolve::<Motion>(name)?;
                self.remove(changes, id);
            }
            Command::RenameMotion { name, new_name } => {
                let id = self.resolve::<Motion>(name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::SetMotionTiming {
                name,
                duration,
                fps,
                looped,
                fade_in,
                fade_out,
            } => {
                let id = self.resolve::<Motion>(name)?;
                if !finite_from(*duration, 0.0)
                    || !finite_from(*fps, f32::MIN_POSITIVE)
                    || !finite_from(*fade_in, 0.0)
                    || !finite_from(*fade_out, 0.0)
                {
                    return Err(refused(name, "bad-range"));
                }
                self.modify(changes, id, |m| {
                    m.duration = *duration;
                    m.fps = *fps;
                    m.looped = *looped;
                    m.fade_in = *fade_in;
                    m.fade_out = *fade_out;
                });
            }
            Command::AddMotionCurve { motion, target } => {
                let id = self.resolve::<Motion>(motion)?;
                let (target, value) = match target {
                    MotionTarget::Parameter { name } => {
                        let p = self.resolve::<Parameter>(name)?;
                        (
                            CurveTarget::Parameter(p),
                            self.parameters.get(p).map(|p| p.default),
                        )
                    }
                    MotionTarget::PartOpacity { name } => (
                        CurveTarget::PartOpacity(self.resolve::<Part>(name)?),
                        Some(1.0),
                    ),
                };
                if self
                    .motions
                    .get(id)
                    .is_some_and(|m| m.curves.iter().any(|c| c.target == target))
                {
                    return Err(refused(motion, "already-bound"));
                }
                let curve = Curve {
                    target,
                    start: [0.0, value.unwrap_or(0.0)],
                    segments: Vec::new(),
                    fade_in: None,
                    fade_out: None,
                };
                self.modify(changes, id, |m| m.curves.push(curve));
            }
            Command::RemoveMotionCurve { motion, curve } => {
                let id = self.resolve::<Motion>(motion)?;
                self.curve_of(id, motion, *curve)?;
                self.modify(changes, id, |m| {
                    m.curves.remove(*curve);
                });
            }
            Command::SetMotionCurveFade {
                motion,
                curve,
                fade_in,
                fade_out,
            } => {
                let id = self.resolve::<Motion>(motion)?;
                self.curve_of(id, motion, *curve)?;
                if ![fade_in, fade_out]
                    .iter()
                    .all(|f| f.is_none_or(|v| finite_from(v, 0.0)))
                {
                    return Err(refused(motion, "bad-range"));
                }
                self.modify(changes, id, |m| {
                    m.curves[*curve].fade_in = *fade_in;
                    m.curves[*curve].fade_out = *fade_out;
                });
            }
            Command::SetMotionCurve {
                motion,
                curve,
                start,
                segments,
            } => {
                let id = self.resolve::<Motion>(motion)?;
                let old = self.curve_of(id, motion, *curve)?;
                let new = Curve {
                    start: *start,
                    segments: segments.clone(),
                    ..old.clone()
                };
                if !new.is_ordered() {
                    return Err(refused(motion, "bad-key"));
                }
                self.modify(changes, id, |m| m.curves[*curve] = new);
            }
            _ => unreachable!("run_motion gets motion commands only"),
        }
        Ok(())
    }

    fn curve_of(
        &self,
        id: crate::arena::Id<Motion>,
        name: &str,
        curve: usize,
    ) -> Result<&Curve, CommandError> {
        self.motions
            .get(id)
            .and_then(|m| m.curves.get(curve))
            .ok_or_else(|| refused(name, "no-such-curve"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::Segment;

    fn model() -> Model {
        let mut m = Model::new();
        for c in [
            Command::AddParameter {
                name: "P".into(),
                min: 0.0,
                max: 1.0,
                default: 0.25,
                repeat: false,
            },
            Command::AddPart {
                name: "Body".into(),
                parent: None,
            },
            Command::AddMotion {
                name: "M".into(),
                duration: 2.0,
                fps: 30.0,
                looped: false,
            },
            Command::AddMotionCurve {
                motion: "M".into(),
                target: MotionTarget::Parameter { name: "P".into() },
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    fn set(m: &mut Model, start: [f32; 2], segments: Vec<Segment>) -> Result<(), CommandError> {
        m.apply(Command::SetMotionCurve {
            motion: "M".into(),
            curve: 0,
            start,
            segments,
        })
    }

    fn curve(m: &Model) -> Curve {
        let id = m.motions().find("M").unwrap();
        m.motions().get(id).unwrap().curves[0].clone()
    }

    #[test]
    fn a_new_curve_starts_at_the_default_and_targets_are_unique() {
        let mut m = model();
        assert_eq!(curve(&m).start, [0.0, 0.25]);
        let again = Command::AddMotionCurve {
            motion: "M".into(),
            target: MotionTarget::Parameter { name: "P".into() },
        };
        assert!(m.apply(again).is_err());
        m.apply(Command::AddMotionCurve {
            motion: "M".into(),
            target: MotionTarget::PartOpacity {
                name: "Body".into(),
            },
        })
        .unwrap();
        assert!(m.undo());
        assert!(m.undo());
        assert!(
            m.motions()
                .find("M")
                .is_some_and(|id| m.motions().get(id).unwrap().curves.is_empty())
        );
    }

    #[test]
    fn a_curve_with_keys_going_back_is_refused() {
        let mut m = model();
        let before = curve(&m);
        let back = vec![Segment::Linear([1.0, 1.0]), Segment::Linear([0.5, 0.0])];
        assert!(set(&mut m, [0.0, 0.0], back).is_err());
        let nan = vec![Segment::Linear([1.0, f32::NAN])];
        assert!(set(&mut m, [0.0, 0.0], nan).is_err());
        assert_eq!(curve(&m), before);
    }

    #[test]
    fn a_bad_timing_is_refused() {
        let mut m = model();
        let timing = |duration, fps| Command::SetMotionTiming {
            name: "M".into(),
            duration,
            fps,
            looped: false,
            fade_in: 0.0,
            fade_out: 0.0,
        };
        assert!(m.apply(timing(-1.0, 30.0)).is_err());
        assert!(m.apply(timing(1.0, 0.0)).is_err());
        assert!(m.apply(timing(1.0, 30.0)).is_ok());
    }

    #[test]
    fn a_parameter_a_motion_names_cannot_be_removed() {
        let mut m = model();
        assert!(
            m.apply(Command::RemoveParameter { name: "P".into() })
                .is_err()
        );
    }

    #[test]
    fn inserting_a_key_keeps_every_kind_of_segment_unchanged() {
        let kinds = [
            Segment::Linear([2.0, 4.0]),
            Segment::Bezier([[0.5, 0.0], [1.5, 4.0], [2.0, 4.0]]),
            Segment::Stepped([2.0, 4.0]),
            Segment::InverseStepped([2.0, 4.0]),
        ];
        let m = model();
        let target = m.parameters().find("P").unwrap();
        for kind in kinds {
            let mut c = Curve {
                target: CurveTarget::Parameter(target),
                start: [0.0, 0.0],
                segments: vec![kind],
                fade_in: None,
                fade_out: None,
            };
            let before = c.clone();
            let i = c.insert_key(0.8).unwrap();
            assert_eq!(i, 1);
            assert_eq!(c.key_count(), 3);
            for t in [0.0, 0.3, 0.79, 0.81, 1.5, 1.99, 2.0, 3.0] {
                let (a, b) = (before.value_at(t), c.value_at(t));
                assert!((a - b).abs() < 1e-3, "{kind:?} at {t}: {a} vs {b}");
            }
        }
    }
}
