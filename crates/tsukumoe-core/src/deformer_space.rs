// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The space a deformer gives its children, at the base shape of the model
//! (no parameter, no keyforms): a point in a child space to the drawing space
//! and back. An editor uses it to move an element under another deformer, or
//! out from under it, without changing how it looks.
//!
//! A warp deformer's children live in `(u, v)`, the grid normalised to
//! `0..=1`; a rotation deformer's children live in coordinates measured from
//! its origin along its scaled axes. The forward maps are the ones of
//! `evaluation.md`: bilinear in the cell, the nearest cell extended outside
//! the grid; a rotation under a warp takes its axes from the warp at its
//! origin. The inverse of a cell is solved by Newton's method.

use std::fmt;

use crate::command::RotationBase;
use crate::elements::Point;
use crate::model::Model;
use crate::rig::DeformerRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceError {
    /// A cell of the grid, or the axes of a rotation, are flat: no inverse.
    Degenerate,
    /// The parent chain is too long (a cycle).
    TooDeep,
}

impl fmt::Display for SpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Degenerate => "the deformer is flat here: its shape cannot be inverted",
            Self::TooDeep => "the deformer parents loop",
        })
    }
}

impl std::error::Error for SpaceError {}

type P = [f64; 2];

fn p64(p: Point) -> P {
    [f64::from(p[0]), f64::from(p[1])]
}

fn point(p: P) -> Point {
    [p[0] as f32, p[1] as f32]
}

/// Bilinear lookup in a grid of `(rows + 1) * (columns + 1)` points (row by
/// row), the way the evaluation does it.
pub fn warp_forward(grid: &[P], rows: usize, columns: usize, uv: P) -> P {
    let fu = uv[0] * columns as f64;
    let fv = uv[1] * rows as f64;
    let i = (fu.floor().max(0.0) as usize).min(columns - 1);
    let j = (fv.floor().max(0.0) as usize).min(rows - 1);
    let (s, t) = (fu - i as f64, fv - j as f64);
    let at = |r: usize, c: usize| grid[r * (columns + 1) + c];
    let (a, b, c, d) = (at(j, i), at(j, i + 1), at(j + 1, i), at(j + 1, i + 1));
    let mix = |k: usize| {
        (1.0 - s) * (1.0 - t) * a[k] + s * (1.0 - t) * b[k] + (1.0 - s) * t * c[k] + s * t * d[k]
    };
    [mix(0), mix(1)]
}

/// The `(s, t)` in the cell with corners `a b / c d` that bilinear-maps to
/// `target`, if Newton's method converges; `ratio` is how far from flat the
/// map is there (1 for a square, near 0 for a collapsed cell).
fn solve_cell(a: P, b: P, c: P, d: P, target: P) -> Option<(f64, f64, f64)> {
    let (mut s, mut t) = (0.5, 0.5);
    for _ in 0..50 {
        let f = [
            (1.0 - s) * (1.0 - t) * a[0]
                + s * (1.0 - t) * b[0]
                + (1.0 - s) * t * c[0]
                + s * t * d[0]
                - target[0],
            (1.0 - s) * (1.0 - t) * a[1]
                + s * (1.0 - t) * b[1]
                + (1.0 - s) * t * c[1]
                + s * t * d[1]
                - target[1],
        ];
        let ds = [
            (1.0 - t) * (b[0] - a[0]) + t * (d[0] - c[0]),
            (1.0 - t) * (b[1] - a[1]) + t * (d[1] - c[1]),
        ];
        let dt = [
            (1.0 - s) * (c[0] - a[0]) + s * (d[0] - b[0]),
            (1.0 - s) * (c[1] - a[1]) + s * (d[1] - b[1]),
        ];
        let det = ds[0] * dt[1] - ds[1] * dt[0];
        let scale = ds[0].hypot(ds[1]) * dt[0].hypot(dt[1]);
        if scale == 0.0 || det.abs() <= 1e-12 * scale {
            return None;
        }
        let ratio = det.abs() / scale;
        let step_s = (f[0] * dt[1] - f[1] * dt[0]) / det;
        let step_t = (ds[0] * f[1] - ds[1] * f[0]) / det;
        s -= step_s;
        t -= step_t;
        if step_s.abs() < 1e-12 && step_t.abs() < 1e-12 {
            return Some((s, t, ratio));
        }
    }
    None
}

