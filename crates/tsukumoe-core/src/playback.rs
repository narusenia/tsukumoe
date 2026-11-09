// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Playback data: motions, expressions and the part pose.
//!
//! Counterparts of motion3, exp3 and pose3.

use serde::{Deserialize, Serialize};

use crate::elements::{ParameterId, PartId, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveTarget {
    Parameter(ParameterId),
    PartOpacity(PartId),
}

/// A curve segment ending at the given `(time, value)` point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
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

impl Segment {
    /// The point the segment ends at.
    pub fn end(&self) -> Point {
        match *self {
            Self::Linear(p) | Self::Stepped(p) | Self::InverseStepped(p) => p,
            Self::Bezier([_, _, p]) => p,
        }
    }
}

/// Why a key edit of a [`Curve`] was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The key index is past the last key.
    NoSuchKey,
    /// A time or value is not finite, or the time leaves the keys' order.
    BadKey,
    /// The curve has one key only.
    LastKey,
}

impl Curve {
    /// The number of keys: the start point and every segment's end.
    pub fn key_count(&self) -> usize {
        self.segments.len() + 1
    }

    /// The `(time, value)` of key `key`.
    pub fn key(&self, key: usize) -> Option<Point> {
        match key {
            0 => Some(self.start),
            k => self.segments.get(k - 1).map(Segment::end),
        }
    }

    /// The key at `time` (to within a microsecond), if there is one.
    pub fn key_at(&self, time: f32) -> Option<usize> {
        (0..self.key_count()).find(|&k| self.key(k).is_some_and(|p| (p[0] - time).abs() < 1e-6))
    }

    /// Whether every number is finite and each segment's points (controls
    /// included) lie between the previous key's time and its own end.
    pub fn is_ordered(&self) -> bool {
        let mut at = self.start[0];
        let mut ok = self.start.iter().all(|v| v.is_finite());
        for s in &self.segments {
            let end = s.end()[0];
            let points: &[Point] = match s {
                Segment::Bezier(p) => p,
                Segment::Linear(p) | Segment::Stepped(p) | Segment::InverseStepped(p) => {
                    std::slice::from_ref(p)
                }
            };
            ok &= points
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && p[0] >= at && p[0] <= end);
            at = end;
        }
        ok
    }

    /// The value at `time`: held before the start and after the last key.
    pub fn value_at(&self, time: f32) -> f32 {
        let mut from = self.start;
        for s in &self.segments {
            let end = s.end();
            if time < end[0] {
                return segment_value(from, s, time);
            }
            from = end;
        }
        from[1]
    }

    /// Adds a key at `time` that leaves the curve's shape as it was. A time
    /// after the last key adds a flat linear segment.
    pub fn insert_key(&mut self, time: f32) -> Result<usize, KeyError> {
        if !time.is_finite() || time <= self.start[0] {
            return Err(KeyError::BadKey);
        }
        let mut from = self.start;
        for i in 0..self.segments.len() {
            let end = self.segments[i].end();
            if time == end[0] {
                return Err(KeyError::BadKey);
            }
            if time < end[0] {
                let (a, b) = split(from, self.segments[i], time);
                self.segments[i] = b;
                self.segments.insert(i, a);
                return Ok(i + 1);
            }
            from = end;
        }
        self.segments.push(Segment::Linear([time, from[1]]));
        Ok(self.segments.len())
    }

    /// Removes key `key`; the segment after it (if any) now starts at the
    /// key before.
    pub fn remove_key(&mut self, key: usize) -> Result<(), KeyError> {
        if key >= self.key_count() {
            return Err(KeyError::NoSuchKey);
        }
        if self.segments.is_empty() {
            return Err(KeyError::LastKey);
        }
        if key == 0 {
            self.start = self.segments.remove(0).end();
        } else {
            self.segments.remove(key - 1);
        }
        Ok(())
    }

    /// Moves key `key` to `(time, value)`. The Bezier controls beside the key
    /// move with it. The time stays between the neighbors' times.
    pub fn move_key(&mut self, key: usize, time: f32, value: f32) -> Result<(), KeyError> {
        let Some(old) = self.key(key) else {
            return Err(KeyError::NoSuchKey);
        };
        let before = key.checked_sub(1).and_then(|k| self.key(k));
        let after = self.key(key + 1);
        if !time.is_finite()
            || !value.is_finite()
            || before.is_some_and(|b| time < b[0])
            || after.is_some_and(|a| time > a[0])
        {
            return Err(KeyError::BadKey);
        }
        let d = [time - old[0], value - old[1]];
        if key == 0 {
            self.start = [time, value];
        } else {
            self.segments[key - 1] = match self.segments[key - 1] {
                Segment::Bezier([a, b, _]) => Segment::Bezier([a, shift(b, d), [time, value]]),
                Segment::Linear(_) => Segment::Linear([time, value]),
                Segment::Stepped(_) => Segment::Stepped([time, value]),
                Segment::InverseStepped(_) => Segment::InverseStepped([time, value]),
            };
        }
        if let Some(Segment::Bezier([a, b, c])) = self.segments.get(key).copied() {
            self.segments[key] = Segment::Bezier([shift(a, d), b, c]);
        }
        Ok(())
    }

    /// Changes the kind of segment `segment` (the one that starts at key
    /// `segment`). The end point stays. A new Bezier gets its controls at the
    /// thirds of the straight line between its keys.
    pub fn set_kind(&mut self, segment: usize, kind: SegmentKind) -> Result<(), KeyError> {
        let (Some(from), Some(s)) = (self.key(segment), self.segments.get(segment).copied()) else {
            return Err(KeyError::NoSuchKey);
        };
        let end = s.end();
        if s.kind() == kind {
            return Ok(());
        }
        let third = |u: f32| {
            [
                from[0] + (end[0] - from[0]) * u,
                from[1] + (end[1] - from[1]) * u,
            ]
        };
        self.segments[segment] = match kind {
            SegmentKind::Linear => Segment::Linear(end),
            SegmentKind::Bezier => Segment::Bezier([third(1.0 / 3.0), third(2.0 / 3.0), end]),
            SegmentKind::Stepped => Segment::Stepped(end),
            SegmentKind::InverseStepped => Segment::InverseStepped(end),
        };
        Ok(())
    }

    /// Moves control `control` (0 or 1) of the Bezier segment `segment`. The
    /// time stays between the segment's keys, which keeps its time monotone.
    pub fn move_control(
        &mut self,
        segment: usize,
        control: usize,
        point: Point,
    ) -> Result<(), KeyError> {
        let (Some(from), Some(s)) = (self.key(segment), self.segments.get(segment).copied()) else {
            return Err(KeyError::NoSuchKey);
        };
        let Segment::Bezier(mut p) = s else {
            return Err(KeyError::BadKey);
        };
        if control > 1 || !point[0].is_finite() || !point[1].is_finite() {
            return Err(KeyError::BadKey);
        }
        p[control] = [point[0].clamp(from[0], p[2][0]), point[1]];
        self.segments[segment] = Segment::Bezier(p);
        Ok(())
    }
}

