// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe psd inspect | import`: reading a PSD and importing it into a
//! project (see `docs/implementation/M5-editor/psd-import.md`).

use std::collections::BTreeMap;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tsukumoe_core::model::Model;
use tsukumoe_format::{Manifest, Project, write_model};
use tsukumoe_psd::{
    Action, Choices, EntryKind, Limits, MAP_PATH, Plan, Prepared, Psd, blend_mode, texture_paths,
};

use crate::store;

fn open_psd(path: &str) -> Result<Psd, String> {
    let file = fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    Psd::read(&mut BufReader::new(file), &Limits::default()).map_err(|e| e.to_string())
}

/// The layer structure and the warnings, as JSON. No pixel is decoded.
pub fn inspect(path: &str) -> Result<Value, String> {
    let psd = open_psd(path)?;
    let mut warnings: Vec<Value> = psd
        .warnings
        .iter()
        .map(|w| json!({ "layer": psd.entries[w.entry].name, "warning": format!("{:?}", w.kind) }))
        .collect();
    let mut layers = Vec::new();
    let (mut groups, mut hidden, mut clipping) = (0, 0, 0);
    for e in &psd.entries {
        let is_group = e.kind == EntryKind::Group;
        groups += usize::from(is_group);
        hidden += usize::from(!e.visible);
        clipping += usize::from(e.clipping);
        let blend = e.blend_key_str();
        if !is_group && blend_mode(&blend).is_none() {
            warnings.push(json!({
                "layer": e.name,
                "warning": format!("blend mode '{blend}' has no counterpart: Normal"),
            }));
        }
        layers.push(json!({
            "name": e.name,
            "kind": if is_group { "group" } else { "layer" },
            "path": e.path.join("/"),
            "z": e.z,
            "rect": [e.rect.left, e.rect.top, e.rect.right, e.rect.bottom],
            "opacity": e.opacity,
            "blend": blend,
            "visible": e.visible,
            "clipping": e.clipping,
        }));
    }
    Ok(json!({
        "canvas": [psd.width, psd.height],
        "depth": psd.depth,
        "summary": {
            "entries": psd.entries.len(),
            "groups": groups,
            "layers": psd.entries.len() - groups,
            "hidden": hidden,
            "clipping": clipping,
        },
        "layers": layers,
        "warnings": warnings,
    }))
}

#[derive(Debug, Default)]
pub struct ImportOptions {
    pub apply: bool,
    pub choices: Choices,
}

/// What a plan will do, counted.
pub fn summary(plan: &Plan) -> Value {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for item in &plan.items {
        let name = match &item.action {
            Action::AddGroup => "add_group",
            Action::AddLayer { .. } => "add_layer",
            Action::UpdateLayer { image: true, .. } => "image_changed",
            Action::UpdateLayer { .. } | Action::UpdateGroup { .. } => "updated",
            Action::Missing { .. } => "missing",
            Action::Unchanged => "unchanged",
        };
        *counts.entry(name).or_default() += 1;
    }
    json!({
        "counts": counts,
        "commands": plan.commands.len(),
        "textures": plan.textures.len(),
        "notes": plan.notes.len(),
    })
}

fn open_or_new(project: &str) -> Result<(Project, Model, bool), String> {
    if Path::new(project).exists() {
        let (p, m) = store::open(project)?;
        Ok((p, m, false))
    } else {
        Ok((Project::new(Manifest::new(None)), Model::new(), true))
    }
}

/// Plans an import of `psd` into `project` and, with `apply`, carries it out.
/// A project that does not exist yet is created. Nothing is written when the
/// plan is empty, when `apply` is off, or when applying fails.
pub fn import(project: &str, psd: &str, options: &ImportOptions) -> Result<Value, String> {
    let prepared = Prepared::open(Path::new(psd), Limits::default(), |done, total| {
        if total > 0 && (done % 100 == 0 || done == total) {
            eprintln!("scanning layers {done}/{total}");
        }
    })?;
    let (mut files, mut model, new_project) = open_or_new(project)?;
    let textures = texture_paths(files.files.keys().map(String::as_str));
    let map = files.files.get(MAP_PATH).cloned();
    let plan = prepared.plan(&model, &textures, map.as_deref(), &options.choices)?;
    let mut out = json!({
        "project": project,
        "canvas": prepared.scan.canvas,
        "summary": summary(&plan),
        "items": plan.items,
        "notes": plan.notes,
        "applied": false,
    });
    if !options.apply || plan.is_empty() {
        return Ok(out);
    }
    // The batch first: it either applies whole or not at all.
    model.apply(plan.batch()).map_err(|e| e.to_string())?;
    write_textures(project, &mut files, &prepared, &plan)?;
    files.files.insert(MAP_PATH.into(), plan.map.to_json());
    if new_project {
        write_model(&model, &mut files).map_err(|e| e.to_string())?;
    }
    store::save(project, &mut files, &model).map_err(|e| {
        // The texture files are already in place; the model is not saved.
        format!("{e} (texture files were written but the model was not saved)")
    })?;
    out["applied"] = json!(true);
    Ok(out)
}

/// Writes the plan's textures. For a directory project each PNG goes to a
/// temporary name first and all are renamed together, so a failure leaves no
/// orphan; an archive keeps them in memory until it is saved.
fn write_textures(
    project: &str,
    files: &mut Project,
    prepared: &Prepared,
    plan: &Plan,
) -> Result<(), String> {
    if plan.textures.is_empty() {
        return Ok(());
    }
    let root = Path::new(project);
    let archive = if root.exists() {
        !root.is_dir()
    } else {
        root.extension()
            .is_some_and(|e| e == "zip" || e == "tsukumoe")
    };
    if archive {
        let mut staged = Vec::new();
        prepared.render(plan, |path, png| {
            staged.push((path.to_owned(), png));
            Ok(())
        })?;
        files.files.extend(staged);
        return Ok(());
    }
    let mut temps: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    let created_root = !root.exists();
    let cleanup = |temps: &[(PathBuf, PathBuf, String)]| {
        for (t, _, _) in temps {
            let _ = fs::remove_file(t);
        }
    };
    let result = (|| {
        fs::create_dir_all(root.join("textures")).map_err(|e| e.to_string())?;
        prepared.render(plan, |path, png| {
            let target = root.join(path);
            let mut name = target.file_name().unwrap_or_default().to_os_string();
            name.push(".tmp");
            let temp = target.with_file_name(name);
            fs::write(&temp, png).map_err(|e| format!("{}: {e}", temp.display()))?;
            temps.push((temp, target, path.to_owned()));
            Ok(())
        })
    })();
    if let Err(e) = result {
        cleanup(&temps);
        if created_root {
            let _ = fs::remove_dir_all(root);
        }
        return Err(e);
    }
    for (temp, target, path) in &temps {
        if let Err(e) = fs::rename(temp, target) {
            cleanup(&temps);
            return Err(format!("{}: {e}", target.display()));
        }
        // The file on disk is the truth; drop any stale copy in memory.
        files.files.remove(path);
    }
    Ok(())
}
