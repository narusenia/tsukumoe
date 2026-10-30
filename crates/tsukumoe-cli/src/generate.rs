// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Builds the reference model (ADR-0017) from a parts table. The shapes are
//! geometric: the point is the size and the coverage of features, not looks.
//!
//! Per group: one part, one rotation deformer, `warps` warp deformers (the
//! first under the rotation, the rest under the first), `meshes` meshes of
//! `grid` x `grid` vertices spread over the remaining warps, and `chains`
//! physics chains (at most 2). Parameters `0` and `1` of a group drive the
//! warps and the rotation, `2` and `3` are physics outputs, and the others
//! pair with parameter `0` in the meshes' keyforms.

use serde::Deserialize;
use tsukumoe_core::elements::{Parameter, Part, Point};
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::physics::{ChainJoint, Channel, PhysicsChain, PhysicsInput, PhysicsOutput};
use tsukumoe_core::rig::{
    ArtMesh, DeformerRef, MeshKeyform, RotationDeformer, RotationKeyform, WarpDeformer, WarpKeyform,
};
use tsukumoe_format::{Manifest, Project, write_model};
use tsukumoe_gpu::Image;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub name: String,
    /// Vertices per side of every mesh.
    pub grid: usize,
    pub groups: Vec<Group>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    pub meshes: usize,
    pub warps: usize,
    pub parameters: usize,
    pub chains: usize,
}

const CELL: f32 = 2.0;
const COLUMNS: usize = 5;
const TEXTURE: u32 = 64;

fn check(table: &Table) -> Result<(), String> {
    if !(2..=64).contains(&table.grid) {
        return Err("grid must be between 2 and 64".into());
    }
    if table.groups.is_empty() || table.groups.len() > 99 {
        return Err("a table needs 1 to 99 groups".into());
    }
    for g in &table.groups {
        if g.name.is_empty() || !g.name.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!(
                "group name {:?} must be ASCII letters and digits",
                g.name
            ));
        }
        if g.meshes == 0 || g.meshes > 15 {
            return Err(format!(
                "{}: meshes must be 1 to 15 (a 5 x 3 layout)",
                g.name
            ));
        }
        if g.warps < 2 || g.parameters < 5 || g.chains > 2 {
            return Err(format!(
                "{}: needs warps >= 2, parameters >= 5, chains <= 2",
                g.name
            ));
        }
    }
    let mut names: Vec<_> = table.groups.iter().map(|g| &g.name).collect();
    names.sort();
    names.dedup();
    if names.len() != table.groups.len() {
        return Err("group names must be unique".into());
    }
    Ok(())
}

fn grid_points(side: usize, origin: Point, size: f32, scale: f32) -> Vec<Point> {
    let n = (side - 1) as f32;
    let center = [origin[0] + size / 2.0, origin[1] + size / 2.0];
    (0..side * side)
        .map(|i| {
            let p = [
                origin[0] + (i % side) as f32 / n * size,
                origin[1] + (i / side) as f32 / n * size,
            ];
            [
                center[0] + (p[0] - center[0]) * scale,
                center[1] + (p[1] - center[1]) * scale,
            ]
        })
        .collect()
}

fn uvs(side: usize) -> Vec<Point> {
    let n = (side - 1) as f32;
    (0..side * side)
        .map(|i| [(i % side) as f32 / n, (i / side) as f32 / n])
        .collect()
}

fn triangles(side: usize) -> Vec<u32> {
    let s = side as u32;
    let mut out = Vec::new();
    for y in 0..s - 1 {
        for x in 0..s - 1 {
            let i = y * s + x;
            out.extend([i, i + 1, i + s + 1, i, i + s + 1, i + s]);
        }
    }
    out
}

