// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The state behind File > Import PSD: the path, the plan against the open
//! project, and the user's choices. It draws nothing, so it is tested without
//! a window. The dialog in `editor.rs` only shows and edits it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use tsukumoe_psd::{Action, Choices, Limits, MAP_PATH, Note, Plan, Prepared, texture_paths};

use crate::project::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for a path.
    Path,
    /// The PSD is being read on another thread.
    Loading,
    /// The plan is shown.
    Review,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    AddGroup,
    AddLayer,
    /// A layer's texture is replaced.
    ImageChanged,
    /// Values from the PSD are updated.
    Updated,
    /// In the project, not in the PSD any more; `remove` is the choice.
    Missing {
        remove: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub kind: RowKind,
    /// The properties updated and kept, for the user to read.
    pub detail: String,
    /// For a new layer: the vanished layer it was matched with by hand.
    pub matched_with: Option<String>,
}

pub struct PsdImport {
    pub path: String,
    pub stage: Stage,
    pub error: Option<String>,
    pub choices: Choices,
    /// Set when the project was just made for this import: its folder and the
    /// files written, so cancelling can remove them.
    pub created: Option<(PathBuf, Vec<String>)>,
    /// The import was applied.
    pub confirmed: bool,
    prepared: Option<Prepared>,
    plan: Option<Plan>,
    loading: Option<Receiver<Result<Prepared, String>>>,
}

impl Default for PsdImport {
    fn default() -> Self {
        Self::new()
    }
}

impl PsdImport {
    pub fn new() -> Self {
        Self {
            path: String::new(),
            stage: Stage::Path,
            error: None,
            choices: Choices::default(),
            created: None,
            confirmed: false,
            prepared: None,
            plan: None,
            loading: None,
        }
    }

    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    pub fn notes(&self) -> &[Note] {
        self.plan.as_ref().map_or(&[], |p| &p.notes)
    }

