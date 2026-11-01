// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `Model` to and from the project files: `model.json`, `physics.json`,
//! `motions/*.json` and `expressions/*.json`.

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use tsukumoe_core::arena::Id;
use tsukumoe_core::driver::Driver;
use tsukumoe_core::elements::{Parameter, Part};
use tsukumoe_core::keyform::{Axis, Binding, BindingId, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::physics::{
    ChainJoint, Channel, Particle, PendulumGroup, PhysicsChain, PhysicsInput, PhysicsOutput,
    PhysicsRig, Range,
};
use tsukumoe_core::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Pose, PoseEntry,
    Segment,
};
use tsukumoe_core::rig::{
    ArtMesh, BlendMode, BlendShape, BlendTarget, Bone, BoneTransform, DeformerRef, Glue, GlueNode,
    MeshKeyform, RotationDeformer, RotationKeyform, VertexInfluences, WarpDeformer, WarpKeyform,
};
use tsukumoe_core::table::Table;

use crate::container::{Project, to_json};
use crate::dto::*;
use crate::error::Error;

pub const MODEL_PATH: &str = "model.json";
pub const PHYSICS_PATH: &str = "physics.json";
pub const MOTIONS_DIR: &str = "motions/";
pub const EXPRESSIONS_DIR: &str = "expressions/";

/// Replaces the model's files in `project`. Other files (textures, sources)
/// are left alone.
pub fn write_model(model: &Model, project: &mut Project) -> Result<(), Error> {
    let errors = model.validate();
    if !errors.is_empty() {
        return Err(Error::Invalid(format!("{errors:?}")));
    }
    let w = Writer {
        m: model,
        bindings: model
            .bindings()
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id, i))
            .collect(),
    };
    let model_file = w.model_file()?;
    let physics = w.physics()?;
    let mut motions = Vec::new();
    for (i, (_, name, motion)) in model.motions().iter().enumerate() {
        motions.push((numbered(MOTIONS_DIR, i, name), w.motion(name, motion)?));
    }
    let mut expressions = Vec::new();
    for (i, (_, name, e)) in model.expressions().iter().enumerate() {
        expressions.push((numbered(EXPRESSIONS_DIR, i, name), w.expression(name, e)?));
    }

    project
        .files
        .retain(|path, _| !path.starts_with(MOTIONS_DIR) && !path.starts_with(EXPRESSIONS_DIR));
    project
        .files
        .insert(MODEL_PATH.to_owned(), to_json(&model_file)?);
    project
        .files
        .insert(PHYSICS_PATH.to_owned(), to_json(&physics)?);
    for (path, file) in motions {
        project.files.insert(path, to_json(&file)?);
    }
    for (path, file) in expressions {
        project.files.insert(path, to_json(&file)?);
    }
    Ok(())
}

/// Reads the model from `project`. The result is a consistent model or an
/// error naming what is wrong.
pub fn read_model(project: &Project) -> Result<Model, Error> {
    let model_file: ModelFile =
        parse(project, MODEL_PATH)?.ok_or_else(|| Error::MissingFile(MODEL_PATH.to_owned()))?;
    let physics: PhysicsFile = parse(project, PHYSICS_PATH)?.unwrap_or_default();
    let mut motions = Vec::new();
    let mut expressions = Vec::new();
    // Path order is the order the writer numbered them in.
    for path in project.files.keys() {
        if path.ends_with(".json") && path.starts_with(MOTIONS_DIR) {
            motions.push(parse::<MotionFile>(project, path)?.expect("listed file exists"));
        } else if path.ends_with(".json") && path.starts_with(EXPRESSIONS_DIR) {
            expressions.push(parse::<ExpressionFile>(project, path)?.expect("listed file exists"));
        }
    }
    Reader::default().build(&model_file, &physics, &motions, &expressions)
}

fn parse<T: DeserializeOwned>(project: &Project, path: &str) -> Result<Option<T>, Error> {
    match project.files.get(path) {
        None => Ok(None),
        Some(bytes) => serde_json::from_slice(bytes)
            .map(Some)
            .map_err(|e| Error::Json {
                file: path.to_owned(),
                message: e.to_string(),
            }),
    }
}

