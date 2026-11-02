// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Rebuilding keyform grids when their binding changes: a key is added or
//! removed, an axis is dropped, an element is bound for the first time.
//!
//! Every operation is one resampling: the new grid takes, at each of its key
//! combinations, the value the old grid interpolates there (the same
//! multilinear rule as evaluation, clamped at the end keys). So adding a key
//! changes nothing that is drawn, and a dropped axis keeps the shape at the
//! value the caller names. Evaluation itself is not touched.

use crate::elements::{ParameterId, Point};
use crate::keyform::{Axis, Binding, BindingError};
use crate::rig::{MeshKeyform, RotationKeyform, WarpKeyform};

/// A keyform that can be mixed from weighted others (the weights sum to 1).
pub trait Blend: Clone {
    fn blend(parts: &[(&Self, f32)]) -> Self;
}

fn mix_points(parts: &[(&[Point], f32)]) -> Vec<Point> {
    let n = parts.first().map_or(0, |(p, _)| p.len());
    let mut out = vec![[0.0f32; 2]; n];
    for (points, weight) in parts {
        for (o, p) in out.iter_mut().zip(*points) {
            o[0] += weight * p[0];
            o[1] += weight * p[1];
        }
    }
    out
}

fn mix_array<const N: usize>(parts: &[([f32; N], f32)]) -> [f32; N] {
    let mut out = [0.0; N];
    for (a, weight) in parts {
        for (o, v) in out.iter_mut().zip(a) {
            *o += weight * v;
        }
    }
    out
}

impl Blend for MeshKeyform {
    fn blend(parts: &[(&Self, f32)]) -> Self {
        let points: Vec<(&[Point], f32)> = parts
            .iter()
            .map(|(k, w)| (k.positions.as_slice(), *w))
            .collect();
        let scalars = |f: &dyn Fn(&Self) -> f32| {
            mix_array(&parts.iter().map(|(k, w)| ([f(k)], *w)).collect::<Vec<_>>())[0]
        };
        Self {
            positions: mix_points(&points),
            opacity: scalars(&|k| k.opacity),
            draw_order: scalars(&|k| k.draw_order),
            multiply_color: mix_array(
                &parts
                    .iter()
                    .map(|(k, w)| (k.multiply_color, *w))
                    .collect::<Vec<_>>(),
            ),
            screen_color: mix_array(
                &parts
                    .iter()
                    .map(|(k, w)| (k.screen_color, *w))
                    .collect::<Vec<_>>(),
            ),
        }
    }
}

impl Blend for WarpKeyform {
    fn blend(parts: &[(&Self, f32)]) -> Self {
        let points: Vec<(&[Point], f32)> = parts
            .iter()
            .map(|(k, w)| (k.points.as_slice(), *w))
            .collect();
        Self {
            points: mix_points(&points),
            opacity: parts.iter().map(|(k, w)| w * k.opacity).sum(),
        }
    }
}

/// Reflection flags are not mixed: the heaviest key wins, as in evaluation.
impl Blend for RotationKeyform {
    fn blend(parts: &[(&Self, f32)]) -> Self {
        let mut out = Self {
            origin: [0.0, 0.0],
            angle_degrees: 0.0,
            scale: 0.0,
            reflect_x: false,
            reflect_y: false,
            opacity: 0.0,
        };
        let mut heaviest = 0.0;
        for (k, w) in parts {
            out.origin[0] += w * k.origin[0];
            out.origin[1] += w * k.origin[1];
            out.angle_degrees += w * k.angle_degrees;
            out.scale += w * k.scale;
            out.opacity += w * k.opacity;
            if *w > heaviest {
                heaviest = *w;
                out.reflect_x = k.reflect_x;
                out.reflect_y = k.reflect_y;
            }
        }
        out
    }
}

/// Key segment containing `value` and the position within it; values
/// outside the keys stick to the end key (as in evaluation).
fn locate(keys: &[f32], value: f32) -> (usize, f32) {
    let last = keys.len() - 1;
    if last == 0 || value.is_nan() || value <= keys[0] {
        return (0, 0.0);
    }
    if value >= keys[last] {
        return (last - 1, 1.0);
    }
    let hi = keys.partition_point(|&k| k <= value);
    let lo = hi - 1;
    (lo, (value - keys[lo]) / (keys[hi] - keys[lo]))
}

