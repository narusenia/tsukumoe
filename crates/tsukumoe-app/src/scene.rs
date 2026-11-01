// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The evaluated model behind the viewport, without any GPU state: the model
//! compiled for evaluation, one frame of its pose, and the fit of the view.
//! Parameter values come from the view state, not from the model.

use tsukumoe_core::command::RotationBase;
use tsukumoe_core::compiled::{CompiledModel, RotationKf};
use tsukumoe_core::eval::Frame;
use tsukumoe_core::model::Model;

use crate::view::ViewState;

/// Used when the model draws nothing at its default pose.
const FALLBACK_VIEW: [f32; 4] = [-1.0, -1.0, 1.0, 1.0];
/// Space around the model, as a fraction of its size.
const MARGIN: f32 = 0.05;

/// The bounding box `[x0, y0, x1, y1]` of `x, y` pairs, or `None` when empty.
pub fn bounds(positions: &[f32]) -> Option<[f32; 4]> {
    let points = positions.as_chunks::<2>().0;
    let (first, rest) = points.split_first()?;
    let init = [first[0], first[1], first[0], first[1]];
    Some(rest.iter().fold(init, |b, p| {
        [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ]
    }))
}

/// Grows `bounds` by a margin and widens one side so the view has the aspect
/// ratio of a `width` x `height` target, keeping the center.
pub fn fit_view(bounds: [f32; 4], width: u32, height: u32) -> [f32; 4] {
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    let cx = (bounds[0] + bounds[2]) / 2.0;
    let cy = (bounds[1] + bounds[3]) / 2.0;
    let w = ((bounds[2] - bounds[0]) * (1.0 + 2.0 * MARGIN)).max(1e-3);
    let h = ((bounds[3] - bounds[1]) * (1.0 + 2.0 * MARGIN)).max(1e-3);
    let (w, h) = if w / h < aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
}

/// A shape shown while it is being dragged, before it is committed. It is
/// laid over the compiled copy of the model that the scene draws; the edited
/// model is not touched (ADR-0019), and a rebuilt scene starts without one.
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    /// The stored vertex positions of a mesh.
    Mesh {
        name: String,
        positions: Vec<[f32; 2]>,
    },
    /// The base control points of a warp deformer.
    Warp { name: String, points: Vec<[f32; 2]> },
    /// The base shape of a rotation deformer.
    Rotation { name: String, base: RotationBase },
}

/// What a preview replaced, to put back.
enum Saved {
    Floats {
        warp: bool,
        start: usize,
        data: Vec<f32>,
    },
    Rotation {
        index: usize,
        base: RotationKf,
    },
}

pub struct Scene {
    pub model: CompiledModel<'static>,
    pub frame: Frame,
    bounds: [f32; 4],
    warp_names: Vec<String>,
    rotation_names: Vec<String>,
    preview: Option<Preview>,
    saved: Option<Saved>,
}

impl Scene {
    pub fn new(source: &Model) -> Result<Self, String> {
        let model = source.compile().map_err(|e| format!("{e:?}"))?;
        let mut frame = Frame::new(&model);
        frame.evaluate(&model);
        let bounds = bounds(frame.draw_list(&model).positions()).unwrap_or(FALLBACK_VIEW);
        Ok(Self {
            model,
            frame,
            bounds,
            warp_names: source
                .warps()
                .iter()
                .map(|(_, n, _)| n.to_owned())
                .collect(),
            rotation_names: source
                .rotations()
                .iter()
                .map(|(_, n, _)| n.to_owned())
                .collect(),
            preview: None,
            saved: None,
        })
    }