pub fn model(table: &Table) -> Result<Model, String> {
    check(table)?;
    let side = table.grid;
    let mut p = ModelParts::default();
    for (gi, g) in table.groups.iter().enumerate() {
        let gx = (gi % COLUMNS) as f32 * CELL;
        let gy = (gi / COLUMNS) as f32 * CELL;
        let err = |e: &dyn std::fmt::Debug| format!("{}: {e:?}", g.name);
        let part = p
            .parts
            .insert(&g.name, Part::default())
            .map_err(|e| err(&e))?;
        let params: Vec<_> = (0..g.parameters)
            .map(|i| {
                p.parameters
                    .insert(format!("Param{}{i}", g.name), Parameter::new(0.0, 1.0, 0.0))
                    .map_err(|e| err(&e))
            })
            .collect::<Result<_, _>>()?;
        let axis = |parameter| Axis {
            parameter,
            keys: vec![0.0, 1.0],
        };
        let one_axis = |p: &mut ModelParts, parameter| {
            p.bindings
                .insert(Binding::new(vec![axis(parameter)]).expect("one ascending axis"))
        };

        let center = [gx + CELL / 2.0, gy + CELL / 2.0];
        let rot_keyform = |angle: f32| RotationKeyform {
            origin: center,
            angle_degrees: angle,
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
            opacity: 1.0,
        };
        let rotation_binding = one_axis(&mut p, params[1]);
        let rotation = p
            .rotations
            .insert(
                format!("{}Rotation", g.name),
                RotationDeformer {
                    part,
                    parent: None,
                    base: rot_keyform(0.0),
                    keyforms: Some(KeyformGrid::new(
                        rotation_binding,
                        vec![rot_keyform(0.0), rot_keyform(10.0)],
                    )),
                },
            )
            .map_err(|e| err(&e))?;

        let warp_binding = one_axis(&mut p, params[0]);
        let warp_points = |scale: f32| grid_points(3, [gx, gy], CELL, scale);
        let mut warps = Vec::new();
        for k in 0..g.warps {
            let parent = match warps.first() {
                None => DeformerRef::Rotation(rotation),
                Some(&first) => DeformerRef::Warp(first),
            };
            let keyform = |scale| WarpKeyform {
                points: warp_points(scale),
                opacity: 1.0,
            };
            let id = p
                .warps
                .insert(
                    format!("{}Warp{k}", g.name),
                    WarpDeformer {
                        part,
                        parent: Some(parent),
                        rows: 2,
                        columns: 2,
                        bezier: true,
                        base_points: warp_points(1.0),
                        keyforms: Some(KeyformGrid::new(
                            warp_binding,
                            vec![keyform(1.0), keyform(1.0 + 0.02 * (k + 1) as f32)],
                        )),
                    },
                )
                .map_err(|e| err(&e))?;
            warps.push(id);
        }

        let mesh_size = CELL / COLUMNS as f32 * 0.9;
        for j in 0..g.meshes {
            let origin = [
                gx + (j % COLUMNS) as f32 * CELL / COLUMNS as f32,
                gy + (j / COLUMNS) as f32 * CELL / COLUMNS as f32,
            ];
            let second = params[4 + j % (g.parameters - 4)];
            let binding = p.bindings.insert(
                Binding::new(vec![axis(params[0]), axis(second)]).expect("two ascending axes"),
            );
            let keyform = |scale: f32| MeshKeyform {
                positions: grid_points(side, origin, mesh_size, scale),
                opacity: 1.0,
                draw_order: 500.0,
                multiply_color: [1.0; 3],
                screen_color: [0.0; 3],
            };
            let mut mesh = ArtMesh::new(
                part,
                grid_points(side, origin, mesh_size, 1.0),
                uvs(side),
                triangles(side),
            );
            mesh.parent = Some(DeformerRef::Warp(warps[1 + j % (g.warps - 1)]));
            mesh.texture = gi as u32;
            mesh.draw_order = (j % 7) as f32 + 500.0;
            // Keys are ordered with the first axis varying slowest.
            mesh.keyforms = Some(KeyformGrid::new(
                binding,
                vec![keyform(1.0), keyform(1.03), keyform(0.97), keyform(1.06)],
            ));
            p.meshes
                .insert(format!("{}Mesh{j}", g.name), mesh)
                .map_err(|e| err(&e))?;
        }

        for c in 0..g.chains {
            p.physics.chains.push(PhysicsChain {
                name: format!("{}Chain{c}", g.name),
                joints: vec![
                    ChainJoint {
                        length: 0.3,
                        stiffness: 0.5,
                        damping: 0.1
                    };
                    3
                ],
                gravity: [0.0, -9.8],
                wind: [0.0, 0.0],
                inputs: vec![PhysicsInput {
                    parameter: params[0],
                    channel: Channel::X,
                    weight: 1.0,
                    reflect: false,
                }],
                outputs: vec![PhysicsOutput {
                    parameter: params[2 + c],
                    channel: Channel::X,
                    particle: 1,
                    scale: 1.0,
                    weight: 1.0,
                    reflect: false,
                }],
            });
        }
    }
    Model::from_parts(p).map_err(|e| format!("generated model is inconsistent: {e:?}"))
}

/// One gradient texture per group, its hue set by the group's position.
fn texture(group: usize, groups: usize) -> Vec<u8> {
    let hue = group as f32 / groups as f32;
    let channel = |offset: f32| (hue + offset).fract() * 0.5 + 0.5;
    let color = [channel(0.0), channel(0.33), channel(0.66)];
    let mut rgba = Vec::with_capacity((TEXTURE * TEXTURE * 4) as usize);
    for y in 0..TEXTURE {
        for x in 0..TEXTURE {
            let shade = 0.6 + 0.4 * (x + y) as f32 / (2 * TEXTURE) as f32;
            rgba.extend(color.map(|c| (c * shade * 255.0) as u8));
            rgba.push(255);
        }
    }
    Image {
        width: TEXTURE,
        height: TEXTURE,
        rgba,
    }
    .to_png()
}

pub fn project(table: &Table) -> Result<(Project, Model), String> {
    let model = model(table)?;
    let mut project = Project::new(Manifest::new(Some(format!(
        "tsukumoe-cli {} ({})",
        env!("CARGO_PKG_VERSION"),
        table.name
    ))));
    write_model(&model, &mut project).map_err(|e| e.to_string())?;
    for gi in 0..table.groups.len() {
        // Two digits, so path order is texture order.
        project.files.insert(
            format!("textures/{gi:02}.png"),
            texture(gi, table.groups.len()),
        );
    }
    Ok((project, model))
}
