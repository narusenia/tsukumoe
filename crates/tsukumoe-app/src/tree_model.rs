// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the parts and deformer panels show, without any drawing: the rows
//! (open and closed folders, kind and name filters, a sort order), moving the
//! selection with the keys, and ranges of rows. The state is view state
//! (ADR-0019): none of it is in the model or in the saved workspace.

use std::collections::BTreeSet;

use tsukumoe_core::arena::Id;
use tsukumoe_core::elements::Part;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::DeformerRef;

use crate::actions::Tool;
use crate::view::Selection;

/// The two panels that list the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// Parts nested by parent, with their meshes.
    Parts,
    /// Deformers nested by parent, with the meshes hanging from them.
    Deformers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Part,
    Mesh,
    Warp,
    Rotation,
}

impl Panel {
    /// The kinds the panel lists, for its filter toggles.
    pub fn kinds(self) -> &'static [Kind] {
        match self {
            Panel::Parts => &[Kind::Part, Kind::Mesh],
            Panel::Deformers => &[Kind::Warp, Kind::Rotation, Kind::Mesh],
        }
    }
}

/// The order of the children of a folder; the model keeps its own order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Model,
    Name,
    /// Meshes by draw order, back to front; the rest stays in model order.
    Draw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    pub kind: Kind,
    pub name: String,
    /// The row's own visible flag; parts and meshes only.
    pub visible: Option<bool>,
    /// Hidden itself or through a hidden part above it.
    pub dimmed: bool,
    /// The deformer this mesh or deformer hangs from.
    pub parent: Option<String>,
    /// The part the element belongs to (shown beside deformer rows).
    pub part: Option<String>,
    /// Has something below it that a folder arrow can open.
    pub expandable: bool,
    /// Its children are listed.
    pub open: bool,
}

impl Row {
    /// What the row stands for in the selection.
    pub fn selection(&self) -> Selection {
        let name = self.name.clone();
        match self.kind {
            Kind::Part => Selection::Part(name),
            Kind::Mesh => Selection::Mesh(name),
            Kind::Warp => Selection::Warp(name),
            Kind::Rotation => Selection::Rotation(name),
        }
    }

    /// Identifies the row for the open / closed state.
    pub fn key(&self) -> String {
        key_of(self.kind, &self.name)
    }
}

pub fn key_of(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Part => format!("p:{name}"),
        Kind::Mesh => format!("m:{name}"),
        Kind::Warp | Kind::Rotation => format!("d:{name}"),
    }
}

/// A name being edited in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renaming {
    pub kind: Kind,
    pub name: String,
    pub text: String,
    /// The field has taken the keyboard.
    pub focused: bool,
}

impl Renaming {
    pub fn new(kind: Kind, name: &str) -> Self {
        Self {
            kind,
            name: name.to_owned(),
            text: name.to_owned(),
            focused: false,
        }
    }
}

/// The state of one panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeView {
    pub filter: String,
    /// The kinds to show; none chosen means all.
    pub kinds: BTreeSet<Kind>,
    /// Show only what is hidden (itself, or through a hidden part).
    pub hidden_only: bool,
    pub sort: Sort,
    /// Folders that are closed, by [`Row::key`]. A folder not listed is open.
    pub collapsed: BTreeSet<String>,
    /// The selection the panel last showed, to tell a change made elsewhere
    /// (the viewport, a menu) from a click on one of its rows.
    pub seen: Option<Selection>,
    /// Scroll offset and height as of the last frame.
    pub scroll: (f32, f32),
    pub renaming: Option<Renaming>,
}

impl TreeView {
    /// A filter is on: the rows are the matches and what leads to them.
    pub fn filtering(&self) -> bool {
        !self.filter.trim().is_empty() || !self.kinds.is_empty() || self.hidden_only
    }

    pub fn toggle_kind(&mut self, kind: Kind) {
        if !self.kinds.remove(&kind) {
            self.kinds.insert(kind);
        }
    }

    pub fn is_open(&self, key: &str) -> bool {
        !self.collapsed.contains(key)
    }

    pub fn set_open(&mut self, key: &str, open: bool) {
        if open {
            self.collapsed.remove(key);
        } else {
            self.collapsed.insert(key.to_owned());
        }
    }

    /// Opens every folder.
    pub fn open_all(&mut self) {
        self.collapsed.clear();
    }

    /// Closes every folder of the panel (with the rows as they would be unfiltered).
    pub fn close_all(&mut self, model: &Model, panel: Panel) {
        let all = TreeView::default();
        self.collapsed = rows(model, &all, panel)
            .iter()
            .filter(|r| r.expandable)
            .map(Row::key)
            .collect();
    }

    /// Opens the folders above `selection`.
    pub fn open_to(&mut self, model: &Model, panel: Panel, selection: &Selection) {
        let all = TreeView::default();
        let rows = rows(model, &all, panel);
        let Some(at) = rows.iter().position(|r| r.selection() == *selection) else {
            return;
        };
        let mut depth = rows[at].depth;
        for r in rows[..at].iter().rev() {
            if r.depth < depth {
                self.collapsed.remove(&r.key());
                depth = r.depth;
            }
        }
    }
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Part => "part",
            Kind::Mesh => "mesh",
            Kind::Warp => "warp",
            Kind::Rotation => "rotation",
        }
    }
}

