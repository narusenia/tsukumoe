// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Model settings (M5-10): which parameter plays which standard role, the
//! hit areas, and the model's information. They are stored in `.tproj` and in
//! an optional `.tmod` section, and read by the runtime.

use serde::{Deserialize, Serialize};

use crate::elements::ParameterId;
use crate::rig::MeshId;

/// What a parameter does for auto blink, lip sync and tracking, so that they
/// need not guess from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StandardRole {
    AngleX,
    AngleY,
    AngleZ,
    BodyAngleX,
    BodyAngleY,
    BodyAngleZ,
    EyeLOpen,
    EyeROpen,
    EyeBallX,
    EyeBallY,
    MouthOpen,
    MouthForm,
    BrowLY,
    BrowRY,
}

impl StandardRole {
    /// Every role, in the order the editor lists them. A role's position is
    /// its code in `.tmod` ([`to_u8`](Self::to_u8)), so only append.
    pub const ALL: [Self; 14] = [
        Self::AngleX,
        Self::AngleY,
        Self::AngleZ,
        Self::BodyAngleX,
        Self::BodyAngleY,
        Self::BodyAngleZ,
        Self::EyeLOpen,
        Self::EyeROpen,
        Self::EyeBallX,
        Self::EyeBallY,
        Self::MouthOpen,
        Self::MouthForm,
        Self::BrowLY,
        Self::BrowRY,
    ];

    pub fn to_u8(self) -> u8 {
        Self::ALL
            .iter()
            .position(|&r| r == self)
            .expect("ALL lists every role") as u8
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }

    /// The standard parameter id (`ParamAngleX`, ...) for this role.
    pub fn standard_name(self) -> &'static str {
        match self {
            Self::AngleX => "ParamAngleX",
            Self::AngleY => "ParamAngleY",
            Self::AngleZ => "ParamAngleZ",
            Self::BodyAngleX => "ParamBodyAngleX",
            Self::BodyAngleY => "ParamBodyAngleY",
            Self::BodyAngleZ => "ParamBodyAngleZ",
            Self::EyeLOpen => "ParamEyeLOpen",
            Self::EyeROpen => "ParamEyeROpen",
            Self::EyeBallX => "ParamEyeBallX",
            Self::EyeBallY => "ParamEyeBallY",
            Self::MouthOpen => "ParamMouthOpenY",
            Self::MouthForm => "ParamMouthForm",
            Self::BrowLY => "ParamBrowLY",
            Self::BrowRY => "ParamBrowRY",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelInfo {
    pub title: String,
    pub author: String,
    pub license: String,
    pub notes: String,
}

/// A named region of the model: the mesh whose bounds answer a hit test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitArea {
    pub name: String,
    pub mesh: MeshId,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelSettings {
    pub info: ModelInfo,
    /// Names are unique.
    pub hit_areas: Vec<HitArea>,
    /// At most one parameter per role, in the order they were set.
    pub standard: Vec<(StandardRole, ParameterId)>,
}

impl ModelSettings {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn parameter(&self, role: StandardRole) -> Option<ParameterId> {
        self.standard
            .iter()
            .find(|(r, _)| *r == role)
            .map(|&(_, p)| p)
    }

    pub fn hit_area(&self, name: &str) -> Option<&HitArea> {
        self.hit_areas.iter().find(|a| a.name == name)
    }
}