/// The kinds of [`Segment`], without their points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Linear,
    Bezier,
    Stepped,
    InverseStepped,
}

impl Segment {
    pub fn kind(&self) -> SegmentKind {
        match self {
            Self::Linear(_) => SegmentKind::Linear,
            Self::Bezier(_) => SegmentKind::Bezier,
            Self::Stepped(_) => SegmentKind::Stepped,
            Self::InverseStepped(_) => SegmentKind::InverseStepped,
        }
    }
}

fn shift(p: Point, d: Point) -> Point {
    [p[0] + d[0], p[1] + d[1]]
}

/// The point of a cubic Bezier at `u`.
fn bezier(p: [Point; 4], u: f32) -> Point {
    let l = |a: Point, b: Point| [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
    let (ab, bc, cd) = (l(p[0], p[1]), l(p[1], p[2]), l(p[2], p[3]));
    l(l(ab, bc), l(bc, cd))
}

/// The parameter at which a Bezier reaches `time` (its time is monotone).
fn bezier_param(p: [Point; 4], time: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        let mid = (lo + hi) / 2.0;
        if bezier(p, mid)[0] < time {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

fn segment_value(from: Point, s: &Segment, time: f32) -> f32 {
    match *s {
        Segment::Linear(e) => {
            let span = e[0] - from[0];
            if span > 0.0 {
                from[1] + (e[1] - from[1]) * (time - from[0]) / span
            } else {
                e[1]
            }
        }
        Segment::Stepped(_) => from[1],
        Segment::InverseStepped(e) => e[1],
        Segment::Bezier([a, b, e]) => {
            bezier([from, a, b, e], bezier_param([from, a, b, e], time))[1]
        }
    }
}

/// Splits the segment that starts at `from` at `time` into two that draw the
/// same curve.
fn split(from: Point, s: Segment, time: f32) -> (Segment, Segment) {
    match s {
        Segment::Linear(e) => {
            let mid = [time, segment_value(from, &s, time)];
            (Segment::Linear(mid), Segment::Linear(e))
        }
        Segment::Stepped(e) => (Segment::Stepped([time, from[1]]), Segment::Stepped(e)),
        Segment::InverseStepped(e) => (
            Segment::InverseStepped([time, e[1]]),
            Segment::InverseStepped(e),
        ),
        Segment::Bezier([a, b, e]) => {
            let u = bezier_param([from, a, b, e], time);
            let l = |p: Point, q: Point| [p[0] + (q[0] - p[0]) * u, p[1] + (q[1] - p[1]) * u];
            let (ab, bc, cd) = (l(from, a), l(a, b), l(b, e));
            let (abc, bcd) = (l(ab, bc), l(bc, cd));
            let mid = l(abc, bcd);
            (
                Segment::Bezier([ab, abc, [time, mid[1]]]),
                Segment::Bezier([bcd, cd, e]),
            )
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
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