impl Sort {
    fn word(self) -> &'static str {
        match self {
            Sort::Model => "model",
            Sort::Name => "name",
            Sort::Draw => "draw",
        }
    }
}

impl TreeView {
    /// The part of the state that is saved with the workspace: the closed
    /// folders, the filters and the sort. Not the selection, the scroll
    /// position or a name being typed.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "filter": self.filter,
            "kinds": self.kinds.iter().map(|k| k.word()).collect::<Vec<_>>(),
            "hidden_only": self.hidden_only,
            "sort": self.sort.word(),
            "collapsed": self.collapsed.iter().collect::<Vec<_>>(),
        })
    }

    /// Reads what [`Self::to_json`] wrote. Each field is read on its own: one
    /// that is missing, of the wrong type or holding a word that is not known
    /// gives its default and leaves the others alone.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let mut view = TreeView::default();
        if let Some(f) = value["filter"].as_str() {
            view.filter = f.to_owned();
        }
        if let Some(kinds) = value["kinds"].as_array() {
            view.kinds = kinds
                .iter()
                .filter_map(|k| k.as_str())
                .filter_map(|w| {
                    [Kind::Part, Kind::Mesh, Kind::Warp, Kind::Rotation]
                        .into_iter()
                        .find(|k| k.word() == w)
                })
                .collect();
        }
        if let Some(h) = value["hidden_only"].as_bool() {
            view.hidden_only = h;
        }
        if let Some(sort) = value["sort"].as_str().and_then(|w| {
            [Sort::Model, Sort::Name, Sort::Draw]
                .into_iter()
                .find(|s| s.word() == w)
        }) {
            view.sort = sort;
        }
        if let Some(keys) = value["collapsed"].as_array() {
            view.collapsed = keys
                .iter()
                .filter_map(|k| k.as_str())
                .filter(|k| k.starts_with("p:") || k.starts_with("d:") || k.starts_with("m:"))
                .map(str::to_owned)
                .collect();
        }
        view
    }

    /// Forgets closed folders that no longer exist (renamed or deleted since).
    pub fn prune(&mut self, model: &Model, panel: Panel) {
        let all = TreeView::default();
        let live: BTreeSet<String> = rows(model, &all, panel)
            .iter()
            .filter(|r| r.expandable)
            .map(Row::key)
            .collect();
        self.collapsed.retain(|k| live.contains(k));
    }

    /// Opens or closes `key` and every folder below it in `panel`.
    pub fn set_subtree_open(&mut self, model: &Model, panel: Panel, key: &str, open: bool) {
        let all = TreeView::default();
        let rows = rows(model, &all, panel);
        let Some(at) = rows.iter().position(|r| r.key() == key) else {
            return;
        };
        let depth = rows[at].depth;
        for r in rows[at..]
            .iter()
            .enumerate()
            .take_while(|(i, r)| *i == 0 || r.depth > depth)
            .map(|(_, r)| r)
        {
            if r.expandable {
                self.set_open(&r.key(), open);
            }
        }
    }
}

pub fn matches(name: &str, filter: &str) -> bool {
    filter.trim().is_empty() || name.to_lowercase().contains(&filter.trim().to_lowercase())
}

struct Builder<'a> {
    model: &'a Model,
    view: &'a TreeView,
    filtering: bool,
}

/// The rows of `panel`, in tree order, with folders opened or closed, the
/// filters applied and the children sorted.
pub fn rows(model: &Model, view: &TreeView, panel: Panel) -> Vec<Row> {
    let b = Builder {
        model,
        view,
        filtering: view.filtering(),
    };
    let mut out = Vec::new();
    match panel {
        Panel::Parts => {
            for (id, _, part) in model.parts().iter() {
                if part.parent.is_none_or(|p| !model.parts().contains(p)) {
                    b.part(id, 0, false, &mut out);
                }
            }
        }
        Panel::Deformers => {
            for r in b.sorted_deformers(|d| d.is_none()) {
                b.deformer(r, 0, &mut out);
            }
        }
    }
    out
}