/// `(u, v)` such that [`warp_forward`] gives `world`: the cell holding the
/// point, or, outside the grid, the nearest edge cell extended. `Degenerate`
/// when that cell is collapsed.
pub fn warp_inverse(grid: &[P], rows: usize, columns: usize, world: P) -> Result<P, SpaceError> {
    let at = |r: usize, c: usize| grid[r * (columns + 1) + c];
    let mut best: Option<(f64, usize, usize, f64, f64)> = None;
    for j in 0..rows {
        for i in 0..columns {
            let (a, b, c, d) = (at(j, i), at(j, i + 1), at(j + 1, i), at(j + 1, i + 1));
            let Some((s, t, ratio)) = solve_cell(a, b, c, d, world) else {
                continue;
            };
            // Beyond a side is allowed only on the edge of the grid.
            let over = |v: f64, first: bool, last: bool| {
                if v < 0.0 {
                    if first { 0.0 } else { -v }
                } else if v > 1.0 {
                    if last { 0.0 } else { v - 1.0 }
                } else {
                    0.0
                }
            };
            let miss = over(s, i == 0, i + 1 == columns).max(over(t, j == 0, j + 1 == rows));
            // Inside the cell wins; otherwise the least outside.
            let outside = s.min(t) < -1e-9 || s.max(t) > 1.0 + 1e-9;
            let score = if outside { 1.0 + miss } else { 0.0 };
            if miss > 1e-9 || ratio < 1e-6 {
                continue;
            }
            if best.is_none_or(|(b, ..)| score < b) {
                best = Some((score, i, j, s, t));
            }
        }
    }
    best.map(|(_, i, j, s, t)| {
        [
            (i as f64 + s) / columns as f64,
            (j as f64 + t) / rows as f64,
        ]
    })
    .ok_or(SpaceError::Degenerate)
}

/// A child space seen from the drawing space.
#[derive(Clone, Debug, PartialEq)]
pub enum Space {
    /// No deformer: the drawing space itself.
    Drawing,
    Warp {
        /// The control points in the drawing space.
        grid: Vec<[f64; 2]>,
        rows: usize,
        columns: usize,
    },
    Rotation {
        origin: [f64; 2],
        /// Images of the local axes in the drawing space.
        ex: [f64; 2],
        ey: [f64; 2],
    },
}

impl Space {
    /// The space the children of `parent` live in, at the base shape.
    pub fn of(model: &Model, parent: Option<DeformerRef>) -> Result<Self, SpaceError> {
        Self::build(model, parent, 0)
    }

    fn build(model: &Model, parent: Option<DeformerRef>, depth: usize) -> Result<Self, SpaceError> {
        if depth > 64 {
            return Err(SpaceError::TooDeep);
        }
        match parent {
            None => Ok(Self::Drawing),
            Some(DeformerRef::Warp(id)) => {
                let Some(warp) = model.warps().get(id) else {
                    return Ok(Self::Drawing);
                };
                let up = Self::build(model, warp.parent, depth + 1)?;
                Ok(Self::Warp {
                    grid: warp
                        .base_points
                        .iter()
                        .map(|&p| up.to_world(p64(p)))
                        .collect(),
                    rows: warp.rows as usize,
                    columns: warp.columns as usize,
                })
            }
            Some(DeformerRef::Rotation(id)) => {
                let Some(rot) = model.rotations().get(id) else {
                    return Ok(Self::Drawing);
                };
                let up = Self::build(model, rot.parent, depth + 1)?;
                Ok(Self::rotation(&up, &RotationBase::of(&rot.base)))
            }
        }
    }

