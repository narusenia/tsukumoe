// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Opening and saving a project (a directory or a zip archive, through
//! `tsukumoe-format`) and the recent-projects list.

use std::fs;
use std::path::{Path, PathBuf};

use tsukumoe_format::{Project, read_model, write_model};

use crate::dispatcher::Dispatcher;

pub const RECENT_MAX: usize = 10;

/// An open project: its files, the editing model behind the dispatcher, and
/// where it was read from.
pub struct Document {
    pub path: PathBuf,
    project: Project,
    pub dispatcher: Dispatcher,
}

impl Document {
    pub fn open(path: &Path) -> Result<Self, String> {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let project = if path.is_dir() {
            Project::read_dir(path).map_err(|e| err(&e))?
        } else {
            let bytes = fs::read(path).map_err(|e| err(&e))?;
            Project::read_zip(&bytes).map_err(|e| err(&e))?
        };
        let model = read_model(&project).map_err(|e| err(&e))?;
        Ok(Self {
            path: path.to_owned(),
            project,
            dispatcher: Dispatcher::new(model),
        })
    }

    /// The project's files, for the textures the viewport draws.
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub fn dirty(&self) -> bool {
        self.dispatcher.dirty()
    }

    pub fn save(&mut self) -> Result<(), String> {
        let path = self.path.clone();
        self.save_to(&path)
    }

    /// Saves to `path` (a folder when it is one or has no extension, otherwise
    /// a zip archive) and makes it the document's path.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        write_model(self.dispatcher.model(), &mut self.project).map_err(|e| err(&e))?;
        if path.is_dir() || path.extension().is_none() {
            self.project.write_dir(path).map_err(|e| err(&e))?;
        } else {
            let bytes = self.project.write_zip().map_err(|e| err(&e))?;
            fs::write(path, bytes).map_err(|e| err(&e))?;
        }
        self.path = path.to_owned();
        self.dispatcher.mark_saved();
        Ok(())
    }
}

/// Most recently used first, no repeats, at most [`RECENT_MAX`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Recent(Vec<PathBuf>);

impl Recent {
    pub fn paths(&self) -> &[PathBuf] {
        &self.0
    }

    pub fn push(&mut self, path: &Path) {
        self.0.retain(|p| p != path);
        self.0.insert(0, path.to_owned());
        self.0.truncate(RECENT_MAX);
    }

    pub fn remove(&mut self, path: &Path) {
        self.0.retain(|p| p != path);
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!(
            self.0
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
        )
    }

    /// A damaged value gives an empty list.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let paths = value
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(PathBuf::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut recent = Self(paths);
        recent.0.truncate(RECENT_MAX);
        recent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tsukumoe-format/tests/fixtures/minimal")
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tsukumoe-app-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn edit_save_reopen_in_both_layouts() {
        for (tag, file) in [("dir", "proj"), ("zip", "proj.tsukumoe")] {
            let out = scratch(tag);
            fs::create_dir_all(&out).unwrap();
            let target = out.join(file);
            let mut doc = Document::open(&fixture()).unwrap();
            assert!(!doc.dirty());
            doc.dispatcher
                .submit(Command::AddParameter {
                    name: "New".into(),
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    repeat: false,
                })
                .unwrap();
            assert!(doc.dirty());
            doc.save_to(&target).unwrap();
            assert!(!doc.dirty() && doc.path == target);
            let again = Document::open(&target).unwrap();
            assert!(
                again.dispatcher.model().parameters().find("New").is_some(),
                "{tag}"
            );
            fs::remove_dir_all(&out).unwrap();
        }
    }

    #[test]
    fn a_missing_or_broken_project_is_an_error() {
        assert!(Document::open(Path::new("/no/such/project")).is_err());
        let dir = scratch("broken");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("x.zip"), b"not a zip").unwrap();
        assert!(Document::open(&dir.join("x.zip")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recent_is_most_recent_first_unique_and_capped() {
        let mut r = Recent::default();
        for i in 0..12 {
            r.push(Path::new(&format!("/p/{i}")));
        }
        r.push(Path::new("/p/5"));
        assert_eq!(r.paths().len(), RECENT_MAX);
        assert_eq!(r.paths()[0], Path::new("/p/5"));
        assert_eq!(
            r.paths()
                .iter()
                .filter(|p| **p == Path::new("/p/5"))
                .count(),
            1
        );
        assert_eq!(Recent::from_json(&r.to_json()), r);
        assert_eq!(Recent::from_json(&serde_json::json!(3)), Recent::default());
    }
}
