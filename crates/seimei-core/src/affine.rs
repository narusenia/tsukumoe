// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! 2D affine transforms. Rotation is counter-clockwise, in degrees where a
//! constructor takes an angle.

use crate::elements::Point;
use crate::rig::BoneTransform;

/// Maps a point to `[m0 x + m2 y + t0, m1 x + m3 y + t1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub m: [f32; 4],
    pub t: [f32; 2],
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Self = Self {
        m: [1.0, 0.0, 0.0, 1.0],
        t: [0.0, 0.0],
    };

    pub fn apply(&self, p: Point) -> Point {
        [
            self.m[0] * p[0] + self.m[2] * p[1] + self.t[0],
            self.m[1] * p[0] + self.m[3] * p[1] + self.t[1],
        ]
    }

    /// Scale, then rotate, then translate.
    pub fn from_bone(b: &BoneTransform) -> Self {
        let (sin, cos) = b.rotation_degrees.to_radians().sin_cos();
        Self {
            m: [
                cos * b.scale[0],
                sin * b.scale[0],
                -sin * b.scale[1],
                cos * b.scale[1],
            ],
            t: b.translation,
        }
    }

    /// The transform that applies `other` first, then `self`.
    pub fn then_after(&self, other: &Self) -> Self {
        let (a, b) = (&self.m, &other.m);
        Self {
            m: [
                a[0] * b[0] + a[2] * b[1],
                a[1] * b[0] + a[3] * b[1],
                a[0] * b[2] + a[2] * b[3],
                a[1] * b[2] + a[3] * b[3],
            ],
            t: self.apply(other.t),
        }
    }

    /// `None` when the transform is not invertible.
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.m;
        let det = m[0] * m[3] - m[2] * m[1];
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let inv = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det];
        let t = [
            -(inv[0] * self.t[0] + inv[2] * self.t[1]),
            -(inv[1] * self.t[0] + inv[3] * self.t[1]),
        ];
        Some(Self { m: inv, t })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Point, b: Point) {
        assert!(
            (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5,
            "{a:?} vs {b:?}"
        );
    }

    fn bone(translation: Point, rotation_degrees: f32, scale: Point) -> Affine {
        Affine::from_bone(&BoneTransform {
            translation,
            rotation_degrees,
            scale,
        })
    }

    #[test]
    fn from_bone_scales_then_rotates_then_translates() {
        near(
            bone([1.0, 2.0], 90.0, [2.0, 1.0]).apply([1.0, 0.0]),
            [1.0, 4.0],
        );
    }

    #[test]
    fn composition_applies_the_argument_first() {
        let turn = bone([0.0, 0.0], 90.0, [1.0, 1.0]);
        let shift = bone([1.0, 0.0], 0.0, [1.0, 1.0]);
        near(turn.then_after(&shift).apply([0.0, 0.0]), [0.0, 1.0]);
        near(shift.then_after(&turn).apply([1.0, 0.0]), [1.0, 1.0]);
    }

    #[test]
    fn inverse_undoes_the_transform() {
        let a = bone([3.0, -1.0], 33.0, [2.0, 0.5]);
        near(a.inverse().unwrap().apply(a.apply([0.7, 1.1])), [0.7, 1.1]);
        assert!(bone([0.0, 0.0], 0.0, [0.0, 1.0]).inverse().is_none());
    }
}
