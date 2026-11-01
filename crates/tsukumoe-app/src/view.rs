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

#[derive(Clone, Debug, Default)]
pub struct ViewState {
    /// What is selected; with several, the one chosen last.
    pub selection: Option<Selection>,
    /// The other selected items, and the `selection` they were chosen next
    /// to: when anything else sets `selection`, they no longer count.
    others: Vec<Selection>,
    others_of: Option<Selection>,
    /// Where a Shift click range starts.
    pub anchor: Option<Selection>,
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
        let mut add = |names: Vec<String>| {
            for n in names {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        };
        for item in self.selected() {
            match item {
                Selection::Mesh(n) => add(vec![n]),
                Selection::Part(name) => add(crate::tree_model::meshes_under_part(model, &name)),
                Selection::Warp(name) | Selection::Rotation(name) => {
                    add(crate::tree_model::meshes_under_deformer(model, &name));
                }
                Selection::Parameter(_) => {}
            }
        }
        out
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

    pub fn reset_params(&mut self) {
        self.params.clear();
        self.touch();
    }

    /// The names that have a value other than the default.
    pub fn set_values(&self) -> impl Iterator<Item = (&str, f32)> {
        self.params.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIT: [f32; 4] = [-1.0, -1.0, 1.0, 1.0];

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