    /// The space a rotation deformer with base shape `base` gives its
    /// children, under the parent space `up`.
    pub fn rotation(up: &Self, base: &RotationBase) -> Self {
        let (sin, cos) = f64::from(base.angle_degrees).to_radians().sin_cos();
        let (sx, sy) = (
            if base.reflect_x { -1.0 } else { 1.0 },
            if base.reflect_y { -1.0 } else { 1.0 },
        );
        let s = f64::from(base.scale);
        let (lex, ley) = ([s * cos * sx, s * sin * sx], [-s * sin * sy, s * cos * sy]);
        Self::axes(up, p64(base.origin), lex, ley)
    }

    /// The rotation space with origin `o` and local axes `ex`, `ey` in `up`:
    /// axes follow the parent from a small step, as the evaluation does.
    fn axes(up: &Self, o: P, ex: P, ey: P) -> Self {
        const STEP: f64 = 0.01;
        let ow = up.to_world(o);
        let image = |axis: P| {
            let p = up.to_world([o[0] + STEP * axis[0], o[1] + STEP * axis[1]]);
            [(p[0] - ow[0]) / STEP, (p[1] - ow[1]) / STEP]
        };
        Self::Rotation {
            origin: ow,
            ex: image(ex),
            ey: image(ey),
        }
    }

    pub fn to_world(&self, p: P) -> P {
        match self {
            Self::Drawing => p,
            Self::Warp {
                grid,
                rows,
                columns,
            } => warp_forward(grid, *rows, *columns, p),
            Self::Rotation { origin, ex, ey } => [
                origin[0] + p[0] * ex[0] + p[1] * ey[0],
                origin[1] + p[0] * ex[1] + p[1] * ey[1],
            ],
        }
    }

    pub fn from_world(&self, w: P) -> Result<P, SpaceError> {
        match self {
            Self::Drawing => Ok(w),
            Self::Warp {
                grid,
                rows,
                columns,
            } => warp_inverse(grid, *rows, *columns, w),
            Self::Rotation { origin, ex, ey } => {
                let det = ex[0] * ey[1] - ex[1] * ey[0];
                let scale = ex[0].hypot(ex[1]) * ey[0].hypot(ey[1]);
                if scale == 0.0 || det.abs() <= 1e-12 * scale {
                    return Err(SpaceError::Degenerate);
                }
                let d = [w[0] - origin[0], w[1] - origin[1]];
                Ok([
                    (d[0] * ey[1] - d[1] * ey[0]) / det,
                    (ex[0] * d[1] - ex[1] * d[0]) / det,
                ])
            }
        }
    }

    /// A [`Point`] in this space to the drawing space.
    pub fn point_to_world(&self, p: Point) -> Point {
        point(self.to_world(p64(p)))
    }

    pub fn point_from_world(&self, w: Point) -> Result<Point, SpaceError> {
        self.from_world(p64(w)).map(point)
    }
}

