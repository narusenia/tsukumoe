// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Head rotation as angles. The order and the axis directions live in
//! [`head_angles`] and nowhere else.
//!
//! Convention (VMC / Unity): left-handed, X right, Y up, Z forward, all from
//! the performer's side. The rotation is `Ry(yaw) * Rx(pitch) * Rz(roll)`
//! (Unity's ZXY Euler order) with these signs:
//!
//! - yaw > 0: the nose turns toward +X (the performer's right)
//! - pitch > 0: the nose goes down
//! - roll > 0: the top of the head tilts toward -X (the performer's left)
//!
//! The sign a model wants is the mapping's business (rule weights).

/// `[yaw, pitch, roll]` in degrees of a rotation `[x, y, z, w]`. The result is
/// always finite: a zero or non-finite quaternion gives zeros, and at
/// pitch = ±90° (gimbal lock) yaw and roll are split arbitrarily but finite.
pub fn head_angles(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q.map(f64::from);
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if !norm.is_finite() || norm < 1e-12 {
        return [0.0; 3];
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    let yaw = (2.0 * (x * z + y * w)).atan2(1.0 - 2.0 * (x * x + y * y));
    let pitch = (2.0 * (x * w - y * z)).clamp(-1.0, 1.0).asin();
    let roll = (2.0 * (x * y + z * w)).atan2(1.0 - 2.0 * (x * x + z * z));
    [yaw, pitch, roll].map(|a| a.to_degrees() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rotation about a unit axis by `deg`.
    fn axis(a: [f32; 3], deg: f32) -> [f32; 4] {
        let (s, c) = (deg.to_radians() / 2.0).sin_cos();
        [a[0] * s, a[1] * s, a[2] * s, c]
    }

    fn near(a: [f32; 3], b: [f32; 3]) {
        for (a, b) in a.into_iter().zip(b) {
            assert!((a - b).abs() < 1e-3, "{a} != {b}");
        }
    }

    #[test]
    fn identity_is_zero() {
        near(head_angles([0.0, 0.0, 0.0, 1.0]), [0.0; 3]);
    }

    #[test]
    fn each_axis_gives_its_angle() {
        for d in [-30.0, 30.0] {
            near(head_angles(axis([0.0, 1.0, 0.0], d)), [d, 0.0, 0.0]);
            near(head_angles(axis([1.0, 0.0, 0.0], d)), [0.0, d, 0.0]);
            near(head_angles(axis([0.0, 0.0, 1.0], d)), [0.0, 0.0, d]);
        }
    }

    #[test]
    fn pitch_limits_and_bad_input_stay_finite() {
        for d in [-90.0, 90.0] {
            let a = head_angles(axis([1.0, 0.0, 0.0], d));
            assert!(a.iter().all(|v| v.is_finite()), "{a:?}");
            assert!((a[1] - d).abs() < 1e-2);
        }
        // A slightly de-normalized quaternion still gives a finite pitch.
        let a = head_angles([0.7072, 0.0, 0.0, 0.7072]);
        assert!(a.iter().all(|v| v.is_finite()));
        for q in [[0.0; 4], [f32::NAN; 4], [f32::INFINITY, 0.0, 0.0, 1.0]] {
            assert_eq!(head_angles(q), [0.0; 3]);
        }
    }
}
