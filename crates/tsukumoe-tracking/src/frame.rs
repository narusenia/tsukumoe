// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The common input type shared by every tracking adapter.

/// Number of ARKit face blendshapes.
pub const ARKIT_COUNT: usize = 52;

/// ARKit blendshape identifiers. The index order is fixed: alphabetical
/// (byte order) by the camelCase identifier. Indices are stable API.
pub const ARKIT_NAMES: [&str; ARKIT_COUNT] = [
    "browDownLeft",
    "browDownRight",
    "browInnerUp",
    "browOuterUpLeft",
    "browOuterUpRight",
    "cheekPuff",
    "cheekSquintLeft",
    "cheekSquintRight",
    "eyeBlinkLeft",
    "eyeBlinkRight",
    "eyeLookDownLeft",
    "eyeLookDownRight",
    "eyeLookInLeft",
    "eyeLookInRight",
    "eyeLookOutLeft",
    "eyeLookOutRight",
    "eyeLookUpLeft",
    "eyeLookUpRight",
    "eyeSquintLeft",
    "eyeSquintRight",
    "eyeWideLeft",
    "eyeWideRight",
    "jawForward",
    "jawLeft",
    "jawOpen",
    "jawRight",
    "mouthClose",
    "mouthDimpleLeft",
    "mouthDimpleRight",
    "mouthFrownLeft",
    "mouthFrownRight",
    "mouthFunnel",
    "mouthLeft",
    "mouthLowerDownLeft",
    "mouthLowerDownRight",
    "mouthPressLeft",
    "mouthPressRight",
    "mouthPucker",
    "mouthRight",
    "mouthRollLower",
    "mouthRollUpper",
    "mouthShrugLower",
    "mouthShrugUpper",
    "mouthSmileLeft",
    "mouthSmileRight",
    "mouthStretchLeft",
    "mouthStretchRight",
    "mouthUpperUpLeft",
    "mouthUpperUpRight",
    "noseSneerLeft",
    "noseSneerRight",
    "tongueOut",
];

/// Index of an ARKit blendshape by name (ASCII case-insensitive, so
/// Perfect-Sync style `JawOpen` also matches).
pub fn arkit_index(name: &str) -> Option<usize> {
    ARKIT_NAMES
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
}

/// One normalized tracking sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackingFrame {
    /// Blendshape weights in `0..=1`, indexed like [`ARKIT_NAMES`].
    pub blend: [f32; ARKIT_COUNT],
    /// Head rotation as a unit quaternion `[x, y, z, w]`.
    pub head_rotation: [f32; 4],
    /// Tracker translation, when the source provides one.
    pub head_position: Option<[f32; 3]>,
    /// Seconds on the producer's monotonic clock.
    pub time: f64,
}

impl Default for TrackingFrame {
    /// Neutral pose: all weights 0, identity rotation, no position.
    fn default() -> Self {
        Self {
            blend: [0.0; ARKIT_COUNT],
            head_rotation: [0.0, 0.0, 0.0, 1.0],
            head_position: None,
            time: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_unique_and_complete() {
        assert_eq!(ARKIT_NAMES.len(), 52);
        assert!(ARKIT_NAMES.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn lookup_by_name() {
        assert_eq!(arkit_index("browDownLeft"), Some(0));
        assert_eq!(arkit_index("JawOpen"), Some(24));
        assert_eq!(arkit_index("tongueOut"), Some(51));
        assert_eq!(arkit_index("A"), None);
    }
}
