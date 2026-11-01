// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Drag-to-parameter: modifier + drag in the viewport moves assigned
//! parameters. The assignment (modifier, which parameter each axis drives,
//! sensitivity, direction) is data the user can change and save. Values go to
//! the view state, clamped to the parameter's range; the model and the undo
//! history are not touched.
//!
//! The pan modifier of the viewport is Shift (middle drag also pans), so the
//! drag modifier must be another one; Space stays free for the host viewer's
//! Space + drag.

use serde::{Deserialize, Serialize};
use tsukumoe_core::elements::Parameter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Alt,
    Ctrl,
    /// Command on macOS.
    Cmd,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AxisTarget {
    /// The parameter's name in the model.
    pub parameter: String,
    /// Pointer travel, in points, that moves the parameter across its range.
    pub points_per_range: f32,
    #[serde(default)]
    pub invert: bool,
}

impl AxisTarget {
    /// The value after the pointer moved `delta` points along this axis,
    /// clamped to the parameter's range. Screen y grows downward, so a
    /// vertical axis is inverted to make "up" increase the value.
    pub fn apply(&self, p: &Parameter, current: f32, delta: f32, vertical: bool) -> f32 {
        let mut sign = if self.invert { -1.0 } else { 1.0 };
        if vertical {
            sign = -sign;
        }
        let travel = self.points_per_range.abs().max(1.0);
        let value = current + sign * delta / travel * (p.max - p.min);
        value.clamp(p.min, p.max)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct DragAssignment {
    pub modifier: Modifier,
    pub x: Option<AxisTarget>,
    pub y: Option<AxisTarget>,
}

impl Default for DragAssignment {
    /// Alt + drag: horizontal turns the head, vertical tilts it.
    fn default() -> Self {
        let axis = |name: &str| {
            Some(AxisTarget {
                parameter: name.to_owned(),
                points_per_range: 400.0,
                invert: false,
            })
        };
        Self {
            modifier: Modifier::Alt,
            x: axis("ParamAngleX"),
            y: axis("ParamAngleY"),
        }
    }
}

impl DragAssignment {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("assignment serializes")
    }

    /// A damaged file gives the default.
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn modifier_down(&self, m: egui::Modifiers) -> bool {
        match self.modifier {
            Modifier::Alt => m.alt,
            Modifier::Ctrl => m.ctrl,
            Modifier::Cmd => m.mac_cmd || m.command && !cfg!(target_os = "macos"),
        }
    }

    /// The new values for a pointer move of `delta` points: for each axis that
    /// has a target present in the model, `(parameter name, value)`.
    pub fn drive(
        &self,
        delta: [f32; 2],
        mut lookup: impl FnMut(&str) -> Option<(Parameter, f32)>,
    ) -> Vec<(String, f32)> {
        let mut out = Vec::new();
        for (axis, d, vertical) in [(&self.x, delta[0], false), (&self.y, delta[1], true)] {
            let Some(axis) = axis else { continue };
            if d == 0.0 {
                continue;
            }
            if let Some((p, current)) = lookup(&axis.parameter) {
                out.push((axis.parameter.clone(), axis.apply(&p, current, d, vertical)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle() -> Parameter {
        Parameter::new(-30.0, 30.0, 0.0)
    }

    #[test]
    fn dragging_moves_the_parameter_by_its_share_of_the_range() {
        let a = DragAssignment::default();
        let lookup = |n: &str| (n == "ParamAngleX" || n == "ParamAngleY").then(|| (angle(), 0.0));
        // 100 of 400 points: a quarter of the 60 range
        let out = a.drive([100.0, 0.0], lookup);
        assert_eq!(out, [("ParamAngleX".to_owned(), 15.0)]);
        // dragging up (negative screen y) raises the vertical parameter
        let out = a.drive([0.0, -100.0], lookup);
        assert_eq!(out, [("ParamAngleY".to_owned(), 15.0)]);
    }

    #[test]
    fn values_clamp_to_the_range_and_invert_flips_direction() {
        let mut axis = AxisTarget {
            parameter: "P".into(),
            points_per_range: 100.0,
            invert: false,
        };
        let p = angle();
        assert_eq!(axis.apply(&p, 25.0, 1000.0, false), 30.0);
        assert_eq!(axis.apply(&p, -25.0, -1000.0, false), -30.0);
        axis.invert = true;
        // half the travel against the direction: -30
        assert_eq!(axis.apply(&p, 0.0, 50.0, false), -30.0);
        // a tiny sensitivity value cannot divide by zero
        axis.points_per_range = 0.0;
        assert!(axis.apply(&p, 0.0, 5.0, false).is_finite());
    }

    #[test]
    fn missing_parameters_and_zero_motion_do_nothing() {
        let a = DragAssignment::default();
        assert!(a.drive([5.0, 5.0], |_| None).is_empty());
        assert!(a.drive([0.0, 0.0], |_| Some((angle(), 0.0))).is_empty());
        let one_axis = DragAssignment {
            y: None,
            ..DragAssignment::default()
        };
        assert_eq!(
            one_axis.drive([5.0, 5.0], |_| Some((angle(), 0.0))).len(),
            1
        );
    }

    #[test]
    fn the_assignment_round_trips_and_a_damaged_file_gives_the_default() {
        let a = DragAssignment {
            modifier: Modifier::Ctrl,
            x: None,
            y: Some(AxisTarget {
                parameter: "Q".into(),
                points_per_range: 250.0,
                invert: true,
            }),
        };
        assert_eq!(DragAssignment::from_json(&a.to_json()), a);
        assert_eq!(DragAssignment::from_json("{"), DragAssignment::default());
        assert_eq!(
            DragAssignment::from_json(r#"{"modifier":"hyper","x":null,"y":null}"#),
            DragAssignment::default()
        );
    }

    #[test]
    fn the_modifier_is_checked_against_egui_modifiers() {
        let a = DragAssignment::default();
        assert!(a.modifier_down(egui::Modifiers::ALT));
        assert!(!a.modifier_down(egui::Modifiers::SHIFT));
        assert!(!a.modifier_down(egui::Modifiers::NONE));
    }
}
