// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The JSON shapes of the project files. Elements refer to each other by
//! persistent name, never by internal id (ADR-0018); keyform grids refer to a
//! shared binding by its position in `ModelFile::bindings`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type Point = [f32; 2];

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

fn no_color() -> [f32; 3] {
    [0.0; 3]
}

fn white() -> [f32; 3] {
    [1.0; 3]
}

fn default_order() -> f32 {
    500.0
}

fn unit_scale() -> Point {
    [1.0, 1.0]
}

/// `model.json`
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ModelFile {
    #[serde(default)]
    pub parameters: Vec<ParameterDto>,
    #[serde(default)]
    pub parts: Vec<PartDto>,
    #[serde(default)]
    pub bindings: Vec<BindingDto>,
    #[serde(default)]
    pub bones: Vec<BoneDto>,
    #[serde(default)]
    pub warps: Vec<WarpDto>,
    #[serde(default)]
    pub rotations: Vec<RotationDto>,
    #[serde(default)]
    pub meshes: Vec<MeshDto>,
    #[serde(default)]
    pub glues: Vec<GlueDto>,
    #[serde(default)]
    pub blend_shapes: Vec<BlendShapeDto>,
    #[serde(default)]
    pub drivers: Vec<DriverDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<PoseDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParameterDto {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default = "yes")]
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AxisDto {
    /// Parameter name.
    pub parameter: String,
    pub keys: Vec<f32>,
}

/// The parameters and key positions a set of keyforms is laid out on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BindingDto {
    pub axes: Vec<AxisDto>,
}

/// Keyforms of one element, row-major over the binding's axes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridDto<T> {
    /// Index into `bindings`.
    pub binding: usize,
    pub keyforms: Vec<T>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeformerRefDto {
    Warp { name: String },
    Rotation { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlendModeDto {
    Normal,
    Additive,
    Multiply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshKeyformDto {
    pub positions: Vec<Point>,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default = "default_order")]
    pub draw_order: f32,
    #[serde(default = "white")]
    pub multiply_color: [f32; 3],
    #[serde(default = "no_color")]
    pub screen_color: [f32; 3],
}

/// Up to four bones moving one vertex. Unused slots are `null` with weight 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InfluenceDto {
    pub bones: [Option<String>; 4],
    pub weights: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshDto {
    pub name: String,
    pub part: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<DeformerRefDto>,
    #[serde(default)]
    pub texture: u32,
    pub positions: Vec<Point>,
    pub uvs: Vec<Point>,
    /// Triangle list.
    pub indices: Vec<u32>,
    #[serde(default = "blend_normal")]
    pub blend_mode: BlendModeDto,
    /// Names of the meshes clipping this one.
    #[serde(default)]
    pub masks: Vec<String>,
    #[serde(default)]
    pub mask_inverted: bool,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default = "default_order")]
    pub draw_order: f32,
    #[serde(default = "white")]
    pub multiply_color: [f32; 3],
    #[serde(default = "no_color")]
    pub screen_color: [f32; 3],
    #[serde(default)]
    pub double_sided: bool,
    /// A hidden mesh is kept in the model but not drawn. Absent means shown.
    #[serde(default = "yes")]
    pub visible: bool,
    /// Empty, or one entry per vertex.
    #[serde(default)]
    pub influences: Vec<InfluenceDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyforms: Option<GridDto<MeshKeyformDto>>,
}