    /// Starts reading the file on another thread; [`Self::poll`] picks up the result.
    pub fn start(&mut self) {
        let path = self.path.trim().to_owned();
        if path.is_empty() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Prepared::open(
                Path::new(&path),
                Limits::default(),
                |_, _| {},
            ));
        });
        self.loading = Some(rx);
        self.stage = Stage::Loading;
        self.error = None;
    }

    /// Takes the result of a finished read. True when something changed.
    pub fn poll(&mut self, doc: &Document) -> bool {
        let Some(rx) = &self.loading else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("the reader stopped".to_owned()),
        };
        self.loading = None;
        self.finish(result, doc);
        true
    }

    /// Reads the file and waits (for tests and scripts).
    pub fn load_blocking(&mut self, doc: &Document) {
        self.error = None;
        let result = Prepared::open(Path::new(self.path.trim()), Limits::default(), |_, _| {});
        self.finish(result, doc);
    }

    fn finish(&mut self, result: Result<Prepared, String>, doc: &Document) {
        match result {
            Ok(prepared) => {
                self.prepared = Some(prepared);
                self.choices = Choices::default();
                self.replan(doc);
            }
            Err(e) => {
                self.stage = Stage::Path;
                self.error = Some(e);
            }
        }
    }

    fn replan(&mut self, doc: &Document) {
        let Some(prepared) = &self.prepared else {
            return;
        };
        let files = &doc.project().files;
        let textures = texture_paths(files.keys().map(String::as_str));
        // The map with the renames made since it was written.
        let map = doc.psd_map_bytes();
        match prepared.plan(
            doc.dispatcher.model(),
            &textures,
            map.as_deref(),
            &self.choices,
        ) {
            Ok(plan) => {
                self.plan = Some(plan);
                self.stage = Stage::Review;
            }
            Err(e) => {
                self.plan = None;
                self.stage = Stage::Path;
                self.error = Some(e);
            }
        }
    }

    /// Keep (false) or remove (true) the mesh of a vanished layer.
    pub fn set_remove(&mut self, key: &str, remove: bool, doc: &Document) {
        self.choices.missing.insert(key.to_owned(), remove);
        self.replan(doc);
    }

    /// Matches a vanished layer (`old`) with a new one by hand, or clears the
    /// match with `None`. `new` is looked up from the new layer's side.
    pub fn set_match(&mut self, new: &str, old: Option<&str>, doc: &Document) {
        self.choices.rename.retain(|_, n| n != new);
        if let Some(old) = old {
            self.choices.rename.remove(old);
            self.choices.rename.insert(old.to_owned(), new.to_owned());
        }
        self.replan(doc);
    }

    /// The vanished layers that can still be matched with a new one.
    pub fn match_candidates(&self) -> Vec<String> {
        self.plan.as_ref().map_or_else(Vec::new, |p| {
            let (missing, _) = p.renamable();
            let mut v: Vec<String> = missing.into_iter().map(str::to_owned).collect();
            // A vanished layer already matched is a renamed one now: offer it
            // under its old key so the match can be moved.
            v.extend(self.choices.rename.keys().cloned());
            v.sort();
            v.dedup();
            v
        })
    }

    /// The changes to show, in plan order; unchanged items are left out.
    pub fn rows(&self) -> Vec<Row> {
        let Some(plan) = &self.plan else {
            return Vec::new();
        };
        plan.items
            .iter()
            .filter_map(|item| {
                let (kind, detail, matched_with) = match &item.action {
                    Action::AddGroup => (RowKind::AddGroup, String::new(), None),
                    Action::AddLayer { .. } => (
                        RowKind::AddLayer,
                        String::new(),
                        self.choices
                            .rename
                            .iter()
                            .find(|(_, n)| **n == item.key)
                            .map(|(o, _)| o.clone()),
                    ),
                    Action::UpdateLayer {
                        image,
                        set,
                        kept,
                        renamed_from,
                    } => (
                        if *image {
                            RowKind::ImageChanged
                        } else {
                            RowKind::Updated
                        },
                        detail(set, kept),
                        renamed_from.clone(),
                    ),
                    Action::UpdateGroup { set, kept } => {
                        (RowKind::Updated, detail(set, kept), None)
                    }
                    Action::Missing { remove } => {
                        (RowKind::Missing { remove: *remove }, String::new(), None)
                    }
                    Action::Unchanged => return None,
                };
                Some(Row {
                    key: item.key.clone(),
                    kind,
                    detail,
                    matched_with,
                })
            })
            .collect()
    }

    /// New layers that could be matched with a vanished one.
    pub fn is_new_layer(&self, key: &str) -> bool {
        self.plan.as_ref().is_some_and(|p| {
            p.items
                .iter()
                .any(|i| i.key == key && matches!(i.action, Action::AddLayer { restored: false }))
                || self.choices.rename.values().any(|n| n == key)
        })
    }

    pub fn can_confirm(&self) -> bool {
        self.stage == Stage::Review && self.plan.as_ref().is_some_and(|p| !p.is_empty())
    }

    /// Applies the plan as one command, then adds the texture files and the
    /// map to the project. If a texture cannot be written the command is
    /// undone and the project keeps nothing of the import.
    pub fn confirm(&mut self, doc: &mut Document) -> Result<(), String> {
        let (Some(plan), Some(prepared)) = (&self.plan, &self.prepared) else {
            return Err("nothing to import".into());
        };
        if plan.is_empty() {
            return Err("nothing to import".into());
        }
        doc.dispatcher.submit(plan.batch())?;
        let mut staged = Vec::new();
        let rendered = prepared.render(plan, |path, png| {
            staged.push((path.to_owned(), png));
            Ok(())
        });
        if let Err(e) = rendered {
            doc.dispatcher.undo();
            return Err(e);
        }
        for (path, png) in staged {
            doc.put_file(&path, png);
        }
        doc.put_file(MAP_PATH, plan.map.to_json());
        self.confirmed = true;
        Ok(())
    }
}