impl Builder<'_> {
    fn kind_on(&self, kind: Kind) -> bool {
        self.view.kinds.is_empty() || self.view.kinds.contains(&kind)
    }

    /// Whether a row matches the filters (always, when none is on).
    fn wanted(&self, kind: Kind, name: &str, dimmed: bool) -> bool {
        !self.filtering
            || (self.kind_on(kind)
                && matches(name, &self.view.filter)
                && (!self.view.hidden_only || dimmed))
    }

    fn part_hidden(&self, part: Id<Part>) -> bool {
        let mut at = Some(part);
        for _ in 0..64 {
            let Some(id) = at else { return false };
            let Some(p) = self.model.parts().get(id) else {
                return false;
            };
            if !p.visible {
                return true;
            }
            at = p.parent;
        }
        false
    }

    fn part_name(&self, part: Id<Part>) -> Option<String> {
        self.model.parts().name(part).map(str::to_owned)
    }

    fn sort_names(&self, mut items: Vec<(String, f32)>) -> Vec<String> {
        match self.view.sort {
            Sort::Model => {}
            Sort::Name => items.sort_by_key(|(n, _)| n.to_lowercase()),
            Sort::Draw => items.sort_by(|a, b| a.1.total_cmp(&b.1)),
        }
        items.into_iter().map(|(n, _)| n).collect()
    }

    /// Finishes a folder: its rows are kept when it or something under it
    /// matches; the children show when it is open, or when a filter found
    /// something under it.
    #[allow(clippy::too_many_arguments)]
    fn folder(
        &self,
        row: Row,
        own: bool,
        children: Vec<Row>,
        has_children: bool,
        out: &mut Vec<Row>,
    ) -> bool {
        let found = !children.is_empty() && self.filtering;
        let any = own || found || !self.filtering;
        if !any {
            return false;
        }
        let open = if self.filtering {
            found || self.view.is_open(&row.key())
        } else {
            self.view.is_open(&row.key())
        };
        out.push(Row {
            expandable: has_children,
            open: open && has_children,
            ..row
        });
        if open {
            out.extend(children);
        }
        true
    }

    fn mesh_row(&self, name: &str, depth: usize, hidden: bool, with_part: bool) -> Option<Row> {
        let id = self.model.meshes().find(name)?;
        let m = self.model.meshes().get(id)?;
        let dimmed = hidden || !m.visible || self.part_hidden(m.part);
        self.wanted(Kind::Mesh, name, dimmed).then(|| Row {
            depth,
            kind: Kind::Mesh,
            name: name.to_owned(),
            visible: Some(m.visible),
            dimmed,
            parent: m
                .parent
                .and_then(|r| crate::deformer_tool::name_of(self.model, r)),
            part: with_part.then(|| self.part_name(m.part)).flatten(),
            expandable: false,
            open: false,
        })
    }

    fn part(&self, part: Id<Part>, depth: usize, hidden_above: bool, out: &mut Vec<Row>) -> bool {
        let name = self.part_name(part).unwrap_or_default();
        let own_visible = self.model.parts().get(part).is_none_or(|p| p.visible);
        let hidden = hidden_above || !own_visible;
        let mut children = Vec::new();
        let child_parts: Vec<(String, f32)> = self
            .model
            .parts()
            .iter()
            .filter(|(_, _, p)| p.parent == Some(part))
            .map(|(_, n, _)| (n.to_owned(), 0.0))
            .collect();
        let child_parts = match self.view.sort {
            Sort::Name => self.sort_names(child_parts),
            _ => child_parts.into_iter().map(|(n, _)| n).collect(),
        };
        let mut has_children = false;
        for n in child_parts {
            if let Some(id) = self.model.parts().find(&n) {
                has_children = true;
                self.part(id, depth + 1, hidden, &mut children);
            }
        }
        let meshes: Vec<(String, f32)> = self
            .model
            .meshes()
            .iter()
            .filter(|(_, _, m)| m.part == part)
            .map(|(_, n, m)| (n.to_owned(), m.draw_order))
            .collect();
        has_children |= !meshes.is_empty();
        for n in self.sort_names(meshes) {
            children.extend(self.mesh_row(&n, depth + 1, hidden, false));
        }
        let row = Row {
            depth,
            kind: Kind::Part,
            name: name.clone(),
            visible: Some(own_visible),
            dimmed: hidden,
            parent: None,
            part: None,
            expandable: false,
            open: false,
        };
        let own = self.wanted(Kind::Part, &name, hidden);
        self.folder(row, own, children, has_children, out)
    }

    fn deformer_info(
        &self,
        r: DeformerRef,
    ) -> Option<(String, Kind, Option<DeformerRef>, Id<Part>)> {
        let name = crate::deformer_tool::name_of(self.model, r)?;
        Some(match r {
            DeformerRef::Warp(id) => {
                let d = self.model.warps().get(id)?;
                (name, Kind::Warp, d.parent, d.part)
            }
            DeformerRef::Rotation(id) => {
                let d = self.model.rotations().get(id)?;
                (name, Kind::Rotation, d.parent, d.part)
            }
        })
    }

    /// The deformers whose parent satisfies `parent`, sorted.
    fn sorted_deformers(&self, parent: impl Fn(Option<DeformerRef>) -> bool) -> Vec<DeformerRef> {
        let mut all: Vec<(String, DeformerRef)> = self
            .model
            .warps()
            .iter()
            .filter(|(_, _, d)| parent(d.parent))
            .map(|(id, n, _)| (n.to_owned(), DeformerRef::Warp(id)))
            .chain(
                self.model
                    .rotations()
                    .iter()
                    .filter(|(_, _, d)| parent(d.parent))
                    .map(|(id, n, _)| (n.to_owned(), DeformerRef::Rotation(id))),
            )
            .collect();
        if self.view.sort == Sort::Name {
            all.sort_by_key(|(n, _)| n.to_lowercase());
        }
        all.into_iter().map(|(_, r)| r).collect()
    }

    fn deformer(&self, r: DeformerRef, depth: usize, out: &mut Vec<Row>) -> bool {
        let Some((name, kind, parent, part)) = self.deformer_info(r) else {
            return false;
        };
        let hidden = self.part_hidden(part);
        let mut children = Vec::new();
        let kids = self.sorted_deformers(|p| p == Some(r));
        let mut has_children = !kids.is_empty();
        for k in kids {
            self.deformer(k, depth + 1, &mut children);
        }
        let meshes: Vec<(String, f32)> = self
            .model
            .meshes()
            .iter()
            .filter(|(_, _, m)| m.parent == Some(r))
            .map(|(_, n, m)| (n.to_owned(), m.draw_order))
            .collect();
        has_children |= !meshes.is_empty();
        for n in self.sort_names(meshes) {
            children.extend(self.mesh_row(&n, depth + 1, hidden, true));
        }
        let row = Row {
            depth,
            kind,
            name: name.clone(),
            visible: None,
            dimmed: hidden,
            parent: parent.and_then(|p| crate::deformer_tool::name_of(self.model, p)),
            part: self.part_name(part),
            expandable: false,
            open: false,
        };
        let own = self.wanted(kind, &name, hidden);
        self.folder(row, own, children, has_children, out)
    }
}

