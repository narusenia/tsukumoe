// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Playback data: motions, expressions and the part pose.
//!
//! Counterparts of motion3, exp3 and pose3.

use crate::elements::{ParameterId, PartId, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveTarget {
    Parameter(ParameterId),
    PartOpacity(PartId),
}

/// A curve segment ending at the given `(time, value)` point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Linear(Point),
    /// Two control points, then the end point.
    Bezier([Point; 3]),
    /// Holds the previous value until the end time.
    Stepped(Point),
    /// Jumps to the new value at the start time.
    InverseStepped(Point),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub target: CurveTarget,
    pub start: Point,
    pub segments: Vec<Segment>,
    pub fade_in: Option<f32>,
    pub fade_out: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub duration: f32,
    pub fps: f32,
    pub looped: bool,
    pub fade_in: f32,
    pub fade_out: f32,
    pub curves: Vec<Curve>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionBlend {
    Add,
    Multiply,
    Overwrite,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionParameter {
    pub parameter: ParameterId,
    pub blend: ExpressionBlend,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub fade_in: f32,
    pub fade_out: f32,
    pub parameters: Vec<ExpressionParameter>,
}

/// A part that is shown together with linked parts.
#[derive(Debug, Clone, PartialEq)]
pub struct PoseEntry {
    pub part: PartId,
    pub linked: Vec<PartId>,
}

/// Parts in one group are mutually exclusive: showing one fades the others.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pose {
    pub fade_in: f32,
    pub groups: Vec<Vec<PoseEntry>>,
}
