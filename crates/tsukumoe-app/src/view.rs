// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! View state: what the screen shows about the model without being part of it
//! (ADR-0019). The selection, the viewport camera and the current parameter
//! values live here, never in the `Model`, and none of it is in the undo
//! history.

use std::collections::BTreeMap;

use tsukumoe_core::elements::Parameter;

use crate::actions::Tool;
use crate::tree_model::TreeView;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Part(String),
    Mesh(String),
    Warp(String),
    Rotation(String),
    Glue(String),
    Parameter(String),
}

/// A zoom and a shift of the view that fits the model. `center` is the offset
/// of the view's center from the fitted center, in fractions of the fitted size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    pub center: [f32; 2],
}

pub const ZOOM_RANGE: (f32, f32) = (0.1, 64.0);

impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center: [0.0, 0.0],
        }
    }
}

impl Camera {
    /// The visible rectangle `[x0, y0, x1, y1]` (y up) for a fitted one.
    pub fn view(&self, fitted: [f32; 4]) -> [f32; 4] {
        let (w, h) = (fitted[2] - fitted[0], fitted[3] - fitted[1]);
        let cx = (fitted[0] + fitted[2]) / 2.0 + self.center[0] * w;
        let cy = (fitted[1] + fitted[3]) / 2.0 + self.center[1] * h;
        let (hw, hh) = (w / self.zoom / 2.0, h / self.zoom / 2.0);
        [cx - hw, cy - hh, cx + hw, cy + hh]
    }

    /// Drags the picture by `delta` points inside a viewport of `size` points.
    pub fn pan(&mut self, delta: [f32; 2], size: [f32; 2]) {
        self.center[0] -= delta[0] / size[0].max(1.0) / self.zoom;
        // Screen y grows downward, model y upward.
        self.center[1] += delta[1] / size[1].max(1.0) / self.zoom;
    }