/// `dir/000001-name.json`, the name made safe for a path. The number keeps
/// the order; the name inside the file is the real one.
fn numbered(dir: &str, index: usize, name: &str) -> String {
    let mut safe = String::new();
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => safe.push(b as char),
            _ => safe.push_str(&format!("%{b:02X}")),
        }
    }
    format!("{dir}{index:06}-{safe}.json")
}

struct Writer<'a> {
    m: &'a Model,
    bindings: HashMap<BindingId, usize>,
}

fn name_of<T>(table: &Table<T>, id: Id<T>) -> Result<String, Error> {
    table
        .name(id)
        .map(str::to_owned)
        .ok_or_else(|| Error::Invalid("a reference points at a removed element".to_owned()))
}

impl Writer<'_> {
    fn parameter(&self, id: Id<Parameter>) -> Result<String, Error> {
        name_of(self.m.parameters(), id)
    }

    fn part(&self, id: Id<Part>) -> Result<String, Error> {
        name_of(self.m.parts(), id)
    }

    fn deformer(&self, r: Option<DeformerRef>) -> Result<Option<DeformerRefDto>, Error> {
        Ok(match r {
            None => None,
            Some(DeformerRef::Warp(id)) => Some(DeformerRefDto::Warp {
                name: name_of(self.m.warps(), id)?,
            }),
            Some(DeformerRef::Rotation(id)) => Some(DeformerRefDto::Rotation {
                name: name_of(self.m.rotations(), id)?,
            }),
        })
    }

    fn grid<T, U>(
        &self,
        grid: &Option<KeyformGrid<T>>,
        f: impl Fn(&T) -> U,
    ) -> Result<Option<GridDto<U>>, Error> {
        let Some(grid) = grid else { return Ok(None) };
        let binding = *self
            .bindings
            .get(&grid.binding)
            .ok_or_else(|| Error::Invalid("a keyform grid has no binding".to_owned()))?;
        Ok(Some(GridDto {
            binding,
            keyforms: grid.keyforms.iter().map(f).collect(),
        }))
    }

    fn model_file(&self) -> Result<ModelFile, Error> {
        let m = self.m;
        let mut f = ModelFile::default();
        for (_, name, p) in m.parameters().iter() {
            f.parameters.push(ParameterDto {
                name: name.to_owned(),
                display_name: p.display_name.clone(),
                min: p.min,
                max: p.max,
                default: p.default,
                repeat: p.repeat,
                group: p.group.clone(),
            });
        }
        for (_, name, p) in m.parts().iter() {
            f.parts.push(PartDto {
                name: name.to_owned(),
                parent: p.parent.map(|id| self.part(id)).transpose()?,
                opacity: p.opacity,
                visible: p.visible,
            });
        }
        for (_, b) in m.bindings().iter() {
            let mut axes = Vec::new();
            for a in b.axes() {
                axes.push(AxisDto {
                    parameter: self.parameter(a.parameter)?,
                    keys: a.keys.clone(),
                });
            }
            f.bindings.push(BindingDto { axes });
        }
        for (_, name, b) in m.bones().iter() {
            f.bones.push(BoneDto {
                name: name.to_owned(),
                parent: b.parent.map(|id| name_of(m.bones(), id)).transpose()?,
                base: bone_dto(&b.base),
                keyforms: self.grid(&b.keyforms, bone_dto)?,
            });
        }
        for (_, name, w) in m.warps().iter() {
            f.warps.push(WarpDto {
                name: name.to_owned(),
                part: self.part(w.part)?,
                parent: self.deformer(w.parent)?,
                rows: w.rows,
                columns: w.columns,
                bezier: w.bezier,
                base_points: w.base_points.clone(),
                keyforms: self.grid(&w.keyforms, |k| WarpKeyformDto {
                    points: k.points.clone(),
                    opacity: k.opacity,
                })?,
            });
        }
        for (_, name, r) in m.rotations().iter() {
            f.rotations.push(RotationDto {
                name: name.to_owned(),
                part: self.part(r.part)?,
                parent: self.deformer(r.parent)?,
                base: rotation_dto(&r.base),
                keyforms: self.grid(&r.keyforms, rotation_dto)?,
            });
        }
        for (_, name, mesh) in m.meshes().iter() {
            let mut influences = Vec::new();
            for inf in &mesh.influences {
                let mut bones: [Option<String>; 4] = Default::default();
                for (slot, bone) in inf.bones.iter().enumerate() {
                    bones[slot] = bone.map(|id| name_of(m.bones(), id)).transpose()?;
                }
                influences.push(InfluenceDto {
                    bones,
                    weights: inf.weights,
                });
            }
            let mut masks = Vec::new();
            for &id in &mesh.masks {
                masks.push(name_of(m.meshes(), id)?);
            }
            f.meshes.push(MeshDto {
                name: name.to_owned(),
                part: self.part(mesh.part)?,
                parent: self.deformer(mesh.parent)?,
                texture: mesh.texture,
                positions: mesh.positions.clone(),
                uvs: mesh.uvs.clone(),
                indices: mesh.indices.clone(),
                blend_mode: blend_mode_dto(mesh.blend_mode),
                masks,
                mask_inverted: mesh.mask_inverted,
                opacity: mesh.opacity,
                draw_order: mesh.draw_order,
                multiply_color: mesh.multiply_color,
                screen_color: mesh.screen_color,
                double_sided: mesh.double_sided,
                visible: mesh.visible,
                influences,
                keyforms: self.grid(&mesh.keyforms, |k| MeshKeyformDto {
                    positions: k.positions.clone(),
                    opacity: k.opacity,
                    draw_order: k.draw_order,
                    multiply_color: k.multiply_color,
                    screen_color: k.screen_color,
                })?,
            });
        }
        for (_, name, g) in m.glues().iter() {
            f.glues.push(GlueDto {
                name: name.to_owned(),
                mesh_a: name_of(m.meshes(), g.mesh_a)?,
                mesh_b: name_of(m.meshes(), g.mesh_b)?,
                nodes: g
                    .nodes
                    .iter()
                    .map(|n| GlueNodeDto {
                        index_a: n.index_a,
                        index_b: n.index_b,
                        weight: n.weight,
                    })
                    .collect(),
                intensity: g.intensity,
                keyforms: self.grid(&g.keyforms, |v| *v)?,
            });
        }
        for (_, name, s) in m.blend_shapes().iter() {
            f.blend_shapes.push(BlendShapeDto {
                name: name.to_owned(),
                target: match s.target {
                    BlendTarget::Mesh(id) => BlendTargetDto::Mesh {
                        name: name_of(m.meshes(), id)?,
                    },
                    BlendTarget::Warp(id) => BlendTargetDto::Warp {
                        name: name_of(m.warps(), id)?,
                    },
                },
                parameter: self.parameter(s.parameter)?,
                keys: s.keys.clone(),
                weights: s.weights.clone(),
                deltas: s.deltas.clone(),
            });
        }
        for (_, name, d) in m.drivers().iter() {
            f.drivers.push(DriverDto {
                name: name.to_owned(),
                output: self.parameter(d.output)?,
                expression: d.expression.clone(),
            });
        }
        if let Some(pose) = m.pose() {
            let mut groups = Vec::new();
            for group in &pose.groups {
                let mut entries = Vec::new();
                for e in group {
                    let mut linked = Vec::new();
                    for &id in &e.linked {
                        linked.push(self.part(id)?);
                    }
                    entries.push(PoseEntryDto {
                        part: self.part(e.part)?,
                        linked,
                    });
                }
                groups.push(entries);
            }
            f.pose = Some(PoseDto {
                fade_in: pose.fade_in,
                groups,
            });
        }
        Ok(f)
    }

    fn physics_io(
        &self,
        inputs: &[PhysicsInput],
        outputs: &[PhysicsOutput],
    ) -> Result<(Vec<PhysicsInputDto>, Vec<PhysicsOutputDto>), Error> {
        let mut i = Vec::new();
        for x in inputs {
            i.push(PhysicsInputDto {
                parameter: self.parameter(x.parameter)?,
                channel: channel_dto(x.channel),
                weight: x.weight,
                reflect: x.reflect,
            });
        }
        let mut o = Vec::new();
        for x in outputs {
            o.push(PhysicsOutputDto {
                parameter: self.parameter(x.parameter)?,
                channel: channel_dto(x.channel),
                particle: x.particle,
                scale: x.scale,
                weight: x.weight,
                reflect: x.reflect,
            });
        }
        Ok((i, o))
    }

    fn physics(&self) -> Result<PhysicsFile, Error> {
        let rig = self.m.physics();
        let mut f = PhysicsFile::default();
        for g in &rig.pendulums {
            let (inputs, outputs) = self.physics_io(&g.inputs, &g.outputs)?;
            f.pendulums.push(PendulumDto {
                name: g.name.clone(),
                inputs,
                outputs,
                particles: g
                    .particles
                    .iter()
                    .map(|p| ParticleDto {
                        position: p.position,
                        mobility: p.mobility,
                        delay: p.delay,
                        acceleration: p.acceleration,
                        radius: p.radius,
                    })
                    .collect(),
                position_range: range_dto(g.position_range),
                angle_range: range_dto(g.angle_range),
            });
        }
        for c in &rig.chains {
            let (inputs, outputs) = self.physics_io(&c.inputs, &c.outputs)?;
            f.chains.push(ChainDto {
                name: c.name.clone(),
                joints: c
                    .joints
                    .iter()
                    .map(|j| ChainJointDto {
                        length: j.length,
                        stiffness: j.stiffness,
                        damping: j.damping,
                    })
                    .collect(),
                gravity: c.gravity,
                wind: c.wind,
                inputs,
                outputs,
            });
        }
        Ok(f)
    }

    fn motion(&self, name: &str, m: &Motion) -> Result<MotionFile, Error> {
        let mut curves = Vec::new();
        for c in &m.curves {
            curves.push(CurveDto {
                target: match c.target {
                    CurveTarget::Parameter(id) => CurveTargetDto::Parameter {
                        name: self.parameter(id)?,
                    },
                    CurveTarget::PartOpacity(id) => CurveTargetDto::PartOpacity {
                        name: self.part(id)?,
                    },
                },
                start: c.start,
                segments: c
                    .segments
                    .iter()
                    .map(|s| match *s {
                        Segment::Linear(point) => SegmentDto::Linear { point },
                        Segment::Bezier(points) => SegmentDto::Bezier { points },
                        Segment::Stepped(point) => SegmentDto::Stepped { point },
                        Segment::InverseStepped(point) => SegmentDto::InverseStepped { point },
                    })
                    .collect(),
                fade_in: c.fade_in,
                fade_out: c.fade_out,
            });
        }
        Ok(MotionFile {
            name: name.to_owned(),
            duration: m.duration,
            fps: m.fps,
            looped: m.looped,
            fade_in: m.fade_in,
            fade_out: m.fade_out,
            curves,
        })
    }

    fn expression(&self, name: &str, e: &Expression) -> Result<ExpressionFile, Error> {
        let mut parameters = Vec::new();
        for p in &e.parameters {
            parameters.push(ExpressionParameterDto {
                parameter: self.parameter(p.parameter)?,
                blend: match p.blend {
                    ExpressionBlend::Add => ExpressionBlendDto::Add,
                    ExpressionBlend::Multiply => ExpressionBlendDto::Multiply,
                    ExpressionBlend::Overwrite => ExpressionBlendDto::Overwrite,
                },
                value: p.value,
            });
        }
        Ok(ExpressionFile {
            name: name.to_owned(),
            fade_in: e.fade_in,
            fade_out: e.fade_out,
            parameters,
        })
    }
}

