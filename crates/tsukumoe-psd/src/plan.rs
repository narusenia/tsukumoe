// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The import plan: a pure function of the scanned PSD, the model, the last
//! import's map and the user's choices. It touches no file; applying it is
//! one `Batch`, and the texture files are written afterwards.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};

use serde::Serialize;
use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Point;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::{ArtMesh, BlendMode};

use crate::map::{GroupRec, LayerRec, MAP_VERSION, PsdMap, Source, UNITS_PER_PIXEL};
use crate::reader::{Limits, Psd, PsdError};
use crate::scan::{Note, Scan, ScannedLayer, blend_mode};

/// The name of the `Part` that holds the imported layers.
pub const ROOT_PART: &str = "PSD";
pub const TEXTURE_PREFIX: &str = "textures/";

/// What the project already has.
pub struct Existing<'a> {
    pub model: &'a Model,
    /// Paths of the files under `textures/` (any order).
    pub textures: &'a [String],
}

/// The user's decisions about a reimport.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    /// Remove the meshes of vanished layers unless `missing` says otherwise.
    pub remove_missing: bool,
    /// Per vanished layer (old key): true removes the mesh, false keeps it.
    pub missing: BTreeMap<String, bool>,
    /// Old key of a vanished layer to the key of a new layer it was renamed to.
    pub rename: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    AddGroup,
    AddLayer {
        restored: bool,
    },
    /// The layer exists in the model. `image` is true when the texture is
    /// replaced; `set` lists the properties updated from the PSD and `kept`
    /// those that changed in the PSD but were edited in the project.
    UpdateLayer {
        image: bool,
        set: Vec<&'static str>,
        kept: Vec<&'static str>,
        renamed_from: Option<String>,
    },
    UpdateGroup {
        set: Vec<&'static str>,
        kept: Vec<&'static str>,
    },
    /// In the project from an earlier import, not in the PSD any more.
    Missing {
        remove: bool,
    },
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub key: String,
    #[serde(flatten)]
    pub action: Action,
}

/// A texture file to write after the batch applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextureJob {
    /// Index into [`Scan::layers`].
    pub layer: usize,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    pub items: Vec<Item>,
    pub notes: Vec<Note>,
    /// Applied as one `Batch`.
    pub commands: Vec<Command>,
    pub textures: Vec<TextureJob>,
    /// What to write to `sources/psd-map.json` once the batch applied.
    pub map: PsdMap,
}

impl Plan {
    /// Nothing to apply.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.textures.is_empty()
    }

    pub fn batch(&self) -> Command {
        Command::Batch {
            commands: self.commands.clone(),
        }
    }

    /// Candidates for a hand-made rename: keys of vanished layers, and of new ones.
    pub fn renamable(&self) -> (Vec<&str>, Vec<&str>) {
        let missing = self
            .items
            .iter()
            .filter(|i| matches!(i.action, Action::Missing { .. }))
            .map(|i| i.key.as_str())
            .collect();
        let added = self
            .items
            .iter()
            .filter(|i| matches!(i.action, Action::AddLayer { restored: false }))
            .map(|i| i.key.as_str())
            .collect();
        (missing, added)
    }
}

/// The paths of the texture files among a project's files.
pub fn texture_paths<'a>(files: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut v: Vec<String> = files
        .filter(|f| f.starts_with(TEXTURE_PREFIX) && f.ends_with(".png"))
        .map(str::to_owned)
        .collect();
    v.sort();
    v
}

fn texture_path(n: u32) -> String {
    format!("{TEXTURE_PREFIX}psd-{n:06}.png")
}