fn blend_normal() -> BlendModeDto {
    BlendModeDto::Normal
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WarpKeyformDto {
    pub points: Vec<Point>,
    #[serde(default = "one")]
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WarpDto {
    pub name: String,
    pub part: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<DeformerRefDto>,
    pub rows: u32,
    pub columns: u32,
    #[serde(default)]
    pub bezier: bool,
    /// `(rows + 1) * (columns + 1)` control points, row by row.
    pub base_points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyforms: Option<GridDto<WarpKeyformDto>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RotationKeyformDto {
    #[serde(default)]
    pub origin: Point,
    #[serde(default)]
    pub angle_degrees: f32,
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default)]
    pub reflect_x: bool,
    #[serde(default)]
    pub reflect_y: bool,
    #[serde(default = "one")]
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RotationDto {
    pub name: String,
    pub part: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<DeformerRefDto>,
    pub base: RotationKeyformDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyforms: Option<GridDto<RotationKeyformDto>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GlueNodeDto {
    pub index_a: u32,
    pub index_b: u32,
    /// Share of the movement taken by `a`; `b` takes the rest.
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GlueDto {
    pub name: String,
    pub mesh_a: String,
    pub mesh_b: String,
    pub nodes: Vec<GlueNodeDto>,
    #[serde(default = "one")]
    pub intensity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyforms: Option<GridDto<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoneTransformDto {
    #[serde(default)]
    pub translation: Point,
    #[serde(default)]
    pub rotation_degrees: f32,
    #[serde(default = "unit_scale")]
    pub scale: Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoneDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub base: BoneTransformDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyforms: Option<GridDto<BoneTransformDto>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlendTargetDto {
    Mesh { name: String },
    Warp { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlendShapeDto {
    pub name: String,
    pub target: BlendTargetDto,
    pub parameter: String,
    pub keys: Vec<f32>,
    pub weights: Vec<f32>,
    pub deltas: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DriverDto {
    pub name: String,
    /// Parameter name.
    pub output: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PoseEntryDto {
    pub part: String,
    #[serde(default)]
    pub linked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PoseDto {
    #[serde(default)]
    pub fade_in: f32,
    pub groups: Vec<Vec<PoseEntryDto>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChannelDto {
    X,
    Y,
    Angle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RangeDto {
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhysicsInputDto {
    pub parameter: String,
    pub channel: ChannelDto,
    pub weight: f32,
    #[serde(default)]
    pub reflect: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhysicsOutputDto {
    pub parameter: String,
    pub channel: ChannelDto,
    /// Index into the group's particles.
    pub particle: usize,
    pub scale: f32,
    pub weight: f32,
    #[serde(default)]
    pub reflect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParticleDto {
    pub position: Point,
    pub mobility: f32,
    pub delay: f32,
    pub acceleration: f32,
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PendulumDto {
    pub name: String,
    pub inputs: Vec<PhysicsInputDto>,
    pub outputs: Vec<PhysicsOutputDto>,
    pub particles: Vec<ParticleDto>,
    pub position_range: RangeDto,
    pub angle_range: RangeDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChainJointDto {
    pub length: f32,
    pub stiffness: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChainDto {
    pub name: String,
    pub joints: Vec<ChainJointDto>,
    pub gravity: Point,
    pub wind: Point,
    pub inputs: Vec<PhysicsInputDto>,
    pub outputs: Vec<PhysicsOutputDto>,
}

/// `physics.json`
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PhysicsFile {
    #[serde(default)]
    pub pendulums: Vec<PendulumDto>,
    #[serde(default)]
    pub chains: Vec<ChainDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CurveTargetDto {
    Parameter { name: String },
    PartOpacity { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SegmentDto {
    Linear {
        point: Point,
    },
    /// Two control points, then the end point.
    Bezier {
        points: [Point; 3],
    },
    Stepped {
        point: Point,
    },
    InverseStepped {
        point: Point,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CurveDto {
    pub target: CurveTargetDto,
    pub start: Point,
    pub segments: Vec<SegmentDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<f32>,
}

/// `motions/*.json`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MotionFile {
    pub name: String,
    pub duration: f32,
    pub fps: f32,
    #[serde(rename = "loop", default)]
    pub looped: bool,
    #[serde(default)]
    pub fade_in: f32,
    #[serde(default)]
    pub fade_out: f32,
    pub curves: Vec<CurveDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionBlendDto {
    Add,
    Multiply,
    Overwrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExpressionParameterDto {
    pub parameter: String,
    pub blend: ExpressionBlendDto,
    pub value: f32,
}

/// `expressions/*.json`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExpressionFile {
    pub name: String,
    #[serde(default)]
    pub fade_in: f32,
    #[serde(default)]
    pub fade_out: f32,
    pub parameters: Vec<ExpressionParameterDto>,
}