fn bone_dto(b: &BoneTransform) -> BoneTransformDto {
    BoneTransformDto {
        translation: b.translation,
        rotation_degrees: b.rotation_degrees,
        scale: b.scale,
    }
}

fn rotation_dto(r: &RotationKeyform) -> RotationKeyformDto {
    RotationKeyformDto {
        origin: r.origin,
        angle_degrees: r.angle_degrees,
        scale: r.scale,
        reflect_x: r.reflect_x,
        reflect_y: r.reflect_y,
        opacity: r.opacity,
    }
}

fn blend_mode_dto(b: BlendMode) -> BlendModeDto {
    match b {
        BlendMode::Normal => BlendModeDto::Normal,
        BlendMode::Additive => BlendModeDto::Additive,
        BlendMode::Multiply => BlendModeDto::Multiply,
    }
}

fn channel_dto(c: Channel) -> ChannelDto {
    match c {
        Channel::X => ChannelDto::X,
        Channel::Y => ChannelDto::Y,
        Channel::Angle => ChannelDto::Angle,
    }
}

fn range_dto(r: Range) -> RangeDto {
    RangeDto {
        min: r.min,
        max: r.max,
        default: r.default,
    }
}

fn find<T>(table: &Table<T>, name: &str, owner: &str, field: &'static str) -> Result<Id<T>, Error> {
    table.find(name).ok_or_else(|| Error::Reference {
        owner: owner.to_owned(),
        field,
        name: name.to_owned(),
    })
}