fn next_texture_number(existing: &[String], map: Option<&PsdMap>) -> u32 {
    existing
        .iter()
        .map(String::as_str)
        .chain(
            map.into_iter()
                .flat_map(|m| m.layers.iter().map(|l| l.texture.as_str())),
        )
        .filter_map(|p| {
            p.strip_prefix(TEXTURE_PREFIX)?
                .strip_prefix("psd-")?
                .strip_suffix(".png")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .map_or(1, |n| n + 1)
}

fn unique(taken: &mut BTreeSet<String>, in_model: impl Fn(&str) -> bool, base: &str) -> String {
    let mut name = base.to_owned();
    let mut n = 2;
    while taken.contains(&name) || in_model(&name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    taken.insert(name.clone());
    name
}

fn same(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn opacity_of(v: u8) -> f32 {
    f32::from(v) / 255.0
}

/// Canvas pixels to model units: x right, y up, the canvas's top left at the origin.
fn point(x: i32, y: i32, scale: f32) -> Point {
    [x as f32 * scale, -(y as f32) * scale]
}

fn quad(rect: [i32; 4], scale: f32) -> (Vec<Point>, Vec<Point>, Vec<u32>) {
    let [l, t, r, b] = rect;
    (
        vec![
            point(l, t, scale),
            point(r, t, scale),
            point(r, b, scale),
            point(l, b, scale),
        ],
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        // Counter-clockwise in the y-up drawing space.
        vec![0, 3, 2, 0, 2, 1],
    )
}

/// UVs for the same canvas pixels in a texture that now covers `new`.
pub fn remap_uvs(uvs: &[Point], old: [i32; 4], new: [i32; 4]) -> Vec<Point> {
    let (ow, oh) = ((old[2] - old[0]) as f32, (old[3] - old[1]) as f32);
    let (nw, nh) = ((new[2] - new[0]) as f32, (new[3] - new[1]) as f32);
    uvs.iter()
        .map(|&[u, v]| {
            let x = old[0] as f32 + u * ow;
            let y = old[1] as f32 + v * oh;
            [(x - new[0] as f32) / nw, (y - new[1] as f32) / nh]
        })
        .collect()
}

/// How a layer is resolved against the project.
enum Resolved<'a> {
    Add {
        mesh: String,
        restored: bool,
        rec: Option<&'a LayerRec>,
    },
    Existing {
        rec: &'a LayerRec,
        renamed_from: Option<String>,
    },
    Skip {
        rec: Option<&'a LayerRec>,
    },
}

/// Builds the plan. Pure: the same inputs give the same plan.
pub fn plan(
    scan: &Scan,
    existing: &Existing<'_>,
    map: Option<&PsdMap>,
    choices: &Choices,
    source: &Source,
) -> Plan {
    let model = existing.model;
    let scale = map.map_or(UNITS_PER_PIXEL, |m| m.units_per_pixel);
    let mut notes = scan.notes.clone();
    let mut items = Vec::new();
    let mut commands = Vec::new();
    let mut taken_parts = BTreeSet::new();
    let mut taken_meshes = BTreeSet::new();
    let part_in_model = |n: &str| model.parts().find(n).is_some();
    let mesh_in_model = |n: &str| model.meshes().find(n).is_some();

    // The root part.
    let root = match map.map(|m| m.root_part.as_str()) {
        Some(name) if part_in_model(name) => name.to_owned(),
        Some(name) => {
            taken_parts.insert(name.to_owned());
            commands.push(Command::AddPart {
                name: name.to_owned(),
                parent: None,
            });
            name.to_owned()
        }
        None => {
            let name = unique(&mut taken_parts, part_in_model, ROOT_PART);
            commands.push(Command::AddPart {
                name: name.clone(),
                parent: None,
            });
            name
        }
    };

    // Groups, parents first (the scan lists a group before its children).
    let mut group_part: Vec<String> = Vec::new();
    let mut group_recs = Vec::new();
    for g in &scan.groups {
        let parent = g.parent.map_or(root.clone(), |p| group_part[p].clone());
        let rec = map.and_then(|m| m.groups.iter().find(|r| r.key == g.key));
        let existing_part = rec.filter(|r| part_in_model(&r.part));
        let (part, action) = match existing_part {
            Some(r) => {
                let part = model
                    .parts()
                    .get(model.parts().find(&r.part).expect("checked"))
                    .expect("found");
                let (mut set, mut kept) = (Vec::new(), Vec::new());
                if g.opacity != r.opacity {
                    if same(part.opacity, opacity_of(r.opacity)) {
                        commands.push(Command::SetPartOpacity {
                            name: r.part.clone(),
                            opacity: opacity_of(g.opacity),
                        });
                        set.push("opacity");
                    } else {
                        kept.push("opacity");
                    }
                }
                if g.visible != r.visible {
                    if part.visible == r.visible {
                        commands.push(Command::SetPartVisible {
                            name: r.part.clone(),
                            visible: g.visible,
                        });
                        set.push("visible");
                    } else {
                        kept.push("visible");
                    }
                }
                let action = if set.is_empty() && kept.is_empty() {
                    Action::Unchanged
                } else {
                    Action::UpdateGroup { set, kept }
                };
                (r.part.clone(), action)
            }
            None => {
                let name = unique(&mut taken_parts, part_in_model, &g.key);
                commands.push(Command::AddPart {
                    name: name.clone(),
                    parent: Some(parent),
                });
                if g.opacity != 255 {
                    commands.push(Command::SetPartOpacity {
                        name: name.clone(),
                        opacity: opacity_of(g.opacity),
                    });
                }
                if !g.visible {
                    commands.push(Command::SetPartVisible {
                        name: name.clone(),
                        visible: false,
                    });
                }
                (name, Action::AddGroup)
            }
        };
        group_recs.push(GroupRec {
            key: g.key.clone(),
            part: part.clone(),
            opacity: g.opacity,
            visible: g.visible,
        });
        group_part.push(part);
        items.push(Item {
            key: g.key.clone(),
            action,
        });
    }

    // Match layers to the map: by key, then by the user's renames.
    let by_key: BTreeMap<&str, &LayerRec> = map
        .into_iter()
        .flat_map(|m| m.layers.iter())
        .map(|r| (r.key.as_str(), r))
        .collect();
    let scan_keys: BTreeSet<&str> = scan.layers.iter().map(|l| l.key.as_str()).collect();
    let mut renamed: BTreeMap<&str, &LayerRec> = BTreeMap::new(); // new key -> old record
    for (old, new) in &choices.rename {
        let Some(rec) = by_key.get(old.as_str()).copied() else {
            continue;
        };
        if !scan_keys.contains(old.as_str())
            && scan_keys.contains(new.as_str())
            && !by_key.contains_key(new.as_str())
        {
            renamed.insert(new.as_str(), rec);
        }
    }
    let renamed_olds: BTreeSet<&str> = renamed.values().map(|r| r.key.as_str()).collect();

    // Resolve every layer first, so a mask can name the mesh of a later layer.
    let mut order: Vec<usize> = (0..scan.layers.len()).collect();
    order.sort_by_key(|&i| scan.layers[i].z);
    let mut resolved: BTreeMap<usize, Resolved<'_>> = BTreeMap::new();
    for &i in &order {
        let l = &scan.layers[i];
        let rec = by_key
            .get(l.key.as_str())
            .copied()
            .map(|r| (r, None))
            .or_else(|| {
                renamed
                    .get(l.key.as_str())
                    .map(|r| (*r, Some(r.key.clone())))
            });
        let r = match (rec.clone(), l.rect) {
            (_, None) => Resolved::Skip {
                rec: rec.as_ref().map(|r| r.0),
            },
            (Some((rec, renamed_from)), Some(_)) if mesh_in_model(&rec.mesh) => {
                taken_meshes.insert(rec.mesh.clone());
                Resolved::Existing { rec, renamed_from }
            }
            (Some((rec, _)), Some(_)) => {
                let mesh = unique(&mut taken_meshes, mesh_in_model, &rec.mesh);
                Resolved::Add {
                    mesh,
                    restored: true,
                    rec: Some(rec),
                }
            }
            (None, Some(_)) => {
                let mesh = unique(&mut taken_meshes, mesh_in_model, &l.key);
                Resolved::Add {
                    mesh,
                    restored: false,
                    rec: None,
                }
            }
        };
        resolved.insert(i, r);
    }
    let mesh_of = |i: usize| -> Option<&str> {
        match resolved.get(&i)? {
            Resolved::Add { mesh, .. } => Some(mesh),
            Resolved::Existing { rec, .. } => Some(&rec.mesh),
            Resolved::Skip { .. } => None,
        }
    };

    // Texture files: new ones get the next number; the rest keep their path.
    let mut sorted_existing: Vec<String> = existing.textures.to_vec();
    sorted_existing.sort();
    let mut next = next_texture_number(&sorted_existing, map);
    let mut textures = Vec::new();
    let mut layer_texture: BTreeMap<usize, String> = BTreeMap::new();
    for &i in &order {
        let path = match &resolved[&i] {
            Resolved::Add { rec: Some(r), .. } => r.texture.clone(),
            Resolved::Add { rec: None, .. } => {
                let p = texture_path(next);
                next += 1;
                p
            }
            Resolved::Existing { rec, .. } => rec.texture.clone(),
            Resolved::Skip { .. } => continue,
        };
        layer_texture.insert(i, path);
    }
    let mut final_set: BTreeSet<String> = sorted_existing.iter().cloned().collect();
    for &i in &order {
        let Some(p) = layer_texture.get(&i) else {
            continue;
        };
        let needs_file = match &resolved[&i] {
            Resolved::Add { .. } => true,
            Resolved::Existing { rec, .. } => {
                let l = &scan.layers[i];
                l.image_hash != rec.image_hash || l.rect != Some(rec.rect)
            }
            Resolved::Skip { .. } => false,
        };
        if needs_file {
            textures.push(TextureJob {
                layer: i,
                path: p.clone(),
            });
            final_set.insert(p.clone());
        }
    }
    let final_paths: Vec<String> = final_set.into_iter().collect();
    let index_of = |path: &str| final_paths.iter().position(|p| p == path).unwrap_or(0) as u32;

    // Which meshes this plan removes.
    let mut removed: BTreeSet<String> = BTreeSet::new();
    let mut new_layers = Vec::new();
    let mut keep_recs: Vec<&LayerRec> = Vec::new();
    if let Some(m) = map {
        for r in &m.layers {
            let present =
                scan_keys.contains(r.key.as_str()) || renamed_olds.contains(r.key.as_str());
            if present {
                continue;
            }
            let remove = choices
                .missing
                .get(&r.key)
                .copied()
                .unwrap_or(choices.remove_missing);
            if remove {
                removed.insert(r.mesh.clone());
            } else {
                keep_recs.push(r);
            }
            items.push(Item {
                key: r.key.clone(),
                action: Action::Missing { remove },
            });
        }
    }

    // Meshes that other meshes mask must stop being masks before removal.
    for name in &removed {
        let Some(id) = model.meshes().find(name) else {
            continue;
        };
        for (_, other, mesh) in model.meshes().iter() {
            if removed.contains(other) || !mesh.masks.contains(&id) {
                continue;
            }
            let masks = mesh
                .masks
                .iter()
                .filter(|&&m| m != id)
                .filter_map(|&m| model.meshes().name(m).map(str::to_owned))
                .collect();
            commands.push(Command::SetMeshMasks {
                name: other.to_owned(),
                masks,
                inverted: mesh.mask_inverted,
            });
        }
    }
    for name in &removed {
        if mesh_in_model(name) {
            commands.push(Command::RemoveMesh { name: name.clone() });
        }
    }

    // Layers, bottom first.
    let mut mask_commands = Vec::new();
    for &i in &order {
        let l = &scan.layers[i];
        let part = l.group.map_or(root.clone(), |g| group_part[g].clone());
        // The mask this layer should have, as a mesh name.
        let want_clip: Option<(String, String)> = l
            .clip_base
            .filter(|_| l.clipping && scan.clipping_problem(l).is_none())
            .and_then(|b| mesh_of(b).map(|m| (scan.layers[b].key.clone(), m.to_owned())));
        let blend = blend_mode(&l.blend).unwrap_or(BlendMode::Normal);
        match &resolved[&i] {
            Resolved::Skip { rec } => {
                items.push(Item {
                    key: l.key.clone(),
                    action: Action::Unchanged,
                });
                if let Some(r) = rec {
                    new_layers.push((*r).clone());
                }
            }
            Resolved::Add {
                mesh,
                restored,
                rec: _,
            } => {
                let rect = l.rect.expect("layers with a mesh have pixels");
                let (positions, uvs, indices) = quad(rect, scale);
                commands.push(Command::AddMesh {
                    name: mesh.clone(),
                    part: part.clone(),
                    positions,
                    uvs,
                    indices,
                });
                let texture = index_of(&layer_texture[&i]);
                if texture != 0 {
                    commands.push(Command::SetMeshTexture {
                        name: mesh.clone(),
                        texture,
                    });
                }
                if l.opacity != 255 {
                    commands.push(Command::SetMeshOpacity {
                        name: mesh.clone(),
                        opacity: opacity_of(l.opacity),
                    });
                }
                if blend != BlendMode::Normal {
                    commands.push(Command::SetMeshBlendMode {
                        name: mesh.clone(),
                        blend_mode: blend,
                    });
                }
                commands.push(Command::SetMeshDrawOrder {
                    name: mesh.clone(),
                    draw_order: l.z as f32,
                });
                if !l.visible {
                    commands.push(Command::SetMeshVisible {
                        name: mesh.clone(),
                        visible: false,
                    });
                }
                if let Some((_, base)) = &want_clip {
                    mask_commands.push(Command::SetMeshMasks {
                        name: mesh.clone(),
                        masks: vec![base.clone()],
                        inverted: false,
                    });
                }
                new_layers.push(LayerRec {
                    key: l.key.clone(),
                    mesh: mesh.clone(),
                    part,
                    texture: layer_texture[&i].clone(),
                    rect,
                    image_hash: l.image_hash.clone(),
                    opacity: l.opacity,
                    blend: l.blend.clone(),
                    visible: l.visible,
                    z: l.z,
                    clip_base: want_clip.map(|c| c.0),
                });
                items.push(Item {
                    key: l.key.clone(),
                    action: Action::AddLayer {
                        restored: *restored,
                    },
                });
            }
            Resolved::Existing { rec, renamed_from } => {
                let rect = l.rect.expect("layers with a mesh have pixels");
                let mesh_id = model.meshes().find(&rec.mesh).expect("checked");
                let mesh: &ArtMesh = model.meshes().get(mesh_id).expect("found");
                let image = l.image_hash != rec.image_hash || rect != rec.rect;
                if rect != rec.rect {
                    commands.push(Command::SetMeshUvs {
                        name: rec.mesh.clone(),
                        uvs: remap_uvs(&mesh.uvs, rec.rect, rect),
                    });
                }
                let (mut set, mut kept) = (Vec::new(), Vec::new());
                let mut decide =
                    |name: &'static str, changed: bool, unedited: bool, cmd: Command| {
                        if !changed {
                            return;
                        }
                        if unedited {
                            commands.push(cmd);
                            set.push(name);
                        } else {
                            kept.push(name);
                        }
                    };
                decide(
                    "opacity",
                    l.opacity != rec.opacity,
                    same(mesh.opacity, opacity_of(rec.opacity)),
                    Command::SetMeshOpacity {
                        name: rec.mesh.clone(),
                        opacity: opacity_of(l.opacity),
                    },
                );
                decide(
                    "blend_mode",
                    l.blend != rec.blend,
                    Some(mesh.blend_mode) == blend_mode(&rec.blend),
                    Command::SetMeshBlendMode {
                        name: rec.mesh.clone(),
                        blend_mode: blend,
                    },
                );
                decide(
                    "visible",
                    l.visible != rec.visible,
                    mesh.visible == rec.visible,
                    Command::SetMeshVisible {
                        name: rec.mesh.clone(),
                        visible: l.visible,
                    },
                );
                decide(
                    "draw_order",
                    l.z != rec.z,
                    same(mesh.draw_order, rec.z as f32),
                    Command::SetMeshDrawOrder {
                        name: rec.mesh.clone(),
                        draw_order: l.z as f32,
                    },
                );
                let current: Vec<&str> = mesh
                    .masks
                    .iter()
                    .filter_map(|&m| model.meshes().name(m))
                    .collect();
                let recorded: Vec<&str> = rec
                    .clip_base
                    .as_deref()
                    .and_then(|k| by_key.get(k))
                    .map(|r| r.mesh.as_str())
                    .into_iter()
                    .collect();
                let want_key = want_clip.as_ref().map(|c| c.0.clone());
                if want_key != rec.clip_base {
                    if current == recorded {
                        mask_commands.push(Command::SetMeshMasks {
                            name: rec.mesh.clone(),
                            masks: want_clip.iter().map(|c| c.1.clone()).collect(),
                            inverted: mesh.mask_inverted,
                        });
                        set.push("clipping");
                    } else {
                        kept.push("clipping");
                    }
                }
                let action =
                    if !image && set.is_empty() && kept.is_empty() && renamed_from.is_none() {
                        Action::Unchanged
                    } else {
                        Action::UpdateLayer {
                            image,
                            set,
                            kept,
                            renamed_from: renamed_from.clone(),
                        }
                    };
                items.push(Item {
                    key: l.key.clone(),
                    action,
                });
                new_layers.push(LayerRec {
                    key: l.key.clone(),
                    mesh: rec.mesh.clone(),
                    part: rec.part.clone(),
                    texture: layer_texture[&i].clone(),
                    rect,
                    image_hash: l.image_hash.clone(),
                    opacity: l.opacity,
                    blend: l.blend.clone(),
                    visible: l.visible,
                    z: l.z,
                    clip_base: want_key,
                });
            }
        }
    }
    commands.extend(mask_commands);

    // Meshes of other origin whose texture index moved because files were added.
    if final_paths != sorted_existing {
        let renumber: BTreeSet<&str> = new_layers.iter().map(|r| r.mesh.as_str()).collect();
        for (_, name, mesh) in model.meshes().iter() {
            if removed.contains(name) || renumber.contains(name) {
                continue;
            }
            let Some(path) = sorted_existing.get(mesh.texture as usize) else {
                continue;
            };
            let now = index_of(path);
            if now != mesh.texture {
                commands.push(Command::SetMeshTexture {
                    name: name.to_owned(),
                    texture: now,
                });
            }
        }
    }
    // Existing layer meshes keep their own file, whose index may have moved too.
    for r in &new_layers {
        let Some(id) = model.meshes().find(&r.mesh) else {
            continue;
        };
        let Some(mesh) = model.meshes().get(id) else {
            continue;
        };
        if removed.contains(&r.mesh) {
            continue;
        }
        if let Some(old_path) = sorted_existing.get(mesh.texture as usize)
            && *old_path == r.texture
            && index_of(old_path) != mesh.texture
        {
            commands.push(Command::SetMeshTexture {
                name: r.mesh.clone(),
                texture: index_of(old_path),
            });
        }
    }

    for r in keep_recs {
        new_layers.push(r.clone());
    }
    notes.sort();
    textures.sort_by(|a, b| a.path.cmp(&b.path));
    Plan {
        items,
        notes,
        commands,
        textures,
        map: PsdMap {
            version: MAP_VERSION,
            source: source.clone(),
            units_per_pixel: scale,
            root_part: root,
            groups: group_recs,
            layers: new_layers,
        },
    }
}

/// Decodes the layers a plan writes, one at a time, trims them to the planned
/// rectangle and hands each PNG to `sink`. Call it after the batch applied.
/// A layer whose pixels differ from the scan (the file changed since) is an
/// error.
pub fn render_textures<R: Read + Seek>(
    plan: &Plan,
    scan: &Scan,
    psd: &Psd,
    r: &mut R,
    limits: &Limits,
    mut sink: impl FnMut(&str, Vec<u8>) -> Result<(), String>,
) -> Result<(), String> {
    let mut image = crate::Image::empty();
    for job in &plan.textures {
        let l: &ScannedLayer = &scan.layers[job.layer];
        psd.decode_layer_into(r, l.entry, limits, &mut image)
            .map_err(|e: PsdError| e.to_string())?;
        let b = image
            .opaque_bounds()
            .ok_or("a layer became transparent since the scan")?;
        if image.hash_rect(b) != l.image_hash {
            return Err(format!("layer '{}' changed since it was scanned", l.key));
        }
        sink(&job.path, image.crop(b).to_png()?)?;
    }
    Ok(())
}