fn detail(set: &[&str], kept: &[&str]) -> String {
    let mut parts = Vec::new();
    if !set.is_empty() {
        parts.push(set.join(", "));
    }
    if !kept.is_empty() {
        parts.push(format!("({})", kept.join(", ")));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tsukumoe_core::model::Model;
    use tsukumoe_format::{Manifest, Project, write_model};
    use tsukumoe_psd::writer::{GroupSpec, LayerSpec, Node, PsdSpec, write};

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tsukumoe-app-psd-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn doc(dir: &Path) -> Document {
        let mut project = Project::new(Manifest::new(None));
        write_model(&Model::new(), &mut project).unwrap();
        let path = dir.join("proj");
        project.write_dir(&path).unwrap();
        Document::open(&path).unwrap()
    }

    fn layer(name: &str, w: u32, v: u8) -> Node {
        Node::Layer(LayerSpec::solid(name, 0, 0, w, 4, [v, 1, 2, 255]))
    }

    fn psd(dir: &Path, file: &str, nodes: Vec<Node>) -> String {
        let path = dir.join(file);
        fs::write(&path, write(&PsdSpec::new(32, 32, nodes))).unwrap();
        path.display().to_string()
    }

    fn face(children: Vec<Node>) -> Node {
        Node::Group(GroupSpec::new("Face", children))
    }

    #[test]
    fn a_bad_path_reports_and_stays_on_the_path_stage() {
        let dir = scratch("bad");
        let d = doc(&dir);
        let mut s = PsdImport::new();
        s.path = dir.join("missing.psd").display().to_string();
        s.load_blocking(&d);
        assert_eq!(s.stage, Stage::Path);
        assert!(s.error.is_some() && s.plan().is_none() && !s.can_confirm());
    }

    #[test]
    fn the_first_import_lists_additions_and_confirms_as_one_undo_step() {
        let dir = scratch("first");
        let mut d = doc(&dir);
        let file = psd(
            &dir,
            "a.psd",
            vec![face(vec![layer("eye", 8, 5)]), layer("bg", 32, 1)],
        );
        let mut s = PsdImport::new();
        s.path = file;
        s.load_blocking(&d);
        assert_eq!(s.stage, Stage::Review);
        let kinds: Vec<_> = s.rows().iter().map(|r| (r.key.clone(), r.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("Face".to_owned(), RowKind::AddGroup),
                ("bg".to_owned(), RowKind::AddLayer),
                ("Face/eye".to_owned(), RowKind::AddLayer),
            ]
        );
        assert!(s.can_confirm());
        let steps = d.dispatcher.model().history().commands().count();
        s.confirm(&mut d).unwrap();
        assert_eq!(d.dispatcher.model().history().commands().count(), steps + 1);
        assert_eq!(d.dispatcher.model().meshes().len(), 2);
        assert_eq!(
            d.project()
                .files
                .keys()
                .filter(|k| k.starts_with("textures/"))
                .count(),
            2
        );
        assert!(d.project().files.contains_key(MAP_PATH));
        assert!(d.dirty());
        assert!(d.dispatcher.undo());
        assert_eq!(d.dispatcher.model().meshes().len(), 0);
    }

    #[test]
    fn renaming_a_mesh_or_part_keeps_the_reimport_matching_the_same_layers() {
        use tsukumoe_core::command::Command;
        let dir = scratch("renamed");
        let mut d = doc(&dir);
        let file = psd(
            &dir,
            "a.psd",
            vec![face(vec![layer("eye", 8, 5)]), layer("bg", 32, 1)],
        );
        let mut s = PsdImport::new();
        s.path = file;
        s.load_blocking(&d);
        s.confirm(&mut d).unwrap();
        let nothing = |s: &mut PsdImport, d: &Document| {
            s.load_blocking(d);
            s.rows().is_empty() && !s.can_confirm()
        };
        assert!(nothing(&mut s, &d));
        // Rename the mesh and its part: the same file still finds its layers.
        d.dispatcher
            .submit(Command::RenameMesh {
                name: "Face/eye".into(),
                new_name: "Eye".into(),
            })
            .unwrap();
        d.dispatcher
            .submit(Command::RenamePart {
                name: "Face".into(),
                new_name: "Head".into(),
            })
            .unwrap();
        assert!(
            nothing(&mut s, &d),
            "no layer is seen as new or missing after a rename"
        );
        // The map says the new names, and saving keeps them.
        let map = tsukumoe_psd::PsdMap::parse(&d.psd_map_bytes().unwrap()).unwrap();
        assert!(
            map.layers
                .iter()
                .any(|l| l.mesh == "Eye" && l.part == "Head")
        );
        assert!(map.groups.iter().any(|g| g.part == "Head"));
        // Undo and redo follow, in both directions.
        assert!(d.dispatcher.undo() && d.dispatcher.undo());
        assert!(nothing(&mut s, &d));
        let map = tsukumoe_psd::PsdMap::parse(&d.psd_map_bytes().unwrap()).unwrap();
        assert!(
            map.layers
                .iter()
                .any(|l| l.mesh == "Face/eye" && l.part == "Face")
        );
        assert!(d.dispatcher.redo() && d.dispatcher.redo());
        assert!(nothing(&mut s, &d));
        d.save().unwrap();
        let saved = tsukumoe_psd::PsdMap::parse(d.project().files.get(MAP_PATH).unwrap()).unwrap();
        assert!(saved.layers.iter().any(|l| l.mesh == "Eye"));
        // Reopened, the reimport still matches.
        let mut reopened = Document::open(&d.path).unwrap();
        let mut again = PsdImport::new();
        again.path = dir.join("a.psd").display().to_string();
        again.load_blocking(&reopened);
        assert!(again.rows().is_empty());
        let _ = &mut reopened;
    }

    #[test]
    fn names_swapped_between_two_meshes_move_the_map_entries_with_them() {
        use tsukumoe_core::command::Command;
        let dir = scratch("swapped");
        let mut d = doc(&dir);
        let file = psd(&dir, "a.psd", vec![layer("a", 8, 5), layer("b", 4, 6)]);
        let mut s = PsdImport::new();
        s.path = file;
        s.load_blocking(&d);
        s.confirm(&mut d).unwrap();
        for (name, new_name) in [("a", "tmp"), ("b", "a"), ("tmp", "b")] {
            d.dispatcher
                .submit(Command::RenameMesh {
                    name: name.into(),
                    new_name: new_name.into(),
                })
                .unwrap();
        }
        let map = tsukumoe_psd::PsdMap::parse(&d.psd_map_bytes().unwrap()).unwrap();
        let mesh_of = |key: &str| {
            map.layers
                .iter()
                .find(|l| l.key == key)
                .unwrap()
                .mesh
                .clone()
        };
        // The layer "a" still has the mesh that was made from it, now called "b".
        assert_eq!(mesh_of("a"), "b");
        assert_eq!(mesh_of("b"), "a");
        s.load_blocking(&d);
        assert!(s.rows().is_empty());
    }

    #[test]
    fn a_reimport_shows_the_diff_and_the_choices_change_the_plan() {
        let dir = scratch("re");
        let mut d = doc(&dir);
        let first = psd(
            &dir,
            "a.psd",
            vec![face(vec![layer("eye", 8, 5), layer("nose", 4, 6)])],
        );
        let mut s = PsdImport::new();
        s.path = first.clone();
        s.load_blocking(&d);
        s.confirm(&mut d).unwrap();

        // The same file: nothing to do.
        s.load_blocking(&d);
        assert!(s.rows().is_empty() && !s.can_confirm());

        // "nose" vanished, "mouth" is new, "eye" has a new image.
        let second = psd(
            &dir,
            "b.psd",
            vec![face(vec![layer("eye", 10, 9), layer("mouth", 4, 7)])],
        );
        s.path = second;
        s.load_blocking(&d);
        let rows = s.rows();
        let kind = |k: &str| rows.iter().find(|r| r.key == k).unwrap().kind;
        assert_eq!(kind("Face/eye"), RowKind::ImageChanged);
        assert_eq!(kind("Face/mouth"), RowKind::AddLayer);
        assert_eq!(kind("Face/nose"), RowKind::Missing { remove: false });
        assert!(s.is_new_layer("Face/mouth"));
        assert_eq!(s.match_candidates(), ["Face/nose"]);

        s.set_remove("Face/nose", true, &d);
        assert!(
            s.rows()
                .iter()
                .any(|r| r.kind == RowKind::Missing { remove: true })
        );

        // Match the vanished nose with the new mouth: it becomes a rename.
        s.set_match("Face/mouth", Some("Face/nose"), &d);
        let mouth = s
            .rows()
            .into_iter()
            .find(|r| r.key == "Face/mouth")
            .unwrap();
        assert_eq!(mouth.matched_with.as_deref(), Some("Face/nose"));
        assert!(
            s.rows()
                .iter()
                .all(|r| !matches!(r.kind, RowKind::Missing { .. }))
        );
        // ... and the match can be undone again.
        s.set_match("Face/mouth", None, &d);
        assert!(
            s.rows()
                .iter()
                .any(|r| r.kind == RowKind::Missing { remove: true })
        );

        s.confirm(&mut d).unwrap();
        assert!(d.dispatcher.model().meshes().find("Face/nose").is_none());
        assert!(d.dispatcher.model().meshes().find("Face/mouth").is_some());
        // One undo reverts the whole reimport.
        assert!(d.dispatcher.undo());
        assert!(d.dispatcher.model().meshes().find("Face/nose").is_some());
        assert!(d.dispatcher.model().meshes().find("Face/mouth").is_none());
    }

    #[test]
    fn polling_picks_up_a_background_read() {
        let dir = scratch("poll");
        let d = doc(&dir);
        let mut s = PsdImport::new();
        s.path = psd(&dir, "a.psd", vec![layer("bg", 8, 1)]);
        s.start();
        assert_eq!(s.stage, Stage::Loading);
        for _ in 0..500 {
            if s.poll(&d) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(s.stage, Stage::Review);
        assert_eq!(s.rows().len(), 1);
    }
}