fn insert<T>(table: &mut Table<T>, kind: &str, name: &str, value: T) -> Result<Id<T>, Error> {
    table
        .insert(name, value)
        .map_err(|e| Error::Invalid(format!("{kind} '{name}': {e:?}")))
}

#[derive(Default)]
struct Reader {
    p: ModelParts,
    bindings: Vec<BindingId>,
}

impl Reader {
    fn deformer(
        &self,
        r: &Option<DeformerRefDto>,
        owner: &str,
    ) -> Result<Option<DeformerRef>, Error> {
        Ok(match r {
            None => None,
            Some(DeformerRefDto::Warp { name }) => Some(DeformerRef::Warp(find(
                &self.p.warps,
                name,
                owner,
                "parent",
            )?)),
            Some(DeformerRefDto::Rotation { name }) => Some(DeformerRef::Rotation(find(
                &self.p.rotations,
                name,
                owner,
                "parent",
            )?)),
        })
    }

    fn grid<T, U>(
        &self,
        grid: &Option<GridDto<T>>,
        owner: &str,
        f: impl Fn(&T) -> U,
    ) -> Result<Option<KeyformGrid<U>>, Error> {
        let Some(grid) = grid else { return Ok(None) };
        let binding = *self.bindings.get(grid.binding).ok_or_else(|| {
            Error::Invalid(format!("{owner}: binding {} does not exist", grid.binding))
        })?;
        Ok(Some(KeyformGrid::new(
            binding,
            grid.keyforms.iter().map(f).collect(),
        )))
    }