    /// Shows `preview` (or, with `None`, the model as it is). True if that
    /// changed what is drawn; the next [`Scene::advance`] evaluates it.
    pub fn set_preview(&mut self, preview: Option<&Preview>) -> bool {
        if self.preview.as_ref() == preview {
            return false;
        }
        match self.saved.take() {
            Some(Saved::Floats { warp, start, data }) => {
                let target = if warp {
                    self.model.warp_base_points.to_mut()
                } else {
                    self.model.mesh_base_positions.to_mut()
                };
                target[start..start + data.len()].copy_from_slice(&data);
            }
            Some(Saved::Rotation { index, base }) => {
                self.model.rotations.to_mut()[index].base = base;
            }
            None => {}
        }
        self.preview = preview.cloned();
        let Some(preview) = preview else {
            return true;
        };
        self.saved = match preview {
            Preview::Mesh { name, positions } => self.model.mesh_index(name).and_then(|i| {
                let def = self.model.meshes[i];
                let (start, n) = (def.vertex_start as usize * 2, def.vertex_count as usize);
                (n == positions.len()).then(|| {
                    let target = self.model.mesh_base_positions.to_mut();
                    let data = target[start..start + n * 2].to_vec();
                    for (k, p) in positions.iter().enumerate() {
                        target[start + 2 * k] = p[0];
                        target[start + 2 * k + 1] = p[1];
                    }
                    Saved::Floats {
                        warp: false,
                        start,
                        data,
                    }
                })
            }),
            Preview::Warp { name, points } => {
                let i = self.warp_names.iter().position(|n| n == name);
                i.and_then(|i| {
                    let def = self.model.warps[i];
                    let (start, n) = (def.point_start as usize, def.point_count as usize);
                    (n == points.len()).then(|| {
                        let target = self.model.warp_base_points.to_mut();
                        let data = target[start..start + n * 2].to_vec();
                        for (k, p) in points.iter().enumerate() {
                            target[start + 2 * k] = p[0];
                            target[start + 2 * k + 1] = p[1];
                        }
                        Saved::Floats {
                            warp: true,
                            start,
                            data,
                        }
                    })
                })
            }
            Preview::Rotation { name, base } => {
                let i = self.rotation_names.iter().position(|n| n == name);
                i.map(|i| {
                    let old = self.model.rotations[i].base;
                    self.model.rotations.to_mut()[i].base = RotationKf {
                        origin: base.origin,
                        angle_degrees: base.angle_degrees,
                        scale: base.scale,
                        reflect_x: base.reflect_x.into(),
                        reflect_y: base.reflect_y.into(),
                        opacity: old.opacity,
                    };
                    Saved::Rotation {
                        index: i,
                        base: old,
                    }
                })
            }
        };
        true
    }

    pub fn bounds(&self) -> [f32; 4] {
        self.bounds
    }

    /// Sets every parameter of the frame from the view state (the default when
    /// the view has no value).
    pub fn apply(&mut self, view: &ViewState) {
        for i in 0..self.model.parameters.len() {
            let default = self.model.parameters[i].default;
            self.frame.set_parameter(i, default);
        }
        for (name, value) in view.set_values() {
            if let Some(i) = self.model.parameter_index(name) {
                self.frame.set_parameter(i, value);
            }
        }
    }

    /// The vertices of the mesh `name` as drawn now: deformers, keyforms and
    /// the current parameter values applied.
    pub fn drawn_positions(&self, name: &str) -> Option<Vec<[f32; 2]>> {
        let mesh = self.model.mesh_index(name)?;
        let list = self.frame.draw_list(&self.model);
        Some(list.drawable(mesh).positions.as_chunks::<2>().0.to_vec())
    }