    /// Multiplies the zoom by `factor`, keeping the model point under `anchor`
    /// (fractions from the top left of the viewport) where it is.
    pub fn zoom_at(&mut self, factor: f32, anchor: [f32; 2]) {
        let zoom = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        // In fitted-size units, relative to the fitted center.
        let ax = self.center[0] + (anchor[0] - 0.5) / self.zoom;
        let ay = self.center[1] - (anchor[1] - 0.5) / self.zoom;
        self.center[0] = ax - (anchor[0] - 0.5) / zoom;
        self.center[1] = ay + (anchor[1] - 0.5) / zoom;
        self.zoom = zoom;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Seconds the pointer rests on an item before its card shows.
pub const CARD_DELAY: f64 = 0.5;

#[derive(Clone, Debug, Default)]
pub struct ViewState {
    /// What is selected; with several, the one chosen last.
    pub selection: Option<Selection>,
    /// Only this part, mesh or deformer (and what is under it) is drawn in the
    /// viewport; nothing in the model changes. Not saved.
    pub solo: Option<Selection>,
    /// The other selected items, and the `selection` they were chosen next
    /// to: when anything else sets `selection`, they no longer count.
    others: Vec<Selection>,
    others_of: Option<Selection>,
    /// Where a Shift click range starts.
    pub anchor: Option<Selection>,
    /// What the pointer rests on in the viewport or a tree row, with the UI
    /// pass it was set in; see `set_hover`. Not saved.
    hover: Option<(Selection, u64)>,
    /// When the pointer came to rest on the hovered item (egui time).
    hover_since: f64,
    /// The texture pages for the mesh pictures; loaded by the editor. Not saved.
    pub atlas: crate::atlas::Atlas,
    /// A grid was just laid: the wheel adds `grid_steps` to its size instead of zooming. Not saved.
    pub grid_live: bool,
    pub grid_steps: Vec<i32>,
    /// Wheel travel not yet worth a step.
    pub grid_wheel: f32,
    /// The parts panel and the deformers panel.
    pub parts: TreeView,
    pub deformers: TreeView,
    pub tool: Tool,
    pub camera: Camera,
    /// Current parameter values by name; absent means the default.
    params: BTreeMap<String, f32>,
    /// Counts changes the viewport must redraw for.
    revision: u64,
    pub param_filter: String,
    /// The parameter the panel's edit section and the key buttons work on.
    pub param_focus: Option<String>,
    /// The parameter whose settings window is open. Not saved.
    pub param_dialog: Option<String>,
    /// The parameter whose keyforms "put here" moves. Not saved.
    pub param_source: Option<String>,
    /// Pairs of parameters the panel shows as one pad (first = horizontal,
    /// second = vertical). Kept with the project, see `Document::param_links`.
    pub param_links: Vec<(String, String)>,
    /// The keyform a mirror copy takes its shape from: the element and its key
    /// (one index per axis). Not saved.
    pub mirror_source: Option<(tsukumoe_core::command::ElementRef, Vec<usize>)>,
    /// The mirror line's x as typed; empty means the centre of the model.
    pub mirror_center: String,
    /// The mesh and vertices that are one side of the glue `mesh.glue` makes. Not saved.
    pub glue_source: Option<(String, std::collections::BTreeSet<u32>)>,
    /// The two axes (by position in the binding) the key grid shows as rows and
    /// columns; clamped to the element's axes when drawn. Not saved.
    pub grid_axes: (usize, usize),
    /// Proportional edit options; not saved with the project or the layout.
    pub proportional: crate::proportional::Proportional,
}

impl ViewState {
    pub fn tree(&self, panel: crate::tree_model::Panel) -> &TreeView {
        match panel {
            crate::tree_model::Panel::Parts => &self.parts,
            crate::tree_model::Panel::Deformers => &self.deformers,
        }
    }

    pub fn tree_mut(&mut self, panel: crate::tree_model::Panel) -> &mut TreeView {
        match panel {
            crate::tree_model::Panel::Parts => &mut self.parts,
            crate::tree_model::Panel::Deformers => &mut self.deformers,
        }
    }

    /// Everything selected, in the order it was chosen; the last is `selection`.
    pub fn selected(&self) -> Vec<Selection> {
        let mut out: Vec<Selection> = if self.others_of == self.selection {
            self.others.clone()
        } else {
            Vec::new()
        };
        if let Some(s) = &self.selection {
            out.retain(|o| o != s);
            out.push(s.clone());
        }
        out
    }

    /// The meshes selected, in the order they were chosen.
    pub fn selected_meshes(&self) -> Vec<String> {
        self.selected()
            .into_iter()
            .filter_map(|s| match s {
                Selection::Mesh(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    /// The mesh chosen last: what the mesh tool edits.
    pub fn last_mesh(&self) -> Option<String> {
        self.selected_meshes().pop()
    }

    /// The deformer chosen last: what the deformer tool edits.
    pub fn last_deformer(&self) -> Option<String> {
        self.selected().into_iter().rev().find_map(|s| match s {
            Selection::Warp(n) | Selection::Rotation(n) => Some(n),
            _ => None,
        })
    }

    /// How many things are selected.
    pub fn selected_count(&self) -> usize {
        self.selected().len()
    }

    /// Whether `s` is selected (any item of a multiple selection counts).
    pub fn is_selected(&self, s: &Selection) -> bool {
        self.selection.as_ref() == Some(s)
            || (self.others_of == self.selection && self.others.contains(s))
    }

    /// Records that the pointer rests on `s` in this pass.
    pub fn set_hover(&mut self, ctx: &egui::Context, s: Selection) {
        // The panel drawn earlier in this pass has not seen it: draw once more.
        if self.hover.as_ref().is_none_or(|(old, _)| *old != s) {
            self.hover_since = ctx.input(|i| i.time);
            ctx.request_repaint();
        }
        self.hover = Some((s, ctx.cumulative_pass_nr()));
    }

    /// What the pointer rests on, if it did so in this pass or the one before
    /// (the panel drawn later reads what the earlier one set).
    pub fn hover(&self, ctx: &egui::Context) -> Option<&Selection> {
        let (s, pass) = self.hover.as_ref()?;
        (pass + 1 >= ctx.cumulative_pass_nr()).then_some(s)
    }

    /// The hovered item once the pointer has rested on it for [`CARD_DELAY`].
    pub fn hover_card(&self, ctx: &egui::Context) -> Option<&Selection> {
        let s = self.hover(ctx)?;
        let waited = ctx.input(|i| i.time) - self.hover_since;
        if waited < CARD_DELAY {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(CARD_DELAY - waited));
            return None;
        }
        Some(s)
    }

    /// Selects `s` alone.
    pub fn select_only(&mut self, s: Selection) {
        self.anchor = Some(s.clone());
        self.others.clear();
        self.others_of = None;
        self.selection = Some(s);
        self.touch();
    }

    /// Selects exactly these; the last one is `selection`.
    pub fn select_all(&mut self, mut items: Vec<Selection>) {
        let Some(last) = items.pop() else {
            self.clear_selection();
            return;
        };
        self.selection = Some(last);
        self.others = items;
        self.others_of = self.selection.clone();
        self.touch();
    }

    /// Nothing selected.
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.others.clear();
        self.others_of = None;
        self.touch();
    }

    /// Selects exactly these meshes; the last one is `selection`.
    pub fn select_meshes(&mut self, names: Vec<String>) {
        self.select_all(names.into_iter().map(Selection::Mesh).collect());
    }

    /// Ctrl / Cmd click: adds `s` to the selection, or takes it out if it is in.
    pub fn toggle(&mut self, s: Selection) {
        let mut items = self.selected();
        if let Some(i) = items.iter().position(|o| *o == s) {
            items.remove(i);
        } else {
            items.push(s.clone());
        }
        self.anchor = Some(s);
        self.select_all(items);
    }

    pub fn toggle_mesh(&mut self, name: &str) {
        self.toggle(Selection::Mesh(name.to_owned()));
    }

    /// The meshes the selection stands for: the selected meshes, every mesh
    /// under a selected part, and those hanging from a selected deformer
    /// (down the whole tree); each once.
    pub fn meshes_of(&self, model: &tsukumoe_core::model::Model) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for item in self.selected() {
            for n in meshes_of_item(model, &item) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// The meshes that stay visible while soloing, `None` when not soloing (or
    /// when the soloed element is gone or holds no mesh).
    pub fn solo_meshes(&self, model: &tsukumoe_core::model::Model) -> Option<Vec<String>> {
        let names = meshes_of_item(model, self.solo.as_ref()?);
        (!names.is_empty()).then_some(names)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Requests a redraw (camera moved, selection changed...).
    pub fn touch(&mut self) {
        self.revision += 1;
    }

    pub fn param(&self, name: &str, p: &Parameter) -> f32 {
        self.params.get(name).copied().unwrap_or(p.default)
    }

    /// Sets the current value, clamped to the parameter's range; returns the
    /// value stored.
    pub fn set_param(&mut self, name: &str, p: &Parameter, value: f32) -> f32 {
        let value = if value.is_finite() {
            value.clamp(p.min, p.max)
        } else {
            p.default
        };
        self.params.insert(name.to_owned(), value);
        self.touch();
        value
    }

    /// Moves a parameter's value and the panel focus to its new name.
    pub fn rename_param(&mut self, from: &str, to: &str) {
        if let Some(v) = self.params.remove(from) {
            self.params.insert(to.to_owned(), v);
        }
        for (a, b) in &mut self.param_links {
            for n in [a, b] {
                if n == from {
                    to.clone_into(n);
                }
            }
        }
        if self.param_dialog.as_deref() == Some(from) {
            self.param_dialog = Some(to.to_owned());
        }
        if self.param_source.as_deref() == Some(from) {
            self.param_source = Some(to.to_owned());
        }
        if self.param_focus.as_deref() == Some(from) {
            self.param_focus = Some(to.to_owned());
        }
        if self.selection == Some(Selection::Parameter(from.to_owned())) {
            self.selection = Some(Selection::Parameter(to.to_owned()));
        }
        self.touch();
    }

    /// Drops the value, the focus and the selection of a deleted parameter.
    pub fn forget_param(&mut self, name: &str) {
        self.params.remove(name);
        self.param_links.retain(|(a, b)| a != name && b != name);
        if self.param_dialog.as_deref() == Some(name) {
            self.param_dialog = None;
        }
        if self.param_source.as_deref() == Some(name) {
            self.param_source = None;
        }
        if self.param_focus.as_deref() == Some(name) {
            self.param_focus = None;
        }
        if self.selection == Some(Selection::Parameter(name.to_owned())) {
            self.selection = None;
        }
        self.touch();
    }

    pub fn reset_params(&mut self) {
        self.params.clear();
        self.touch();
    }

    /// The names that have a value other than the default.
    pub fn set_values(&self) -> impl Iterator<Item = (&str, f32)> {
        self.params.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

/// The meshes one selected item stands for: the mesh itself, every mesh under
/// a part, those hanging from a deformer (down the whole tree), the two a glue joins.
pub fn meshes_of_item(model: &tsukumoe_core::model::Model, item: &Selection) -> Vec<String> {
    match item {
        Selection::Mesh(n) => vec![n.clone()],
        Selection::Part(name) => crate::tree_model::meshes_under_part(model, name),
        Selection::Warp(name) | Selection::Rotation(name) => {
            crate::tree_model::meshes_under_deformer(model, name)
        }
        Selection::Glue(name) => model
            .glues()
            .find(name)
            .and_then(|id| model.glues().get(id))
            .map(|g| {
                [g.mesh_a, g.mesh_b]
                    .iter()
                    .filter_map(|&m| model.meshes().name(m).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        Selection::Parameter(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_hover_card_waits_half_a_second() {
        let ctx = egui::Context::default();
        let mut v = ViewState::default();
        let at = |t: f64| egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        ctx.run_pass(at(1.0), |ctx| {
            v.set_hover(ctx, Selection::Mesh("a".into()));
            assert!(v.hover_card(ctx).is_none());
        })
        .textures_delta
        .clear();
        ctx.run_pass(at(1.6), |ctx| {
            v.set_hover(ctx, Selection::Mesh("a".into()));
            assert_eq!(v.hover_card(ctx), Some(&Selection::Mesh("a".into())));
        })
        .textures_delta
        .clear();
    }

    use super::*;

    const FIT: [f32; 4] = [-1.0, -1.0, 1.0, 1.0];

    #[test]
    fn soloing_a_folder_keeps_every_mesh_under_it_and_nothing_else() {
        use tsukumoe_core::command::Command;
        use tsukumoe_core::model::Model;
        let mut m = Model::new();
        let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let part = |name: &str, parent: Option<&str>| Command::AddPart {
            name: name.into(),
            parent: parent.map(Into::into),
        };
        let mesh = |name: &str, part: &str| Command::AddMesh {
            name: name.into(),
            part: part.into(),
            positions: tri.clone(),
            uvs: tri.clone(),
            indices: vec![0, 1, 2],
        };
        for c in [
            part("Head", None),
            part("Hair", Some("Head")),
            part("Body", None),
            mesh("Face", "Head"),
            mesh("Fringe", "Hair"),
            mesh("Shirt", "Body"),
        ] {
            m.apply(c).unwrap();
        }
        let mut view = ViewState::default();
        assert_eq!(view.solo_meshes(&m), None);
        view.solo = Some(Selection::Part("Head".into()));
        let mut kept = view.solo_meshes(&m).unwrap();
        kept.sort();
        assert_eq!(kept, ["Face", "Fringe"]);
        view.solo = Some(Selection::Mesh("Shirt".into()));
        assert_eq!(view.solo_meshes(&m), Some(vec!["Shirt".to_owned()]));
        // An element that is gone, or holds no mesh, means no solo.
        view.solo = Some(Selection::Part("Nope".into()));
        assert_eq!(view.solo_meshes(&m), None);
    }

    #[test]
    fn the_default_camera_shows_the_fitted_view() {
        assert_eq!(Camera::default().view(FIT), FIT);
    }

    #[test]
    fn pan_drags_the_picture_with_the_pointer() {
        let mut c = Camera::default();
        // Dragging right by a quarter of the width moves the view left.
        c.pan([50.0, 0.0], [200.0, 100.0]);
        let v = c.view(FIT);
        assert!(
            (v[0] - -1.5).abs() < 1e-5 && (v[2] - 0.5).abs() < 1e-5,
            "{v:?}"
        );
        // Dragging down moves the view up (model y is up).
        let mut c = Camera::default();
        c.pan([0.0, 25.0], [200.0, 100.0]);
        assert!(c.view(FIT)[1] > -1.0);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_anchor() {
        let mut c = Camera::default();
        c.pan([13.0, -7.0], [200.0, 100.0]);
        let anchor = [0.25, 0.75];
        let at = |c: &Camera| {
            let v = c.view(FIT);
            (
                v[0] + anchor[0] * (v[2] - v[0]),
                v[3] - anchor[1] * (v[3] - v[1]),
            )
        };
        let before = at(&c);
        c.zoom_at(2.0, anchor);
        let after = at(&c);
        assert!((before.0 - after.0).abs() < 1e-5 && (before.1 - after.1).abs() < 1e-5);
        assert_eq!(c.zoom, 2.0);
        c.zoom_at(1e9, [0.5, 0.5]);
        assert_eq!(c.zoom, ZOOM_RANGE.1);
        c.zoom_at(1e-9, [0.5, 0.5]);
        assert_eq!(c.zoom, ZOOM_RANGE.0);
    }

    #[test]
    fn values_are_clamped_default_when_unset_and_bump_the_revision() {
        let p = Parameter::new(-30.0, 30.0, 5.0);
        let mut v = ViewState::default();
        assert_eq!(v.param("a", &p), 5.0);
        let rev = v.revision();
        assert_eq!(v.set_param("a", &p, 100.0), 30.0);
        assert_eq!(v.set_param("a", &p, f32::NAN), 5.0);
        assert!(v.revision() > rev);
        v.set_param("a", &p, -3.0);
        assert_eq!(v.param("a", &p), -3.0);
        v.reset_params();
        assert_eq!(v.param("a", &p), 5.0);
    }

    #[test]
    fn meshes_toggle_in_and_out_and_a_single_click_returns_to_one() {
        let mesh = |n: &str| Selection::Mesh(n.into());
        let mut v = ViewState::default();
        v.select_only(mesh("A"));
        v.toggle_mesh("B");
        v.toggle_mesh("C");
        assert_eq!(v.selected_meshes(), ["A", "B", "C"]);
        assert_eq!(v.selection, Some(mesh("C")));
        assert!(
            v.is_selected(&mesh("A")) && v.is_selected(&mesh("C")) && !v.is_selected(&mesh("D"))
        );
        // Taking one out keeps the others; the last chosen stays the main one.
        v.toggle_mesh("B");
        assert_eq!(v.selected_meshes(), ["A", "C"]);
        v.toggle_mesh("C");
        assert_eq!(v.selected_meshes(), ["A"]);
        assert_eq!(v.selection, Some(mesh("A")));
        v.toggle_mesh("A");
        assert!(v.selected_meshes().is_empty() && v.selection.is_none());
        // Back to one.
        v.toggle_mesh("A");
        v.toggle_mesh("B");
        v.select_only(mesh("B"));
        assert_eq!(v.selected_meshes(), ["B"]);
        assert!(!v.is_selected(&mesh("A")));
    }

    #[test]
    fn setting_the_selection_directly_drops_the_others() {
        let mut v = ViewState::default();
        v.select_meshes(vec!["A".into(), "B".into()]);
        assert_eq!(v.selected_meshes(), ["A", "B"]);
        v.selection = Some(Selection::Part("P".into()));
        assert!(v.selected_meshes().is_empty());
        assert!(!v.is_selected(&Selection::Mesh("A".into())));
    }
}