    fn build(
        mut self,
        f: &ModelFile,
        physics: &PhysicsFile,
        motions: &[MotionFile],
        expressions: &[ExpressionFile],
    ) -> Result<Model, Error> {
        for d in &f.parameters {
            let p = Parameter {
                display_name: d.display_name.clone(),
                min: d.min,
                max: d.max,
                default: d.default,
                repeat: d.repeat,
                group: d.group.clone(),
            };
            insert(&mut self.p.parameters, "parameter", &d.name, p)?;
        }

        let mut part_ids = Vec::new();
        for d in &f.parts {
            let part = Part {
                parent: None,
                opacity: d.opacity,
                visible: d.visible,
            };
            part_ids.push(insert(&mut self.p.parts, "part", &d.name, part)?);
        }
        for (d, &id) in f.parts.iter().zip(&part_ids) {
            if let Some(parent) = &d.parent {
                let owner = format!("part '{}'", d.name);
                let parent = find(&self.p.parts, parent, &owner, "parent")?;
                if let Some(part) = self.p.parts.get_mut(id) {
                    part.parent = Some(parent);
                }
            }
        }

        for (i, d) in f.bindings.iter().enumerate() {
            let owner = format!("binding {i}");
            let mut axes = Vec::new();
            for a in &d.axes {
                axes.push(Axis {
                    parameter: find(&self.p.parameters, &a.parameter, &owner, "parameter")?,
                    keys: a.keys.clone(),
                });
            }
            let binding =
                Binding::new(axes).map_err(|e| Error::Invalid(format!("{owner}: {e:?}")))?;
            self.bindings.push(self.p.bindings.insert(binding));
        }

        let mut bone_ids = Vec::new();
        for d in &f.bones {
            let owner = format!("bone '{}'", d.name);
            let bone = Bone {
                parent: None,
                base: bone_transform(&d.base),
                keyforms: self.grid(&d.keyforms, &owner, bone_transform)?,
            };
            bone_ids.push(insert(&mut self.p.bones, "bone", &d.name, bone)?);
        }
        for (d, &id) in f.bones.iter().zip(&bone_ids) {
            if let Some(parent) = &d.parent {
                let owner = format!("bone '{}'", d.name);
                let parent = find(&self.p.bones, parent, &owner, "parent")?;
                if let Some(bone) = self.p.bones.get_mut(id) {
                    bone.parent = Some(parent);
                }
            }
        }

        let mut warp_ids = Vec::new();
        for d in &f.warps {
            let owner = format!("warp '{}'", d.name);
            let warp = WarpDeformer {
                part: find(&self.p.parts, &d.part, &owner, "part")?,
                parent: None,
                rows: d.rows,
                columns: d.columns,
                bezier: d.bezier,
                base_points: d.base_points.clone(),
                keyforms: self.grid(&d.keyforms, &owner, |k| WarpKeyform {
                    points: k.points.clone(),
                    opacity: k.opacity,
                })?,
            };
            warp_ids.push(insert(&mut self.p.warps, "warp", &d.name, warp)?);
        }
        let mut rotation_ids = Vec::new();
        for d in &f.rotations {
            let owner = format!("rotation '{}'", d.name);
            let rotation = RotationDeformer {
                part: find(&self.p.parts, &d.part, &owner, "part")?,
                parent: None,
                base: rotation_keyform(&d.base),
                keyforms: self.grid(&d.keyforms, &owner, rotation_keyform)?,
            };
            rotation_ids.push(insert(
                &mut self.p.rotations,
                "rotation",
                &d.name,
                rotation,
            )?);
        }
        for (d, &id) in f.warps.iter().zip(&warp_ids) {
            let parent = self.deformer(&d.parent, &format!("warp '{}'", d.name))?;
            if let Some(warp) = self.p.warps.get_mut(id) {
                warp.parent = parent;
            }
        }
        for (d, &id) in f.rotations.iter().zip(&rotation_ids) {
            let parent = self.deformer(&d.parent, &format!("rotation '{}'", d.name))?;
            if let Some(rotation) = self.p.rotations.get_mut(id) {
                rotation.parent = parent;
            }
        }

        let mut mesh_ids = Vec::new();
        for d in &f.meshes {
            let owner = format!("mesh '{}'", d.name);
            let mut influences = Vec::new();
            for inf in &d.influences {
                let mut bones = [None; 4];
                for (slot, name) in inf.bones.iter().enumerate() {
                    if let Some(name) = name {
                        bones[slot] = Some(find(&self.p.bones, name, &owner, "influences")?);
                    }
                }
                influences.push(VertexInfluences {
                    bones,
                    weights: inf.weights,
                });
            }
            let mut mesh = ArtMesh::new(
                find(&self.p.parts, &d.part, &owner, "part")?,
                d.positions.clone(),
                d.uvs.clone(),
                d.indices.clone(),
            );
            mesh.parent = self.deformer(&d.parent, &owner)?;
            mesh.texture = d.texture;
            mesh.blend_mode = blend_mode(d.blend_mode);
            mesh.mask_inverted = d.mask_inverted;
            mesh.opacity = d.opacity;
            mesh.draw_order = d.draw_order;
            mesh.multiply_color = d.multiply_color;
            mesh.screen_color = d.screen_color;
            mesh.double_sided = d.double_sided;
            mesh.visible = d.visible;
            mesh.influences = influences;
            mesh.keyforms = self.grid(&d.keyforms, &owner, |k| MeshKeyform {
                positions: k.positions.clone(),
                opacity: k.opacity,
                draw_order: k.draw_order,
                multiply_color: k.multiply_color,
                screen_color: k.screen_color,
            })?;
            mesh_ids.push(insert(&mut self.p.meshes, "mesh", &d.name, mesh)?);
        }
        for (d, &id) in f.meshes.iter().zip(&mesh_ids) {
            let owner = format!("mesh '{}'", d.name);
            let mut masks = Vec::new();
            for name in &d.masks {
                masks.push(find(&self.p.meshes, name, &owner, "masks")?);
            }
            if let Some(mesh) = self.p.meshes.get_mut(id) {
                mesh.masks = masks;
            }
        }

        for d in &f.glues {
            let owner = format!("glue '{}'", d.name);
            let glue = Glue {
                mesh_a: find(&self.p.meshes, &d.mesh_a, &owner, "mesh_a")?,
                mesh_b: find(&self.p.meshes, &d.mesh_b, &owner, "mesh_b")?,
                nodes: d
                    .nodes
                    .iter()
                    .map(|n| GlueNode {
                        index_a: n.index_a,
                        index_b: n.index_b,
                        weight: n.weight,
                    })
                    .collect(),
                intensity: d.intensity,
                keyforms: self.grid(&d.keyforms, &owner, |v| *v)?,
            };
            insert(&mut self.p.glues, "glue", &d.name, glue)?;
        }

        for d in &f.blend_shapes {
            let owner = format!("blend shape '{}'", d.name);
            let target = match &d.target {
                BlendTargetDto::Mesh { name } => {
                    BlendTarget::Mesh(find(&self.p.meshes, name, &owner, "target")?)
                }
                BlendTargetDto::Warp { name } => {
                    BlendTarget::Warp(find(&self.p.warps, name, &owner, "target")?)
                }
            };
            let shape = BlendShape {
                target,
                parameter: find(&self.p.parameters, &d.parameter, &owner, "parameter")?,
                keys: d.keys.clone(),
                weights: d.weights.clone(),
                deltas: d.deltas.clone(),
            };
            insert(&mut self.p.blend_shapes, "blend shape", &d.name, shape)?;
        }

        for d in &f.drivers {
            let owner = format!("driver '{}'", d.name);
            let driver = Driver {
                output: find(&self.p.parameters, &d.output, &owner, "output")?,
                expression: d.expression.clone(),
            };
            insert(&mut self.p.drivers, "driver", &d.name, driver)?;
        }

        for d in motions {
            let owner = format!("motion '{}'", d.name);
            let mut curves = Vec::new();
            for c in &d.curves {
                curves.push(Curve {
                    target: match &c.target {
                        CurveTargetDto::Parameter { name } => CurveTarget::Parameter(find(
                            &self.p.parameters,
                            name,
                            &owner,
                            "curves",
                        )?),
                        CurveTargetDto::PartOpacity { name } => {
                            CurveTarget::PartOpacity(find(&self.p.parts, name, &owner, "curves")?)
                        }
                    },
                    start: c.start,
                    segments: c
                        .segments
                        .iter()
                        .map(|s| match *s {
                            SegmentDto::Linear { point } => Segment::Linear(point),
                            SegmentDto::Bezier { points } => Segment::Bezier(points),
                            SegmentDto::Stepped { point } => Segment::Stepped(point),
                            SegmentDto::InverseStepped { point } => Segment::InverseStepped(point),
                        })
                        .collect(),
                    fade_in: c.fade_in,
                    fade_out: c.fade_out,
                });
            }
            let motion = Motion {
                duration: d.duration,
                fps: d.fps,
                looped: d.looped,
                fade_in: d.fade_in,
                fade_out: d.fade_out,
                curves,
            };
            insert(&mut self.p.motions, "motion", &d.name, motion)?;
        }

        for d in expressions {
            let owner = format!("expression '{}'", d.name);
            let mut parameters = Vec::new();
            for p in &d.parameters {
                parameters.push(ExpressionParameter {
                    parameter: find(&self.p.parameters, &p.parameter, &owner, "parameters")?,
                    blend: match p.blend {
                        ExpressionBlendDto::Add => ExpressionBlend::Add,
                        ExpressionBlendDto::Multiply => ExpressionBlend::Multiply,
                        ExpressionBlendDto::Overwrite => ExpressionBlend::Overwrite,
                    },
                    value: p.value,
                });
            }
            let expression = Expression {
                fade_in: d.fade_in,
                fade_out: d.fade_out,
                parameters,
            };
            insert(&mut self.p.expressions, "expression", &d.name, expression)?;
        }

        if let Some(d) = &f.pose {
            let mut groups = Vec::new();
            for group in &d.groups {
                let mut entries = Vec::new();
                for e in group {
                    let mut linked = Vec::new();
                    for name in &e.linked {
                        linked.push(find(&self.p.parts, name, "pose", "linked")?);
                    }
                    entries.push(PoseEntry {
                        part: find(&self.p.parts, &e.part, "pose", "part")?,
                        linked,
                    });
                }
                groups.push(entries);
            }
            self.p.pose = Some(Pose {
                fade_in: d.fade_in,
                groups,
            });
        }

        self.p.physics = self.physics(physics)?;

        Model::from_parts(self.p).map_err(|errors| Error::Invalid(format!("{errors:?}")))
    }