/// The meshes under a part, with the parts below it.
pub fn meshes_under_part(model: &Model, part: &str) -> Vec<String> {
    let Some(root) = model.parts().find(part) else {
        return Vec::new();
    };
    let mut parts = vec![root];
    let mut i = 0;
    while i < parts.len() {
        let at = parts[i];
        let below: Vec<_> = model
            .parts()
            .iter()
            .filter(|(id, _, p)| p.parent == Some(at) && !parts.contains(id))
            .map(|(id, _, _)| id)
            .collect();
        parts.extend(below);
        i += 1;
    }
    model
        .meshes()
        .iter()
        .filter(|(_, _, m)| parts.contains(&m.part))
        .map(|(_, n, _)| n.to_owned())
        .collect()
}

/// The row to bring into view: the one of `selection` when it changed since
/// the panel last saw it (a change made elsewhere). `None` when it did not
/// change, or when the row is not among `rows` (hidden by a filter, or not a row).
pub fn scroll_row(
    rows: &[Row],
    selection: Option<&Selection>,
    seen: Option<&Selection>,
) -> Option<usize> {
    let selection = selection?;
    if Some(selection) == seen {
        return None;
    }
    rows.iter().position(|r| r.selection() == *selection)
}

/// The scroll offset that shows row `index` of `total` (each `row_height`
/// tall) in a panel `viewport` tall now scrolled to `offset`: `None` if the
/// row is fully in view already, else the row centred, within the content.
pub fn scroll_offset_for(
    index: usize,
    total: usize,
    row_height: f32,
    offset: f32,
    viewport: f32,
) -> Option<f32> {
    let (top, bottom) = (index as f32 * row_height, (index + 1) as f32 * row_height);
    if viewport > 0.0 && top >= offset && bottom <= offset + viewport {
        return None;
    }
    let content = total as f32 * row_height;
    let centred = top - (viewport - row_height).max(0.0) / 2.0;
    Some(centred.clamp(0.0, (content - viewport).max(0.0)))
}

/// Whether a click may add `kind` to a multiple selection under `tool`: the
/// mesh tool works on meshes, the deformer tool on deformers and meshes.
pub fn accepted(tool: Tool, kind: Kind) -> bool {
    match tool {
        Tool::Select => true,
        Tool::Mesh => kind == Kind::Mesh,
        Tool::Deformer => kind != Kind::Part,
    }
}

/// The selections from `anchor` to `target` among `rows` (a Shift click),
/// ending with `target`, of the kinds `tool` takes. Without the anchor among
/// the rows, just `target`.
pub fn range_selection(
    rows: &[Row],
    anchor: Option<&Selection>,
    target: &Selection,
    tool: Tool,
) -> Vec<Selection> {
    let (Some(a), Some(t)) = (
        anchor.and_then(|a| rows.iter().position(|r| r.selection() == *a)),
        rows.iter().position(|r| r.selection() == *target),
    ) else {
        return vec![target.clone()];
    };
    let mut range: Vec<Selection> = rows[a.min(t)..=a.max(t)]
        .iter()
        .filter(|r| accepted(tool, r.kind))
        .map(Row::selection)
        .collect();
    if t < a {
        range.reverse();
    }
    // The target is always in, and last.
    range.retain(|s| s != target);
    range.push(target.clone());
    range
}

/// A key the panel handles while it has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
}

