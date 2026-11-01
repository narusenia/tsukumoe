// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Opening a project (directory or archive) as a [`Model`] and saving it back
//! without leaving a half-written project behind.

use std::fs;
use std::path::Path;

use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_format::{MANIFEST_PATH, Project, read_model, to_json, write_model};

/// Reads the project at `path` and its model.
pub fn open(path: &str) -> Result<(Project, Model), String> {
    let p = Path::new(path);
    let project = if p.is_dir() {
        Project::read_dir(p).map_err(|e| e.to_string())?
    } else {
        let bytes = fs::read(p).map_err(|e| format!("{path}: {e}"))?;
        Project::read_zip(&bytes).map_err(|e| e.to_string())?
    };
    let model = read_model(&project).map_err(|e| e.to_string())?;
    Ok((project, model))
}

/// Writes `model` into `project` and `project` to `path`. Every file is
/// written to a temporary name first and then renamed, so a failure while
/// writing leaves the project as it was. Undo history is not saved: the
/// format has no place for the command list.
pub fn save(path: &str, project: &mut Project, model: &Model) -> Result<(), String> {
    write_model(model, project).map_err(|e| e.to_string())?;
    let p = Path::new(path);
    if !p.is_dir() {
        let bytes = project.write_zip().map_err(|e| e.to_string())?;
        return replace_all(&[(p.to_path_buf(), bytes)]);
    }
    let mut files = vec![(
        p.join(MANIFEST_PATH),
        to_json(&project.manifest).map_err(|e| e.to_string())?,
    )];
    files.extend(
        project
            .files
            .iter()
            .map(|(name, bytes)| (p.join(name), bytes.clone())),
    );
    // Skip files that did not change, so textures are not rewritten.
    files.retain(|(file, bytes)| fs::read(file).map_or(true, |old| old != *bytes));
    replace_all(&files)?;
    // Motions and expressions are renumbered on save; drop the stale ones.
    for dir in ["motions", "expressions"] {
        let Ok(entries) = fs::read_dir(p.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = format!("{dir}/{}", entry.file_name().to_string_lossy());
            if entry.path().extension().is_some_and(|e| e == "json")
                && !project.files.contains_key(&name)
            {
                fs::remove_file(entry.path()).map_err(|e| format!("{name}: {e}"))?;
            }
        }
    }
    Ok(())
}

fn replace_all(files: &[(std::path::PathBuf, Vec<u8>)]) -> Result<(), String> {
    let temp = |file: &Path| {
        let mut name = file.file_name().unwrap_or_default().to_os_string();
        name.push(".tmp");
        file.with_file_name(name)
    };
    let fail = |file: &Path, e: std::io::Error| format!("{}: {e}", file.display());
    let mut written = Vec::new();
    for (file, bytes) in files {
        let result = file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(temp(file), bytes));
        written.push(temp(file));
        if let Err(e) = result {
            for t in &written {
                let _ = fs::remove_file(t);
            }
            return Err(fail(file, e));
        }
    }
    for (file, _) in files {
        fs::rename(temp(file), file).map_err(|e| fail(file, e))?;
    }
    Ok(())
}

/// Parses a command or an array of commands (one undo step: an array
/// becomes a `Batch`).
pub fn parse_commands(json: &[u8]) -> Result<Command, String> {
    match serde_json::from_slice::<serde_json::Value>(json).map_err(|e| e.to_string())? {
        serde_json::Value::Array(items) => {
            let commands = items
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<Command>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(Command::Batch { commands })
        }
        value => serde_json::from_value(value).map_err(|e| e.to_string()),
    }
}