    fn physics_io(
        &self,
        owner: &str,
        inputs: &[PhysicsInputDto],
        outputs: &[PhysicsOutputDto],
    ) -> Result<(Vec<PhysicsInput>, Vec<PhysicsOutput>), Error> {
        let mut i = Vec::new();
        for x in inputs {
            i.push(PhysicsInput {
                parameter: find(&self.p.parameters, &x.parameter, owner, "inputs")?,
                channel: channel(x.channel),
                weight: x.weight,
                reflect: x.reflect,
            });
        }
        let mut o = Vec::new();
        for x in outputs {
            o.push(PhysicsOutput {
                parameter: find(&self.p.parameters, &x.parameter, owner, "outputs")?,
                channel: channel(x.channel),
                particle: x.particle,
                scale: x.scale,
                weight: x.weight,
                reflect: x.reflect,
            });
        }
        Ok((i, o))
    }

    fn physics(&self, f: &PhysicsFile) -> Result<PhysicsRig, Error> {
        let mut rig = PhysicsRig::default();
        for g in &f.pendulums {
            let (inputs, outputs) =
                self.physics_io(&format!("pendulum '{}'", g.name), &g.inputs, &g.outputs)?;
            rig.pendulums.push(PendulumGroup {
                name: g.name.clone(),
                inputs,
                outputs,
                particles: g
                    .particles
                    .iter()
                    .map(|p| Particle {
                        position: p.position,
                        mobility: p.mobility,
                        delay: p.delay,
                        acceleration: p.acceleration,
                        radius: p.radius,
                    })
                    .collect(),
                position_range: range(g.position_range),
                angle_range: range(g.angle_range),
            });
        }
        for c in &f.chains {
            let (inputs, outputs) = self.physics_io(
                &format!("physics chain '{}'", c.name),
                &c.inputs,
                &c.outputs,
            )?;
            rig.chains.push(PhysicsChain {
                name: c.name.clone(),
                joints: c
                    .joints
                    .iter()
                    .map(|j| ChainJoint {
                        length: j.length,
                        stiffness: j.stiffness,
                        damping: j.damping,
                    })
                    .collect(),
                gravity: c.gravity,
                wind: c.wind,
                inputs,
                outputs,
            });
        }
        Ok(rig)
    }
}

fn bone_transform(b: &BoneTransformDto) -> BoneTransform {
    BoneTransform {
        translation: b.translation,
        rotation_degrees: b.rotation_degrees,
        scale: b.scale,
    }
}

fn rotation_keyform(r: &RotationKeyformDto) -> RotationKeyform {
    RotationKeyform {
        origin: r.origin,
        angle_degrees: r.angle_degrees,
        scale: r.scale,
        reflect_x: r.reflect_x,
        reflect_y: r.reflect_y,
        opacity: r.opacity,
    }
}

fn blend_mode(b: BlendModeDto) -> BlendMode {
    match b {
        BlendModeDto::Normal => BlendMode::Normal,
        BlendModeDto::Additive => BlendMode::Additive,
        BlendModeDto::Multiply => BlendMode::Multiply,
    }
}

fn channel(c: ChannelDto) -> Channel {
    match c {
        ChannelDto::X => Channel::X,
        ChannelDto::Y => Channel::Y,
        ChannelDto::Angle => Channel::Angle,
    }
}

fn range(r: RangeDto) -> Range {
    Range {
        min: r.min,
        max: r.max,
        default: r.default,
    }
}