/// What a key does to the rows: the selection it moves to, and the folder it
/// opens or closes. `selected` is the row the keys start from.
pub fn navigate(
    rows: &[Row],
    selected: Option<&Selection>,
    key: NavKey,
    view: &mut TreeView,
) -> Option<Selection> {
    let at = selected.and_then(|s| rows.iter().position(|r| r.selection() == *s));
    let Some(at) = at else {
        // Nothing selected (or hidden): a vertical key starts at the ends.
        return match key {
            NavKey::Down => rows.first().map(Row::selection),
            NavKey::Up => rows.last().map(Row::selection),
            _ => None,
        };
    };
    let row = &rows[at];
    match key {
        NavKey::Down => rows.get(at + 1).map(Row::selection),
        NavKey::Up => at.checked_sub(1).map(|i| rows[i].selection()),
        NavKey::Right => {
            if row.expandable && !row.open {
                view.set_open(&row.key(), true);
                None
            } else if row.expandable {
                rows.get(at + 1).map(Row::selection)
            } else {
                None
            }
        }
        NavKey::Left => {
            if row.expandable && row.open {
                view.set_open(&row.key(), false);
                None
            } else {
                rows[..at]
                    .iter()
                    .rev()
                    .find(|r| r.depth < row.depth)
                    .map(Row::selection)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::{Command, RotationBase};

    fn mesh(name: &str, part: &str) -> Command {
        let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        Command::AddMesh {
            name: name.into(),
            part: part.into(),
            positions: tri.clone(),
            uvs: tri,
            indices: vec![0, 1, 2],
        }
    }

    fn part(name: &str, parent: Option<&str>) -> Command {
        Command::AddPart {
            name: name.into(),
            parent: parent.map(Into::into),
        }
    }

    /// Body > Head > Hair > HairFront, Body > Arm; Face with warp W > rotation R,
    /// meshes Skin (no deformer), Eye (under R), Mouth (under W); Far (part Arm) under W.
    fn model() -> Model {
        let mut m = Model::new();
        let base = RotationBase {
            origin: [0.0, 0.0],
            angle_degrees: 0.0,
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
        };
        let commands = vec![
            part("Body", None),
            part("Head", Some("Body")),
            part("Hair", Some("Head")),
            part("Arm", Some("Body")),
            part("Face", None),
            mesh("HairFront", "Hair"),
            mesh("Skin", "Face"),
            mesh("Eye", "Face"),
            mesh("Mouth", "Face"),
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Face".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            },
            Command::AddRotationDeformer {
                name: "R".into(),
                part: "Face".into(),
                parent: Some("W".into()),
                base,
            },
            Command::AddRotationDeformer {
                name: "Far".into(),
                part: "Arm".into(),
                parent: Some("W".into()),
                base,
            },
            Command::SetMeshParent {
                name: "Eye".into(),
                parent: Some("R".into()),
            },
            Command::SetMeshParent {
                name: "Mouth".into(),
                parent: Some("W".into()),
            },
        ];
        for c in commands {
            m.apply(c).unwrap();
        }
        m
    }

    fn names(rows: &[Row]) -> Vec<(usize, &str)> {
        rows.iter().map(|r| (r.depth, r.name.as_str())).collect()
    }

    #[test]
    fn the_parts_panel_nests_parts_and_lists_all_their_meshes_without_deformers() {
        let m = model();
        let r = rows(&m, &TreeView::default(), Panel::Parts);
        assert_eq!(
            names(&r),
            [
                (0, "Body"),
                (1, "Head"),
                (2, "Hair"),
                (3, "HairFront"),
                (1, "Arm"),
                (0, "Face"),
                (1, "Skin"),
                (1, "Eye"),
                (1, "Mouth"),
            ]
        );
        assert!(
            r.iter()
                .all(|x| !matches!(x.kind, Kind::Warp | Kind::Rotation))
        );
        let body = &r[0];
        assert!(body.expandable && body.open);
        assert!(!r.iter().find(|x| x.name == "Arm").unwrap().expandable);
    }

    #[test]
    fn the_deformers_panel_nests_deformers_with_their_meshes_and_shows_the_part() {
        let m = model();
        let r = rows(&m, &TreeView::default(), Panel::Deformers);
        assert_eq!(
            names(&r),
            [(0, "W"), (1, "R"), (2, "Eye"), (1, "Far"), (1, "Mouth")]
        );
        let get = |n: &str| r.iter().find(|x| x.name == n).unwrap();
        assert_eq!(get("W").part.as_deref(), Some("Face"));
        assert_eq!(get("Far").part.as_deref(), Some("Arm"));
        assert_eq!(get("Eye").part.as_deref(), Some("Face"));
        assert_eq!(get("Eye").parent.as_deref(), Some("R"));
        assert_eq!(get("W").kind, Kind::Warp);
        assert_eq!(get("R").kind, Kind::Rotation);
        // A mesh with no deformer is not listed.
        assert!(r.iter().all(|x| x.name != "Skin"));
    }

    #[test]
    fn folders_open_and_close_and_a_closed_one_hides_what_is_under_it() {
        let m = model();
        let mut v = TreeView::default();
        v.set_open("p:Head", false);
        let r = rows(&m, &v, Panel::Parts);
        assert_eq!(
            names(&r)[..4],
            [(0, "Body"), (1, "Head"), (1, "Arm"), (0, "Face")]
        );
        let head = r.iter().find(|x| x.name == "Head").unwrap();
        assert!(head.expandable && !head.open);
        v.close_all(&m, Panel::Parts);
        let r = rows(&m, &v, Panel::Parts);
        assert_eq!(names(&r), [(0, "Body"), (0, "Face")]);
        v.open_all();
        assert_eq!(rows(&m, &v, Panel::Parts).len(), 9);
        // Reveal a selection: only the folders above it open.
        v.close_all(&m, Panel::Parts);
        v.open_to(&m, Panel::Parts, &Selection::Mesh("HairFront".into()));
        let r = rows(&m, &v, Panel::Parts);
        assert_eq!(
            names(&r),
            [
                (0, "Body"),
                (1, "Head"),
                (2, "Hair"),
                (3, "HairFront"),
                (1, "Arm"),
                (0, "Face")
            ]
        );
    }

    #[test]
    fn a_name_filter_opens_the_folders_above_matches_and_releasing_it_restores_the_state() {
        let m = model();
        let mut v = TreeView::default();
        v.close_all(&m, Panel::Parts);
        v.filter = "front".into();
        assert!(v.filtering());
        let r = rows(&m, &v, Panel::Parts);
        assert_eq!(
            names(&r),
            [(0, "Body"), (1, "Head"), (2, "Hair"), (3, "HairFront")]
        );
        assert!(r.iter().all(|x| !x.expandable || x.open));
        v.filter.clear();
        assert_eq!(
            names(&rows(&m, &v, Panel::Parts)),
            [(0, "Body"), (0, "Face")]
        );
    }

    #[test]
    fn kind_and_hidden_filters_combine_with_the_name_and_with_each_other() {
        let mut m = model();
        let mut v = TreeView::default();
        v.toggle_kind(Kind::Mesh);
        let r = rows(&m, &v, Panel::Parts);
        // Meshes only; the parts that lead to them stay as structure.
        assert_eq!(r.iter().filter(|x| x.kind == Kind::Mesh).count(), 4);
        assert!(r.iter().any(|x| x.name == "Head"));
        assert!(!r.iter().any(|x| x.name == "Arm"), "no mesh under Arm");
        v.toggle_kind(Kind::Mesh);
        v.toggle_kind(Kind::Part);
        let r = rows(&m, &v, Panel::Parts);
        assert!(r.iter().all(|x| x.kind == Kind::Part));
        assert!(r.iter().any(|x| x.name == "Arm"));
        // Hidden only, with the name: AND.
        v.toggle_kind(Kind::Part);
        m.apply(Command::SetMeshVisible {
            name: "Eye".into(),
            visible: false,
        })
        .unwrap();
        m.apply(Command::SetPartVisible {
            name: "Head".into(),
            visible: false,
        })
        .unwrap();
        v.hidden_only = true;
        let r = rows(&m, &v, Panel::Parts);
        let shown: Vec<&str> = r.iter().map(|x| x.name.as_str()).collect();
        assert!(shown.contains(&"Eye") && shown.contains(&"Hair") && shown.contains(&"HairFront"));
        assert!(!shown.contains(&"Skin") && !shown.contains(&"Arm"));
        v.filter = "eye".into();
        assert_eq!(
            rows(&m, &v, Panel::Parts)
                .iter()
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>(),
            ["Face", "Eye"]
        );
        // The deformers panel has its own kinds.
        let mut d = TreeView::default();
        d.toggle_kind(Kind::Rotation);
        let r = rows(&m, &d, Panel::Deformers);
        assert_eq!(names(&r), [(0, "W"), (1, "R"), (1, "Far")]);
        assert_eq!(
            Panel::Deformers.kinds(),
            [Kind::Warp, Kind::Rotation, Kind::Mesh]
        );
    }

    #[test]
    fn the_sort_changes_the_order_of_the_children_and_nothing_else() {
        let mut m = model();
        m.apply(Command::SetMeshDrawOrder {
            name: "Skin".into(),
            draw_order: 700.0,
        })
        .unwrap();
        m.apply(Command::SetMeshDrawOrder {
            name: "Eye".into(),
            draw_order: 300.0,
        })
        .unwrap();
        let face = |sort: Sort| {
            let v = TreeView {
                sort,
                ..TreeView::default()
            };
            rows(&m, &v, Panel::Parts)
                .iter()
                .skip_while(|x| x.name != "Face")
                .map(|x| x.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(face(Sort::Model), ["Face", "Skin", "Eye", "Mouth"]);
        assert_eq!(face(Sort::Name), ["Face", "Eye", "Mouth", "Skin"]);
        // Back to front: Eye 300, Mouth 500, Skin 700.
        assert_eq!(face(Sort::Draw), ["Face", "Eye", "Mouth", "Skin"]);
        // The model itself is as it was.
        assert_eq!(
            m.meshes().iter().next().map(|(_, n, _)| n),
            Some("HairFront")
        );
        let by_name = TreeView {
            sort: Sort::Name,
            ..TreeView::default()
        };
        let d = rows(&m, &by_name, Panel::Deformers);
        assert_eq!(names(&d)[..2], [(0, "W"), (1, "Far")]);
    }

    #[test]
    fn a_range_runs_over_the_rows_shown_and_mixes_meshes_and_deformers() {
        let m = model();
        let r = rows(&m, &TreeView::default(), Panel::Deformers);
        let sel = |n: &str| r.iter().find(|x| x.name == n).unwrap().selection();
        let range = range_selection(&r, Some(&sel("R")), &sel("Mouth"), Tool::Select);
        assert_eq!(range, [sel("R"), sel("Eye"), sel("Far"), sel("Mouth")]);
        // Upwards: the target is last.
        let up = range_selection(&r, Some(&sel("Mouth")), &sel("R"), Tool::Select);
        assert_eq!(up.last(), Some(&sel("R")));
        assert_eq!(up.len(), 4);
        // The mesh tool takes meshes only; the target is always in.
        let meshes = range_selection(&r, Some(&sel("R")), &sel("Mouth"), Tool::Mesh);
        assert_eq!(meshes, [sel("Eye"), sel("Mouth")]);
        let to_deformer = range_selection(&r, Some(&sel("Eye")), &sel("Far"), Tool::Mesh);
        assert_eq!(to_deformer, [sel("Eye"), sel("Far")]);
        // The deformer tool leaves parts out.
        assert!(accepted(Tool::Deformer, Kind::Mesh) && !accepted(Tool::Deformer, Kind::Part));
        // No anchor, or one not shown: the target alone.
        assert_eq!(
            range_selection(&r, None, &sel("Far"), Tool::Select),
            [sel("Far")]
        );
        assert_eq!(
            range_selection(
                &r,
                Some(&Selection::Mesh("Gone".into())),
                &sel("Far"),
                Tool::Select
            ),
            [sel("Far")]
        );
    }

    #[test]
    fn the_keys_move_the_selection_and_open_and_close_folders() {
        let m = model();
        let mut v = TreeView::default();
        let r = rows(&m, &v, Panel::Parts);
        let part = |n: &str| Selection::Part(n.into());
        // Nothing selected: Down starts at the top, Up at the bottom.
        assert_eq!(navigate(&r, None, NavKey::Down, &mut v), Some(part("Body")));
        assert_eq!(
            navigate(&r, None, NavKey::Up, &mut v),
            Some(Selection::Mesh("Mouth".into()))
        );
        assert_eq!(navigate(&r, None, NavKey::Left, &mut v), None);
        // Down and Up move by one row and stop at the ends.
        assert_eq!(
            navigate(&r, Some(&part("Body")), NavKey::Down, &mut v),
            Some(part("Head"))
        );
        assert_eq!(navigate(&r, Some(&part("Body")), NavKey::Up, &mut v), None);
        // Left on an open folder closes it; Left again goes to the parent.
        assert_eq!(
            navigate(&r, Some(&part("Head")), NavKey::Left, &mut v),
            None
        );
        assert!(!v.is_open("p:Head"));
        let r = rows(&m, &v, Panel::Parts);
        assert_eq!(
            navigate(&r, Some(&part("Head")), NavKey::Left, &mut v),
            Some(part("Body"))
        );
        // Right opens a closed folder; on an open one it goes to the first child.
        assert_eq!(
            navigate(&r, Some(&part("Head")), NavKey::Right, &mut v),
            None
        );
        assert!(v.is_open("p:Head"));
        let r = rows(&m, &v, Panel::Parts);
        assert_eq!(
            navigate(&r, Some(&part("Head")), NavKey::Right, &mut v),
            Some(part("Hair"))
        );
        // Right on a leaf does nothing.
        let leaf = Selection::Mesh("HairFront".into());
        assert_eq!(navigate(&r, Some(&leaf), NavKey::Right, &mut v), None);
        // A selection the rows do not show behaves as none.
        assert_eq!(
            navigate(&r, Some(&part("Gone")), NavKey::Down, &mut v),
            Some(part("Body"))
        );
    }

    #[test]
    fn the_row_to_scroll_to_follows_a_selection_made_elsewhere() {
        let m = model();
        let r = rows(&m, &TreeView::default(), Panel::Parts);
        let eye = Selection::Mesh("Eye".into());
        let index = r.iter().position(|x| x.name == "Eye").unwrap();
        assert_eq!(scroll_row(&r, Some(&eye), None), Some(index));
        assert_eq!(
            scroll_row(&r, Some(&eye), Some(&Selection::Mesh("Skin".into()))),
            Some(index)
        );
        assert_eq!(scroll_row(&r, Some(&eye), Some(&eye)), None);
        assert_eq!(scroll_row(&r, None, Some(&eye)), None);
        let hidden = rows(
            &m,
            &TreeView {
                filter: "sk".into(),
                ..TreeView::default()
            },
            Panel::Parts,
        );
        assert_eq!(scroll_row(&hidden, Some(&eye), None), None);
        assert_eq!(
            scroll_row(&r, Some(&Selection::Parameter("P".into())), None),
            None
        );
    }

    #[test]
    fn the_offset_is_only_set_when_the_row_is_not_fully_in_view() {
        let h = 26.0;
        let (offset, view) = (260.0, 260.0);
        assert_eq!(scroll_offset_for(10, 100, h, offset, view), None);
        assert_eq!(scroll_offset_for(19, 100, h, offset, view), None);
        assert!(scroll_offset_for(9, 100, h, offset, view).is_some());
        assert!(scroll_offset_for(20, 100, h, offset, view).is_some());
        let to = scroll_offset_for(60, 100, h, offset, view).unwrap();
        let top = 60.0 * h - to;
        assert!(top > 0.0 && top + h < view, "{top}");
        assert_eq!(scroll_offset_for(0, 100, h, offset, view), Some(0.0));
        let last = scroll_offset_for(99, 100, h, 0.0, view).unwrap();
        assert!((last - (100.0 * h - view)).abs() < 1e-3);
        assert!(scroll_offset_for(3, 100, h, 0.0, 0.0).is_some());
    }

    #[test]
    fn meshes_under_a_part_include_the_parts_below_it() {
        let m = model();
        assert_eq!(meshes_under_part(&m, "Body"), ["HairFront"]);
        assert_eq!(meshes_under_part(&m, "Face"), ["Skin", "Eye", "Mouth"]);
        assert!(meshes_under_part(&m, "Nope").is_empty());
    }

    #[test]
    fn the_saved_part_of_the_state_round_trips_and_leaves_out_the_rest() {
        let mut v = TreeView {
            filter: "eye".into(),
            ..TreeView::default()
        };
        v.toggle_kind(Kind::Mesh);
        v.toggle_kind(Kind::Part);
        v.hidden_only = true;
        v.sort = Sort::Draw;
        v.set_open("p:Head", false);
        v.set_open("d:W", false);
        v.seen = Some(Selection::Mesh("x".into()));
        v.scroll = (10.0, 20.0);
        v.renaming = Some(Renaming::new(Kind::Mesh, "x"));
        let back = TreeView::from_json(&v.to_json());
        assert_eq!(back.filter, "eye");
        assert_eq!(back.kinds, v.kinds);
        assert!(back.hidden_only);
        assert_eq!(back.sort, Sort::Draw);
        assert_eq!(back.collapsed, v.collapsed);
        assert!(back.seen.is_none() && back.renaming.is_none() && back.scroll == (0.0, 0.0));
        assert_eq!(
            TreeView::from_json(&TreeView::default().to_json()),
            TreeView::default()
        );
    }

    #[test]
    fn damaged_or_foreign_data_gives_defaults_field_by_field() {
        use serde_json::json;
        // The wrong type for one field does not spoil the others.
        let v = TreeView::from_json(&json!({
            "filter": 7,
            "kinds": ["mesh", "bogus", 3],
            "hidden_only": "yes",
            "sort": "name",
            "collapsed": ["p:Head", 5, "nonsense", "d:W"],
        }));
        assert_eq!(v.filter, "");
        assert_eq!(v.kinds, BTreeSet::from([Kind::Mesh]));
        assert!(!v.hidden_only);
        assert_eq!(v.sort, Sort::Name);
        assert_eq!(
            v.collapsed,
            BTreeSet::from(["p:Head".to_owned(), "d:W".to_owned()])
        );
        assert_eq!(TreeView::from_json(&json!(null)), TreeView::default());
        assert_eq!(TreeView::from_json(&json!([1, 2])), TreeView::default());
        assert_eq!(
            TreeView::from_json(&json!({"sort": "sideways"})).sort,
            Sort::Model
        );
    }

    #[test]
    fn closed_folders_that_no_longer_exist_are_forgotten() {
        let m = model();
        let mut v = TreeView::default();
        v.set_open("p:Head", false);
        v.set_open("p:Gone", false);
        v.set_open("d:Missing", false);
        v.set_open("d:W", false);
        // A closed folder that is not in this panel is not kept for it.
        v.prune(&m, Panel::Parts);
        assert_eq!(v.collapsed, BTreeSet::from(["p:Head".to_owned()]));
        let mut d = TreeView::default();
        d.set_open("d:W", false);
        d.set_open("d:Old", false);
        d.prune(&m, Panel::Deformers);
        assert_eq!(d.collapsed, BTreeSet::from(["d:W".to_owned()]));
        // Unknown names in a saved state do no harm to the rows meanwhile.
        let mut stale = TreeView::default();
        stale.set_open("p:Gone", false);
        assert_eq!(
            rows(&m, &stale, Panel::Parts).len(),
            rows(&m, &TreeView::default(), Panel::Parts).len()
        );
    }
}
