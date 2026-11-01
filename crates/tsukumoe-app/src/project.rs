// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Opening and saving a project (a directory or a zip archive, through
//! `tsukumoe-format`) and the recent-projects list.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tsukumoe_core::model::Model;
use tsukumoe_format::{Manifest, Project, read_model, write_model};

use crate::dispatcher::Dispatcher;

pub const RECENT_MAX: usize = 10;

/// An open project: its files, the editing model behind the dispatcher, and
/// where it was read from.
pub struct Document {
    /// Distinct for every opened document; views key their caches on it.
    pub id: u64,
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
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            path: path.to_owned(),
            project,
            dispatcher: Dispatcher::new(model),
        })
    }

    /// The project's files, for the textures the viewport draws.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The alpha channel of the `index`-th texture (the order the viewport
    /// loads them in): width, height and one byte per pixel.
    pub fn texture_alpha(&self, index: u32) -> Result<(u32, u32, Vec<u8>), String> {
        let (path, bytes) = self
            .project
            .files
            .iter()
            .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
            .nth(index as usize)
            .ok_or_else(|| format!("no texture {index}"))?;
        let texture = tsukumoe_gpu::Texture::from_png(bytes).map_err(|e| format!("{path}: {e}"))?;
        let alpha = texture
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3])
            .collect();
        Ok((texture.width, texture.height, alpha))
    }

    /// Adds or replaces a file of the project (a texture, the PSD map). It is
    /// written on the next save.
    pub fn put_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.project.files.insert(path.to_owned(), bytes);
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
        let folder = path.is_dir() || path.extension().is_none();
        self.write_to(path, folder)
    }

    /// Saves as a folder project whatever the name looks like.
    pub fn save_to_folder(&mut self, path: &Path) -> Result<(), String> {
        self.write_to(path, true)
    }

    fn write_to(&mut self, path: &Path, folder: bool) -> Result<(), String> {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        write_model(self.dispatcher.model(), &mut self.project).map_err(|e| err(&e))?;
        if folder {
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

/// Creates an empty folder project in `dir` and returns the files written
/// (relative paths). `dir` must be missing (its parent must exist) or an empty
/// folder; nothing outside `dir` is written and nothing existing is replaced.
/// On failure nothing is left behind.
pub fn create_project(dir: &Path) -> Result<Vec<String>, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", dir.display());
    let existed = dir.exists();
    if existed {
        if !dir.is_dir() {
            return Err(err(&"not a folder"));
        }
        if fs::read_dir(dir).map_err(|e| err(&e))?.next().is_some() {
            return Err(err(&"the folder is not empty"));
        }
    }
    let mut project = Project::new(Manifest::new(None));
    write_model(&Model::new(), &mut project).map_err(|e| err(&e))?;
    let mut written: Vec<String> = vec![tsukumoe_format::MANIFEST_PATH.to_owned()];
    written.extend(project.files.keys().cloned());
    if !existed {
        fs::create_dir(dir).map_err(|e| err(&e))?;
    }
    if let Err(e) = project.write_dir(dir) {
        // `dir` was empty or ours: remove what was written.
        if existed {
            for name in &written {
                let _ = fs::remove_file(dir.join(name));
            }
        } else {
            let _ = fs::remove_dir_all(dir);
        }
        return Err(err(&e));
    }
    Ok(written)
}

/// Removes the files [`create_project`] wrote and the folder if it is empty
/// afterwards (used when a new project is abandoned). Anything else in the
/// folder is left alone.
pub fn discard_project(dir: &Path, written: &[String]) {
    for name in written {
        let _ = fs::remove_file(dir.join(name));
    }
    // Only an empty folder goes (remove_dir fails otherwise); empty
    // sub-folders such as `textures` are removed first.
    for sub in ["textures", "sources"] {
        let _ = fs::remove_dir(dir.join(sub));
    }
    let _ = fs::remove_dir(dir);
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

    #[test]
    fn a_new_project_is_made_in_an_empty_or_new_folder_and_opens() {
        let out = scratch("new");
        fs::create_dir_all(&out).unwrap();
        // A new folder.
        let a = out.join("a");
        let written = create_project(&a).unwrap();
        assert!(written.contains(&"manifest.json".to_owned()));
        let doc = Document::open(&a).unwrap();
        assert!(doc.dispatcher.model().meshes().is_empty());
        // An existing empty folder.
        let b = out.join("b");
        fs::create_dir(&b).unwrap();
        create_project(&b).unwrap();
        Document::open(&b).unwrap();
        fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn a_new_project_refuses_what_it_cannot_make_and_leaves_nothing() {
        let out = scratch("new-bad");
        fs::create_dir_all(&out).unwrap();
        // Not empty: untouched.
        let full = out.join("full");
        fs::create_dir(&full).unwrap();
        fs::write(full.join("keep.txt"), b"x").unwrap();
        assert!(create_project(&full).unwrap_err().contains("not empty"));
        assert_eq!(fs::read_dir(&full).unwrap().count(), 1);
        // A file, and a folder whose parent is missing: nothing is created.
        let file = out.join("f");
        fs::write(&file, b"x").unwrap();
        assert!(create_project(&file).is_err());
        let deep = out.join("no/such/dir");
        assert!(create_project(&deep).is_err());
        assert!(!out.join("no").exists());
        // Nothing but the chosen folders was written under `out`.
        let mut names: Vec<_> = fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["f", "full"]);
        fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn discarding_a_new_project_removes_its_files_and_keeps_other_files() {
        let out = scratch("discard");
        fs::create_dir_all(&out).unwrap();
        let a = out.join("a");
        let written = create_project(&a).unwrap();
        discard_project(&a, &written);
        assert!(!a.exists());
        let b = out.join("b");
        fs::create_dir(&b).unwrap();
        let written = create_project(&b).unwrap();
        fs::write(b.join("mine.txt"), b"x").unwrap();
        discard_project(&b, &written);
        assert_eq!(fs::read_dir(&b).unwrap().count(), 1);
        fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn save_to_folder_writes_a_folder_even_with_an_extension() {
        let out = scratch("folder-ext");
        fs::create_dir_all(&out).unwrap();
        let mut doc = Document::open(&fixture()).unwrap();
        let target = out.join("model.v1");
        doc.save_to_folder(&target).unwrap();
        assert!(target.is_dir() && target.join("manifest.json").is_file());
        fs::remove_dir_all(&out).unwrap();
    }
}
