// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Motions, expressions and the pose as flat arrays: the optional sections
//! `0x90..=0x99` of a `.tmod` (`docs/specifications/file-formats.md`, 2.3).
//! Parameters and parts are indices into the model's own arrays.

use std::borrow::Cow;
use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

use crate::compiled::{NONE, StrRef};
use crate::layout::LayoutError;
use crate::model::Model;
use crate::playback::{CurveTarget, ExpressionBlend, Segment};

/// `target_kind` of a curve: a parameter value.
pub const TARGET_PARAMETER: u32 = 0;
/// `target_kind` of a curve: a part's opacity.
pub const TARGET_PART_OPACITY: u32 = 1;
/// Curve flag: `fade_in` overrides the motion's.
pub const CURVE_FADE_IN: u32 = 1;
/// Curve flag: `fade_out` overrides the motion's.
pub const CURVE_FADE_OUT: u32 = 2;
pub const SEGMENT_LINEAR: u32 = 0;
pub const SEGMENT_BEZIER: u32 = 1;
pub const SEGMENT_STEPPED: u32 = 2;
pub const SEGMENT_INVERSE_STEPPED: u32 = 3;
pub const BLEND_ADD: u32 = 0;
pub const BLEND_MULTIPLY: u32 = 1;
pub const BLEND_OVERWRITE: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct MotionDef {
    pub name: StrRef,
    pub duration: f32,
    pub fps: f32,
    /// 0 or 1.
    pub looped: u32,
    pub fade_in: f32,
    pub fade_out: f32,
    pub curve_start: u32,
    pub curve_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct CurveDef {
    pub target_kind: u32,
    /// A parameter or part index.
    pub target: u32,
    pub flags: u32,
    pub fade_in: f32,
    pub fade_out: f32,
    pub start_time: f32,
    pub start_value: f32,
    pub segment_start: u32,
    pub segment_count: u32,
}

/// A segment ending at `(end_time, end_value)`. The controls of a Bezier
/// are the two points before the end; other kinds write them as 0.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct SegmentDef {
    pub kind: u32,
    pub end_time: f32,
    pub end_value: f32,
    pub c1_time: f32,
    pub c1_value: f32,
    pub c2_time: f32,
    pub c2_value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ExpressionDef {
    pub name: StrRef,
    pub fade_in: f32,
    pub fade_out: f32,
    pub param_start: u32,
    pub param_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ExpressionParamDef {
    pub parameter: u32,
    pub blend: u32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct PoseDef {
    pub fade_in: f32,
    pub group_start: u32,
    pub group_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct PoseGroupDef {
    pub entry_start: u32,
    pub entry_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct PoseEntryDef {
    pub part: u32,
    pub link_start: u32,
    pub link_count: u32,
}

/// The playback arrays. Empty (the default) when a file has none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaybackData<'a> {
    /// UTF-8 bytes the motion and expression names point into.
    pub strings: Cow<'a, [u8]>,
    pub motions: Cow<'a, [MotionDef]>,
    pub curves: Cow<'a, [CurveDef]>,
    pub segments: Cow<'a, [SegmentDef]>,
    pub expressions: Cow<'a, [ExpressionDef]>,
    pub expression_params: Cow<'a, [ExpressionParamDef]>,
    /// Zero or one element.
    pub pose: Cow<'a, [PoseDef]>,
    pub pose_groups: Cow<'a, [PoseGroupDef]>,
    pub pose_entries: Cow<'a, [PoseEntryDef]>,
    /// Part indices linked to an entry.
    pub pose_links: Cow<'a, [u32]>,
}

type Check = Result<(), LayoutError>;

fn ensure(ok: bool, array: &'static str, index: usize, reason: &'static str) -> Check {
    if ok {
        Ok(())
    } else {
        Err(LayoutError {
            array,
            index,
            reason,
        })
    }
}

fn within(start: u32, count: u32, len: usize) -> bool {
    start != NONE && u64::from(start) + u64::from(count) <= len as u64
}

fn fade(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

impl PlaybackData<'_> {
    /// No motion, expression or pose: a writer then omits the sections.
    pub fn is_empty(&self) -> bool {
        self.motions.is_empty() && self.expressions.is_empty() && self.pose.is_empty()
    }

    pub fn into_owned(self) -> PlaybackData<'static> {
        PlaybackData {
            strings: Cow::Owned(self.strings.into_owned()),
            motions: Cow::Owned(self.motions.into_owned()),
            curves: Cow::Owned(self.curves.into_owned()),
            segments: Cow::Owned(self.segments.into_owned()),
            expressions: Cow::Owned(self.expressions.into_owned()),
            expression_params: Cow::Owned(self.expression_params.into_owned()),
            pose: Cow::Owned(self.pose.into_owned()),
            pose_groups: Cow::Owned(self.pose_groups.into_owned()),
            pose_entries: Cow::Owned(self.pose_entries.into_owned()),
            pose_links: Cow::Owned(self.pose_links.into_owned()),
        }
    }

    fn str(&self, r: StrRef) -> &str {
        let start = r.offset as usize;
        start
            .checked_add(r.len as usize)
            .and_then(|end| self.strings.get(start..end))
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    pub fn motion_name(&self, i: usize) -> &str {
        self.str(self.motions[i].name)
    }

    pub fn expression_name(&self, i: usize) -> &str {
        self.str(self.expressions[i].name)
    }

    pub fn motion_index(&self, name: &str) -> Option<usize> {
        (0..self.motions.len()).find(|&i| self.motion_name(i) == name)
    }

    pub fn expression_index(&self, name: &str) -> Option<usize> {
        (0..self.expressions.len()).find(|&i| self.expression_name(i) == name)
    }

    pub fn motion_curves(&self, m: &MotionDef) -> &[CurveDef] {
        &self.curves[m.curve_start as usize..(m.curve_start + m.curve_count) as usize]
    }

    pub fn curve_segments(&self, c: &CurveDef) -> &[SegmentDef] {
        &self.segments[c.segment_start as usize..(c.segment_start + c.segment_count) as usize]
    }

    pub fn expression_params_of(&self, e: &ExpressionDef) -> &[ExpressionParamDef] {
        &self.expression_params[e.param_start as usize..(e.param_start + e.param_count) as usize]
    }

    pub fn pose_groups_of(&self, p: &PoseDef) -> &[PoseGroupDef] {
        &self.pose_groups[p.group_start as usize..(p.group_start + p.group_count) as usize]
    }

    pub fn group_entries(&self, g: &PoseGroupDef) -> &[PoseEntryDef] {
        &self.pose_entries[g.entry_start as usize..(g.entry_start + g.entry_count) as usize]
    }

    pub fn entry_links(&self, e: &PoseEntryDef) -> &[u32] {
        &self.pose_links[e.link_start as usize..(e.link_start + e.link_count) as usize]
    }

    /// Everything the player relies on: ranges, indices into `parameters`
    /// and `parts` elements, enumerations, finite numbers, and curve times
    /// that never run backwards (a Bezier's control times stay between its
    /// ends, so its time axis is monotonic).
    pub fn check(&self, parameters: usize, parts: usize) -> Check {
        let name_ok = |r: &StrRef| {
            within(r.offset, r.len, self.strings.len())
                && std::str::from_utf8(
                    &self.strings[r.offset as usize..(r.offset + r.len) as usize],
                )
                .is_ok()
        };
        ensure(
            self.strings.len() < NONE as usize,
            "playback_strings",
            0,
            "too many bytes",
        )?;
        ensure(self.pose.len() <= 1, "pose", 0, "at most one pose")?;
        for (i, m) in self.motions.iter().enumerate() {
            ensure(name_ok(&m.name), "motions", i, "bad name")?;
            ensure(
                fade(m.duration) && fade(m.fps) && fade(m.fade_in) && fade(m.fade_out),
                "motions",
                i,
                "duration, fps or fade is negative or not finite",
            )?;
            ensure(m.looped <= 1, "motions", i, "bad flag")?;
            ensure(
                within(m.curve_start, m.curve_count, self.curves.len()),
                "motions",
                i,
                "curves outside the array",
            )?;
        }
        for (i, c) in self.curves.iter().enumerate() {
            let limit = match c.target_kind {
                TARGET_PARAMETER => parameters,
                TARGET_PART_OPACITY => parts,
                _ => return ensure(false, "curves", i, "unknown target kind"),
            };
            ensure(
                (c.target as usize) < limit,
                "curves",
                i,
                "target outside its array",
            )?;
            ensure(c.flags <= 3, "curves", i, "unknown flags")?;
            ensure(
                fade(c.fade_in)
                    && fade(c.fade_out)
                    && c.start_time.is_finite()
                    && c.start_value.is_finite(),
                "curves",
                i,
                "number is negative or not finite",
            )?;
            ensure(
                within(c.segment_start, c.segment_count, self.segments.len()),
                "curves",
                i,
                "segments outside the array",
            )?;
            let mut time = c.start_time;
            for (k, s) in self.curve_segments(c).iter().enumerate() {
                let all = [
                    s.end_time,
                    s.end_value,
                    s.c1_time,
                    s.c1_value,
                    s.c2_time,
                    s.c2_value,
                ];
                ensure(
                    s.kind <= SEGMENT_INVERSE_STEPPED && all.iter().all(|v| v.is_finite()),
                    "segments",
                    c.segment_start as usize + k,
                    "unknown kind or number not finite",
                )?;
                let bezier_ok = s.kind != SEGMENT_BEZIER
                    || (time <= s.c1_time && s.c1_time <= s.c2_time && s.c2_time <= s.end_time);
                ensure(
                    s.end_time >= time && bezier_ok,
                    "segments",
                    c.segment_start as usize + k,
                    "times run backwards",
                )?;
                time = s.end_time;
            }
        }
        for (i, e) in self.expressions.iter().enumerate() {
            ensure(name_ok(&e.name), "expressions", i, "bad name")?;
            ensure(
                fade(e.fade_in) && fade(e.fade_out),
                "expressions",
                i,
                "fade is negative or not finite",
            )?;
            ensure(
                within(e.param_start, e.param_count, self.expression_params.len()),
                "expressions",
                i,
                "parameters outside the array",
            )?;
        }
        for (i, p) in self.expression_params.iter().enumerate() {
            ensure(
                (p.parameter as usize) < parameters
                    && p.blend <= BLEND_OVERWRITE
                    && p.value.is_finite(),
                "expression_params",
                i,
                "parameter outside its array, unknown blend or value not finite",
            )?;
        }
        for (i, p) in self.pose.iter().enumerate() {
            ensure(fade(p.fade_in), "pose", i, "fade is negative or not finite")?;
            ensure(
                within(p.group_start, p.group_count, self.pose_groups.len()),
                "pose",
                i,
                "groups outside the array",
            )?;
        }
        for (i, g) in self.pose_groups.iter().enumerate() {
            ensure(
                within(g.entry_start, g.entry_count, self.pose_entries.len()),
                "pose_groups",
                i,
                "entries outside the array",
            )?;
        }
        for (i, e) in self.pose_entries.iter().enumerate() {
            ensure(
                (e.part as usize) < parts
                    && within(e.link_start, e.link_count, self.pose_links.len()),
                "pose_entries",
                i,
                "part or links outside their arrays",
            )?;
        }
        for (i, &part) in self.pose_links.iter().enumerate() {
            ensure(
                (part as usize) < parts,
                "pose_links",
                i,
                "part outside the array",
            )?;
        }
        Ok(())
    }
}

fn idx(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(NONE)
}

fn push_str(strings: &mut Vec<u8>, text: &str) -> StrRef {
    let r = StrRef {
        offset: idx(strings.len()),
        len: idx(text.len()),
    };
    strings.extend_from_slice(text.as_bytes());
    r
}

impl PlaybackData<'static> {
    /// Flattens the motions, expressions and pose of `model`, with parameters
    /// and parts as the indices [`Model::compile`] gives them. Fails when the
    /// data breaks the rules of [`PlaybackData::check`].
    pub fn from_model(model: &Model) -> Result<Self, LayoutError> {
        let param_index: HashMap<_, _> = model
            .parameters
            .iter()
            .enumerate()
            .map(|(i, (id, _, _))| (id, i))
            .collect();
        let part_index: HashMap<_, _> = model
            .part_order()
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let mut strings = Vec::new();
        let mut d = PlaybackData::default();
        for (_, name, m) in model.motions.iter() {
            let curve_start = idx(d.curves.len());
            for c in &m.curves {
                let (target_kind, target) = match c.target {
                    CurveTarget::Parameter(id) => (TARGET_PARAMETER, param_index[&id]),
                    CurveTarget::PartOpacity(id) => (TARGET_PART_OPACITY, part_index[&id]),
                };
                let segment_start = idx(d.segments.len());
                for s in &c.segments {
                    let (kind, end, c1, c2) = match *s {
                        Segment::Linear(p) => (SEGMENT_LINEAR, p, [0.0; 2], [0.0; 2]),
                        Segment::Bezier([a, b, p]) => (SEGMENT_BEZIER, p, a, b),
                        Segment::Stepped(p) => (SEGMENT_STEPPED, p, [0.0; 2], [0.0; 2]),
                        Segment::InverseStepped(p) => {
                            (SEGMENT_INVERSE_STEPPED, p, [0.0; 2], [0.0; 2])
                        }
                    };
                    d.segments.to_mut().push(SegmentDef {
                        kind,
                        end_time: end[0],
                        end_value: end[1],
                        c1_time: c1[0],
                        c1_value: c1[1],
                        c2_time: c2[0],
                        c2_value: c2[1],
                    });
                }
                d.curves.to_mut().push(CurveDef {
                    target_kind,
                    target: idx(target),
                    flags: (u32::from(c.fade_in.is_some()) * CURVE_FADE_IN)
                        | (u32::from(c.fade_out.is_some()) * CURVE_FADE_OUT),
                    fade_in: c.fade_in.unwrap_or(0.0),
                    fade_out: c.fade_out.unwrap_or(0.0),
                    start_time: c.start[0],
                    start_value: c.start[1],
                    segment_start,
                    segment_count: idx(c.segments.len()),
                });
            }
            let name = push_str(&mut strings, name);
            d.motions.to_mut().push(MotionDef {
                name,
                duration: m.duration,
                fps: m.fps,
                looped: m.looped.into(),
                fade_in: m.fade_in,
                fade_out: m.fade_out,
                curve_start,
                curve_count: idx(m.curves.len()),
            });
        }
        for (_, name, e) in model.expressions.iter() {
            let param_start = idx(d.expression_params.len());
            for p in &e.parameters {
                d.expression_params.to_mut().push(ExpressionParamDef {
                    parameter: idx(param_index[&p.parameter]),
                    blend: match p.blend {
                        ExpressionBlend::Add => BLEND_ADD,
                        ExpressionBlend::Multiply => BLEND_MULTIPLY,
                        ExpressionBlend::Overwrite => BLEND_OVERWRITE,
                    },
                    value: p.value,
                });
            }
            let name = push_str(&mut strings, name);
            d.expressions.to_mut().push(ExpressionDef {
                name,
                fade_in: e.fade_in,
                fade_out: e.fade_out,
                param_start,
                param_count: idx(e.parameters.len()),
            });
        }
        if let Some(pose) = &model.pose {
            let group_start = idx(d.pose_groups.len());
            for group in &pose.groups {
                let entry_start = idx(d.pose_entries.len());
                for e in group {
                    let link_start = idx(d.pose_links.len());
                    d.pose_links
                        .to_mut()
                        .extend(e.linked.iter().map(|id| idx(part_index[id])));
                    d.pose_entries.to_mut().push(PoseEntryDef {
                        part: idx(part_index[&e.part]),
                        link_start,
                        link_count: idx(e.linked.len()),
                    });
                }
                d.pose_groups.to_mut().push(PoseGroupDef {
                    entry_start,
                    entry_count: idx(group.len()),
                });
            }
            d.pose.to_mut().push(PoseDef {
                fade_in: pose.fade_in,
                group_start,
                group_count: idx(pose.groups.len()),
            });
        }
        d.strings = Cow::Owned(strings);
        d.check(model.parameters.len(), model.parts.len())?;
        Ok(d)
    }
}