    pub fn advance(&mut self, dt: f32) {
        self.frame.advance(&self.model, dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

    #[test]
    fn bounds_and_fit_keep_the_aspect_and_the_center() {
        assert_eq!(bounds(&[]), None);
        assert_eq!(
            bounds(&[1.0, 5.0, -2.0, 3.0, 4.0, -1.0]),
            Some([-2.0, -1.0, 4.0, 5.0])
        );
        let b = [0.0, 0.0, 2.0, 1.0];
        for (w, h) in [(200, 100), (100, 200), (100, 100)] {
            let v = fit_view(b, w, h);
            assert!((((v[2] - v[0]) / (v[3] - v[1])) - w as f32 / h as f32).abs() < 1e-4);
            assert!(v[0] <= 0.0 && v[2] >= 2.0 && v[1] <= 0.0 && v[3] >= 1.0);
        }
        let v = fit_view([3.0, 3.0, 3.0, 3.0], 0, 0);
        assert!(v[2] > v[0] && v[3] > v[1]);
    }

    #[test]
    fn a_view_value_reaches_the_frame_and_missing_ones_use_the_default() {
        let mut m = Model::new();
        for (n, d) in [("A", 0.25), ("B", 0.75)] {
            m.apply(Command::AddParameter {
                name: n.into(),
                min: 0.0,
                max: 1.0,
                default: d,
                repeat: false,
            })
            .unwrap();
        }
        let mut scene = Scene::new(&m).unwrap();
        let mut view = ViewState::default();
        let p = m
            .parameters()
            .get(m.parameters().find("A").unwrap())
            .unwrap();
        view.set_param("A", p, 1.0);
        scene.apply(&view);
        let a = scene.model.parameter_index("A").unwrap();
        let b = scene.model.parameter_index("B").unwrap();
        assert_eq!(
            (
                scene.frame.parameters_mut()[a],
                scene.frame.parameters_mut()[b]
            ),
            (1.0, 0.75)
        );
        view.reset_params();
        scene.apply(&view);
        assert_eq!(scene.frame.parameters_mut()[a], 0.25);
    }

    #[test]
    fn the_drawn_positions_follow_a_keyform_driven_warp_and_the_parameter() {
        use tsukumoe_core::elements::{Parameter, Part};
        use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
        use tsukumoe_core::model::ModelParts;
        use tsukumoe_core::rig::{ArtMesh, DeformerRef, WarpDeformer, WarpKeyform};
        let mut parts = ModelParts::default();
        let p = parts
            .parameters
            .insert("P", Parameter::new(0.0, 1.0, 0.0))
            .unwrap();
        let part = parts.parts.insert("Part", Part::default()).unwrap();
        let binding = parts.bindings.insert(
            Binding::new(vec![Axis {
                parameter: p,
                keys: vec![0.0, 1.0],
            }])
            .unwrap(),
        );
        let flat = vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]];
        // At P = 1 the whole grid slides two units to the right.
        let moved: Vec<[f32; 2]> = flat.iter().map(|q| [q[0] + 2.0, q[1]]).collect();
        let key = |points: Vec<[f32; 2]>| WarpKeyform {
            points,
            opacity: 1.0,
        };
        let warp = parts
            .warps
            .insert(
                "Warp",
                WarpDeformer {
                    part,
                    parent: None,
                    rows: 1,
                    columns: 1,
                    bezier: false,
                    base_points: flat.clone(),
                    keyforms: Some(KeyformGrid::new(binding, vec![key(flat), key(moved)])),
                },
            )
            .unwrap();
        let mut mesh = ArtMesh::new(
            part,
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            vec![[0.0, 0.0]; 3],
            vec![0, 1, 2],
        );
        mesh.parent = Some(DeformerRef::Warp(warp));
        parts.meshes.insert("Skin", mesh).unwrap();
        let model = Model::from_parts(parts).unwrap();
        let mut scene = Scene::new(&model).unwrap();
        let mut view = ViewState::default();
        let at_rest = scene.drawn_positions("Skin").unwrap();
        assert_eq!(at_rest[1], [2.0, 0.0]);
        // Moving the parameter moves where the vertices are drawn.
        let param = model
            .parameters()
            .get(model.parameters().find("P").unwrap())
            .unwrap();
        view.set_param("P", param, 1.0);
        scene.apply(&view);
        scene.advance(0.0);
        let moved = scene.drawn_positions("Skin").unwrap();
        assert!(
            (moved[1][0] - 4.0).abs() < 1e-4 && moved[1][1].abs() < 1e-4,
            "{:?}",
            moved[1]
        );
        assert!(scene.drawn_positions("Nope").is_none());
    }