/// The keyform `kfs` (on `binding`) gives at `values`, one per axis.
pub fn sample<T: Blend>(binding: &Binding, kfs: &[T], values: &[f32]) -> T {
    let mut corners: Vec<(usize, f32)> = vec![(0, 1.0)];
    for (axis, &value) in binding.axes().iter().zip(values) {
        let n = axis.keys.len();
        let (i, t) = locate(&axis.keys, value);
        let mut next = Vec::with_capacity(corners.len() * 2);
        for &(index, weight) in &corners {
            next.push((index * n + i, weight * (1.0 - t)));
            if t > 0.0 {
                next.push((index * n + i + 1, weight * t));
            }
        }
        corners = next;
    }
    let parts: Vec<(&T, f32)> = corners.iter().map(|&(k, w)| (&kfs[k], w)).collect();
    T::blend(&parts)
}

/// The grid of `new` filled from the grid `kfs` on `old`: at each key
/// combination of `new`, the old grid's value there. An old axis missing
/// from `new` is read at `fallback(parameter)`.
pub fn resample<T: Blend>(
    old: &Binding,
    kfs: &[T],
    new: &Binding,
    fallback: &dyn Fn(ParameterId) -> f32,
) -> Vec<T> {
    let axes = new.axes();
    let count = new.keyform_count();
    let mut out = Vec::with_capacity(count);
    let mut coords = vec![0usize; axes.len()];
    for index in 0..count {
        let mut rest = index;
        for (c, axis) in coords.iter_mut().zip(axes).rev() {
            *c = rest % axis.keys.len();
            rest /= axis.keys.len();
        }
        let values: Vec<f32> = old
            .axes()
            .iter()
            .map(|a| {
                axes.iter()
                    .position(|n| n.parameter == a.parameter)
                    .map_or_else(|| fallback(a.parameter), |j| axes[j].keys[coords[j]])
            })
            .collect();
        out.push(sample(old, kfs, &values));
    }
    out
}

/// A binding on one parameter whose keys are the end points and the
/// default (two keys when the default is an end, one when the range is a point).
pub fn single_axis(
    parameter: ParameterId,
    min: f32,
    default: f32,
    max: f32,
) -> Result<Binding, BindingError> {
    let mut keys = vec![min, default, max];
    keys.sort_by(f32::total_cmp);
    keys.dedup();
    Binding::new(vec![Axis { parameter, keys }])
}

/// `binding` with `value` added as a key on the axis of `parameter`; `None`
/// if there is no such axis or `value` is already a key (or not finite).
pub fn with_key(binding: &Binding, parameter: ParameterId, value: f32) -> Option<Binding> {
    let mut axes = binding.axes().to_vec();
    let axis = axes.iter_mut().find(|a| a.parameter == parameter)?;
    if !value.is_finite() || axis.keys.contains(&value) {
        return None;
    }
    let at = axis.keys.partition_point(|&k| k < value);
    axis.keys.insert(at, value);
    Binding::new(axes).ok()
}

/// `binding` without the key `value` (exact) on the axis of `parameter`;
/// `None` if there is no such key or it is the axis's last.
pub fn without_key(binding: &Binding, parameter: ParameterId, value: f32) -> Option<Binding> {
    let mut axes = binding.axes().to_vec();
    let axis = axes.iter_mut().find(|a| a.parameter == parameter)?;
    let at = axis.keys.iter().position(|&k| k == value)?;
    if axis.keys.len() <= 1 {
        return None;
    }
    axis.keys.remove(at);
    Binding::new(axes).ok()
}