/// `points` given in the child space of `from`, expressed in the child space
/// of `to`, so that they stay where they are in the drawing space.
pub fn convert(
    model: &Model,
    from: Option<DeformerRef>,
    to: Option<DeformerRef>,
    points: &[Point],
) -> Result<Vec<Point>, SpaceError> {
    let (from, to) = (Space::of(model, from)?, Space::of(model, to)?);
    points
        .iter()
        .map(|&p| to.from_world(from.to_world(p64(p))).map(point))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, RotationBase};
    use crate::compiled::CompiledModel;
    use crate::eval::Frame;

    fn model() -> Model {
        let mut m = Model::new();
        let commands = [
            Command::AddPart {
                name: "P".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "Warp".into(),
                part: "P".into(),
                parent: None,
                rows: 2,
                columns: 2,
                // A skewed 2 x 2 grid.
                base_points: vec![
                    [0.0, 0.0],
                    [1.2, -0.1],
                    [2.0, 0.1],
                    [-0.1, 1.0],
                    [1.0, 1.1],
                    [2.2, 0.9],
                    [0.1, 2.0],
                    [0.9, 2.2],
                    [2.0, 2.0],
                ],
            },
            Command::AddRotationDeformer {
                name: "Rot".into(),
                part: "P".into(),
                parent: Some("Warp".into()),
                base: RotationBase {
                    origin: [0.5, 0.5],
                    angle_degrees: 30.0,
                    scale: 0.5,
                    reflect_x: false,
                    reflect_y: true,
                },
            },
            Command::AddMesh {
                name: "Skin".into(),
                part: "P".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.5], [3.0, 2.0]],
                uvs: vec![[0.0, 0.0]; 5],
                indices: vec![0, 1, 2, 0, 2, 3, 3, 2, 4],
            },
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: Some("Rot".into()),
            },
        ];
        for c in commands {
            m.apply(c).unwrap();
        }
        m
    }

    #[test]
    fn the_space_matches_the_evaluation_of_a_warp_and_rotation_chain() {
        let m = model();
        let mesh = m.meshes().get(m.meshes().find("Skin").unwrap()).unwrap();
        let space = Space::of(&m, mesh.parent).unwrap();
        let compiled: CompiledModel = m.compile().unwrap();
        let mut frame = Frame::new(&compiled);
        frame.evaluate(&compiled);
        let drawn = frame.positions();
        for (i, p) in mesh.positions.iter().enumerate() {
            let w = space.point_to_world(*p);
            assert!(
                (w[0] - drawn[2 * i]).abs() < 1e-4 && (w[1] - drawn[2 * i + 1]).abs() < 1e-4,
                "{i}: {w:?}"
            );
        }
    }

    #[test]
    fn points_round_trip_through_every_space_including_outside_the_grid() {
        let m = model();
        for parent in [
            None,
            Some(DeformerRef::Warp(m.warps().find("Warp").unwrap())),
            Some(DeformerRef::Rotation(m.rotations().find("Rot").unwrap())),
        ] {
            let space = Space::of(&m, parent).unwrap();
            for p in [
                [0.3, 0.7],
                [0.0, 0.0],
                [1.0, 1.0],
                [-0.4, 1.3],
                [1.5, -0.2],
                [3.0, 2.0],
            ] {
                let w = space.point_to_world(p);
                let back = space.point_from_world(w).unwrap();
                assert!(
                    (back[0] - p[0]).abs() < 1e-4 && (back[1] - p[1]).abs() < 1e-4,
                    "{parent:?} {p:?} {back:?}"
                );
            }
        }
    }

    #[test]
    fn converting_between_parents_keeps_the_drawn_position() {
        let m = model();
        let warp = Some(DeformerRef::Warp(m.warps().find("Warp").unwrap()));
        let rot = Some(DeformerRef::Rotation(m.rotations().find("Rot").unwrap()));
        let points = vec![[0.2, 0.4], [0.9, 0.1], [-0.3, 0.6]];
        for to in [None, warp] {
            let there = convert(&m, rot, to, &points).unwrap();
            let before = Space::of(&m, rot).unwrap();
            let after = Space::of(&m, to).unwrap();
            for (p, q) in points.iter().zip(&there) {
                let (a, b) = (before.point_to_world(*p), after.point_to_world(*q));
                assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
            }
            let back = convert(&m, to, rot, &there).unwrap();
            for (p, q) in points.iter().zip(&back) {
                assert!((p[0] - q[0]).abs() < 1e-4 && (p[1] - q[1]).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn a_collapsed_cell_or_flat_axes_are_refused() {
        // All four corners of the only cell are one point.
        let grid = vec![[1.0, 1.0]; 4];
        assert_eq!(
            warp_inverse(&grid, 1, 1, [1.0, 1.0]),
            Err(SpaceError::Degenerate)
        );
        let flat = Space::Rotation {
            origin: [0.0, 0.0],
            ex: [1.0, 0.0],
            ey: [2.0, 0.0],
        };
        assert_eq!(flat.from_world([1.0, 1.0]), Err(SpaceError::Degenerate));
        // A cell squeezed to a line.
        let line = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [1.0, 0.0]];
        assert_eq!(
            warp_inverse(&line, 1, 1, [0.5, 0.0]),
            Err(SpaceError::Degenerate)
        );
    }
}