    fn live_model() -> Model {
        use tsukumoe_core::command::RotationBase;
        let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let mut m = Model::new();
        for c in [
            Command::AddPart {
                name: "P".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "P".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [4.0, 0.0], [0.0, 4.0], [4.0, 4.0]],
            },
            Command::AddRotationDeformer {
                name: "R".into(),
                part: "P".into(),
                parent: None,
                base: RotationBase {
                    origin: [10.0, 0.0],
                    angle_degrees: 0.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
            Command::AddMesh {
                name: "Skin".into(),
                part: "P".into(),
                positions: tri.clone(),
                uvs: tri.clone(),
                indices: vec![0, 1, 2],
            },
            Command::AddMesh {
                name: "Spin".into(),
                part: "P".into(),
                positions: vec![[1.0, 0.0], [2.0, 0.0], [1.0, 1.0]],
                uvs: tri.clone(),
                indices: vec![0, 1, 2],
            },
            Command::AddMesh {
                name: "Clipped".into(),
                part: "P".into(),
                positions: tri.clone(),
                uvs: tri,
                indices: vec![0, 1, 2],
            },
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: Some("W".into()),
            },
            Command::SetMeshParent {
                name: "Spin".into(),
                parent: Some("R".into()),
            },
            Command::SetMeshMasks {
                name: "Clipped".into(),
                masks: vec!["Skin".into()],
                inverted: false,
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    type Drawn = Vec<Vec<[f32; 2]>>;

    fn drawn_all(scene: &Scene) -> Drawn {
        ["Skin", "Spin", "Clipped"]
            .iter()
            .map(|n| scene.drawn_positions(n).unwrap())
            .collect()
    }

    fn settle(scene: &mut Scene) {
        scene.advance(0.0);
    }

    #[test]
    fn a_preview_changes_what_is_drawn_not_the_model_and_clears_back() {
        use crate::dispatcher::Dispatcher;
        let d = Dispatcher::new(live_model());
        let (steps, rev) = (d.log().len(), d.revision());
        let mut scene = Scene::new(d.model()).unwrap();
        settle(&mut scene);
        let before = drawn_all(&scene);
        let preview = Preview::Mesh {
            name: "Clipped".into(),
            positions: vec![[5.0, 5.0], [6.0, 5.0], [5.0, 6.0]],
        };
        assert!(scene.set_preview(Some(&preview)));
        assert!(
            !scene.set_preview(Some(&preview)),
            "the same preview changes nothing"
        );
        settle(&mut scene);
        assert_eq!(scene.drawn_positions("Clipped").unwrap()[0], [5.0, 5.0]);
        // The edited model, its history and its saved flag are untouched.
        assert_eq!(
            (d.log().len(), d.revision(), d.dirty()),
            (steps, rev, false)
        );
        assert!(scene.set_preview(None));
        settle(&mut scene);
        assert_eq!(drawn_all(&scene), before);
        // A preview of the wrong size or for an unknown name is ignored.
        let bad = Preview::Mesh {
            name: "Clipped".into(),
            positions: vec![[0.0, 0.0]],
        };
        scene.set_preview(Some(&bad));
        settle(&mut scene);
        assert_eq!(drawn_all(&scene), before);
        scene.set_preview(Some(&Preview::Warp {
            name: "Nope".into(),
            points: vec![],
        }));
        assert_eq!(drawn_all(&scene), before);
    }

    /// Previews `preview`, then commits `command` and draws the new model: the
    /// two pictures are the same.
    fn preview_matches_commit(preview: Preview, command: Command) -> (Drawn, Drawn) {
        let mut d = crate::dispatcher::Dispatcher::new(live_model());
        let mut scene = Scene::new(d.model()).unwrap();
        settle(&mut scene);
        let before = drawn_all(&scene);
        scene.set_preview(Some(&preview));
        settle(&mut scene);
        let live = drawn_all(&scene);
        assert_ne!(live, before, "the preview changes the picture");
        d.submit(command).unwrap();
        let mut committed = Scene::new(d.model()).unwrap();
        settle(&mut committed);
        let after = drawn_all(&committed);
        // Esc: dropping the preview returns the old picture.
        scene.set_preview(None);
        settle(&mut scene);
        assert_eq!(drawn_all(&scene), before);
        (live, after)
    }

    fn close(a: &Drawn, b: &Drawn) {
        for (x, y) in a.iter().flatten().zip(b.iter().flatten()) {
            assert!(
                (x[0] - y[0]).abs() < 1e-4 && (x[1] - y[1]).abs() < 1e-4,
                "{x:?} vs {y:?}"
            );
        }
    }

    #[test]
    fn a_mesh_preview_is_the_committed_picture() {
        let positions = vec![[0.5, 0.5], [1.5, 0.0], [0.0, 1.0]];
        let (live, after) = preview_matches_commit(
            Preview::Mesh {
                name: "Skin".into(),
                positions: positions.clone(),
            },
            Command::SetMeshPositions {
                name: "Skin".into(),
                positions,
            },
        );
        close(&live, &after);
    }

    #[test]
    fn a_warp_preview_moves_its_children_and_is_the_committed_picture() {
        let points = vec![[0.0, 0.0], [6.0, 0.0], [0.0, 4.0], [6.0, 5.0]];
        let (live, after) = preview_matches_commit(
            Preview::Warp {
                name: "W".into(),
                points: points.clone(),
            },
            Command::SetWarpBasePoints {
                name: "W".into(),
                points,
            },
        );
        close(&live, &after);
        // The child (Skin, index 0) moved with its parent.
        assert!(live[0][1][0] > 1.0 + 1e-3);
    }

    #[test]
    fn a_rotation_preview_turns_its_children_and_is_the_committed_picture() {
        use tsukumoe_core::command::RotationBase;
        let base = RotationBase {
            origin: [10.0, 1.0],
            angle_degrees: 90.0,
            scale: 2.0,
            reflect_x: false,
            reflect_y: true,
        };
        let (live, after) = preview_matches_commit(
            Preview::Rotation {
                name: "R".into(),
                base,
            },
            Command::SetRotationBase {
                name: "R".into(),
                base,
            },
        );
        close(&live, &after);
    }
}
