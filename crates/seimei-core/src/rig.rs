//! Rig elements: meshes, deformers, glue, bones and blend shapes.
//!
//! Keyforms hold absolute values, not deltas from a base shape (ADR-0018), so
//! a Cubism model maps onto them one to one.

use crate::arena::Id;
use crate::elements::{ParameterId, PartId, Point};
use crate::keyform::KeyformGrid;

pub type MeshId = Id<ArtMesh>;
pub type WarpId = Id<WarpDeformer>;
pub type RotationId = Id<RotationDeformer>;
pub type GlueId = Id<Glue>;
pub type BoneId = Id<Bone>;
pub type BlendShapeId = Id<BlendShape>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Additive,
    Multiply,
}

/// The deformer an element hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeformerRef {
    Warp(WarpId),
    Rotation(RotationId),
}

/// At most four bones drive one vertex (REQ-CORE-003).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VertexInfluences {
    pub bones: [Option<BoneId>; 4],
    pub weights: [f32; 4],
}

/// One keyform of an [`ArtMesh`]: shape, opacity, draw order and colors move
/// together, as in Cubism.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshKeyform {
    pub positions: Vec<Point>,
    pub opacity: f32,
    pub draw_order: f32,
    pub multiply_color: [f32; 3],
    pub screen_color: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtMesh {
    pub part: PartId,
    pub parent: Option<DeformerRef>,
    pub texture: u32,
    pub positions: Vec<Point>,
    pub uvs: Vec<Point>,
    /// Triangle list.
    pub indices: Vec<u32>,
    pub blend_mode: BlendMode,
    /// Meshes whose shape clips this one.
    pub masks: Vec<MeshId>,
    pub mask_inverted: bool,
    pub opacity: f32,
    pub draw_order: f32,
    pub multiply_color: [f32; 3],
    pub screen_color: [f32; 3],
    pub double_sided: bool,
    /// Empty, or one entry per vertex.
    pub influences: Vec<VertexInfluences>,
    pub keyforms: Option<KeyformGrid<MeshKeyform>>,
}

impl ArtMesh {
    pub fn new(part: PartId, positions: Vec<Point>, uvs: Vec<Point>, indices: Vec<u32>) -> Self {
        Self {
            part,
            parent: None,
            texture: 0,
            positions,
            uvs,
            indices,
            blend_mode: BlendMode::Normal,
            masks: Vec::new(),
            mask_inverted: false,
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
            double_sided: false,
            influences: Vec::new(),
            keyforms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarpKeyform {
    /// `(rows + 1) * (columns + 1)` control points, row by row.
    pub points: Vec<Point>,
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarpDeformer {
    pub part: PartId,
    pub parent: Option<DeformerRef>,
    pub rows: u32,
    pub columns: u32,
    /// Bezier-interpolated grid instead of bilinear.
    pub bezier: bool,
    pub base_points: Vec<Point>,
    pub keyforms: Option<KeyformGrid<WarpKeyform>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationKeyform {
    pub origin: Point,
    pub angle_degrees: f32,
    pub scale: f32,
    pub reflect_x: bool,
    pub reflect_y: bool,
    pub opacity: f32,
}

impl Default for RotationKeyform {
    fn default() -> Self {
        Self {
            origin: [0.0, 0.0],
            angle_degrees: 0.0,
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RotationDeformer {
    pub part: PartId,
    pub parent: Option<DeformerRef>,
    pub base: RotationKeyform,
    pub keyforms: Option<KeyformGrid<RotationKeyform>>,
}

/// A weighted vertex pair joining two meshes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlueNode {
    pub index_a: u32,
    pub index_b: u32,
    /// Share of the movement taken by `a`; `b` takes `1 - weight`.
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glue {
    pub mesh_a: MeshId,
    pub mesh_b: MeshId,
    pub nodes: Vec<GlueNode>,
    pub intensity: f32,
    pub keyforms: Option<KeyformGrid<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneTransform {
    pub translation: Point,
    pub rotation_degrees: f32,
    pub scale: Point,
}

impl Default for BoneTransform {
    fn default() -> Self {
        Self {
            translation: [0.0, 0.0],
            rotation_degrees: 0.0,
            scale: [1.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub parent: Option<BoneId>,
    pub base: BoneTransform,
    pub keyforms: Option<KeyformGrid<BoneTransform>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendTarget {
    Mesh(MeshId),
    Warp(WarpId),
}

/// A shape added to its target in proportion to a parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendShape {
    pub target: BlendTarget,
    pub parameter: ParameterId,
    /// Parameter values and the weight each maps to; same length.
    pub keys: Vec<f32>,
    pub weights: Vec<f32>,
    /// One offset per vertex (mesh) or control point (warp deformer).
    pub deltas: Vec<Point>,
}