/// `binding` without the axis of `parameter`: `Some(None)` when that was the
/// only axis, `None` if there is no such axis.
pub fn without_axis(binding: &Binding, parameter: ParameterId) -> Option<Option<Binding>> {
    let axes: Vec<Axis> = binding
        .axes()
        .iter()
        .filter(|a| a.parameter != parameter)
        .cloned()
        .collect();
    if axes.len() == binding.axes().len() {
        return None;
    }
    if axes.is_empty() {
        return Some(None);
    }
    Some(Some(Binding::new(axes).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::Arena;
    use crate::elements::Parameter;

    fn params() -> (ParameterId, ParameterId) {
        let mut arena = Arena::new();
        (
            arena.insert(Parameter::new(0.0, 1.0, 0.0)),
            arena.insert(Parameter::new(0.0, 1.0, 0.0)),
        )
    }

    fn warp(x: f32, opacity: f32) -> WarpKeyform {
        WarpKeyform {
            points: vec![[x, 0.0], [x + 1.0, 2.0 * x]],
            opacity,
        }
    }

    fn close(a: &WarpKeyform, b: &WarpKeyform) {
        assert!((a.opacity - b.opacity).abs() < 1e-5);
        for (p, q) in a.points.iter().zip(&b.points) {
            assert!(
                (p[0] - q[0]).abs() < 1e-5 && (p[1] - q[1]).abs() < 1e-5,
                "{p:?} {q:?}"
            );
        }
    }

    #[test]
    fn a_new_key_is_filled_with_the_interpolated_form_and_nothing_else_moves() {
        let (p, _) = params();
        let old = single_axis(p, 0.0, 0.0, 1.0).unwrap();
        assert_eq!(old.axes()[0].keys, [0.0, 1.0]);
        let kfs = vec![warp(0.0, 1.0), warp(4.0, 0.0)];
        let new = with_key(&old, p, 0.25).unwrap();
        let grown = resample(&old, &kfs, &new, &|_| 0.0);
        assert_eq!(grown.len(), 3);
        close(&grown[0], &kfs[0]);
        close(&grown[2], &kfs[1]);
        close(&grown[1], &warp(1.0, 0.75));
        // Sampling either grid anywhere gives the same form.
        for v in [-1.0, 0.0, 0.1, 0.25, 0.6, 1.0, 2.0] {
            close(&sample(&old, &kfs, &[v]), &sample(&new, &grown, &[v]));
        }
        assert!(with_key(&old, p, 0.0).is_none() && with_key(&old, p, f32::NAN).is_none());
    }

    #[test]
    fn removing_a_key_drops_its_form_and_the_rest_interpolate() {
        let (p, _) = params();
        let old = Binding::new(vec![Axis {
            parameter: p,
            keys: vec![0.0, 0.5, 1.0],
        }])
        .unwrap();
        let kfs = vec![warp(0.0, 1.0), warp(1.0, 1.0), warp(4.0, 1.0)];
        let new = without_key(&old, p, 0.5).unwrap();
        let shrunk = resample(&old, &kfs, &new, &|_| 0.0);
        assert_eq!(shrunk.len(), 2);
        close(&shrunk[0], &kfs[0]);
        close(&shrunk[1], &kfs[2]);
        assert!(without_key(&old, p, 0.3).is_none());
        let one = without_key(&new, p, 0.0).unwrap();
        assert!(without_key(&one, p, 1.0).is_none(), "the last key stays");
    }

    #[test]
    fn dropping_an_axis_keeps_the_form_at_the_named_value() {
        let (a, b) = params();
        let old = Binding::new(vec![
            Axis {
                parameter: a,
                keys: vec![0.0, 1.0],
            },
            Axis {
                parameter: b,
                keys: vec![0.0, 1.0],
            },
        ])
        .unwrap();
        // The form at (a, b) is 10 a + b.
        let kfs: Vec<WarpKeyform> = [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]
            .iter()
            .map(|(x, y)| warp(10.0 * x + y, 1.0))
            .collect();
        let without_b = without_axis(&old, b).unwrap().unwrap();
        let kept = resample(&old, &kfs, &without_b, &|_| 0.5);
        close(&kept[0], &warp(0.5, 1.0));
        close(&kept[1], &warp(10.5, 1.0));
        assert!(without_axis(&without_b, b).is_none());
        assert_eq!(without_axis(&without_b, a), Some(None));
    }

    #[test]
    fn rotation_flags_follow_the_heaviest_key() {
        let k = |angle: f32, flip: bool| RotationKeyform {
            origin: [0.0, 0.0],
            angle_degrees: angle,
            scale: 1.0,
            reflect_x: flip,
            reflect_y: false,
            opacity: 1.0,
        };
        let (p, _) = params();
        let b = single_axis(p, 0.0, 0.0, 1.0).unwrap();
        let kfs = vec![k(0.0, false), k(90.0, true)];
        let near_end = sample(&b, &kfs, &[0.8]);
        assert!(near_end.reflect_x && (near_end.angle_degrees - 72.0).abs() < 1e-4);
        assert!(!sample(&b, &kfs, &[0.2]).reflect_x);
    }
}
