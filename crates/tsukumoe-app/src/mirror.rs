// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Left-right mirror copy of one keyform to another (M5-06c). The shape is
//! mirrored about the vertical line `x = center` in the drawing space (at the
//! base shape of the deformers, as in `deformer_ops`), then expressed in the
//! space of the target's parent. The result is one `Set*Keyform` command; the
//! model is not changed here (ADR-0019).

use std::fmt;

use tsukumoe_core::command::{Command, ElementRef, RotationBase};
use tsukumoe_core::deformer_space::{Space, SpaceError};
use tsukumoe_core::elements::{ParameterId, Point};
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::DeformerRef;

use crate::keyed;
use crate::mesh_tool::Op;
use crate::view::ViewState;

/// Vertices pair when the nearest mirrored one is within this fraction of the
/// diagonal of the target mesh (in the drawing space).
pub const PAIR_TOLERANCE: f32 = 0.02;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MirrorError {
    /// An element is not in the model.
    Missing(String),
    /// The element has no keyforms, or the key does not exist.
    NoKey(String),
    /// Source and target are not the same kind of element.
    Kind,
    /// Grids or meshes of different sizes.
    Shape,
    /// A vertex has no mirror partner (or two share one).
    Unpaired,
    /// A deformer is flat where the conversion needs its inverse.
    Space(SpaceError),
}

impl MirrorError {
    /// The translation key of the message.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Missing(_) => "deformer-missing",
            Self::NoKey(_) => "mirror-err-no-key",
            Self::Kind => "mirror-err-kind",
            Self::Shape => "mirror-err-shape",
            Self::Unpaired => "mirror-err-unpaired",
            Self::Space(_) => "deformer-flat-space",
        }
    }
}

impl fmt::Display for MirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(n) | Self::NoKey(n) => write!(f, "{}: '{n}'", self.key()),
            other => f.write_str(other.key()),
        }
    }
}

impl From<SpaceError> for MirrorError {
    fn from(e: SpaceError) -> Self {
        Self::Space(e)
    }
}

/// `p` of one parent space as seen from another, mirrored about `x = center`.
struct Move {
    from: Space,
    to: Space,
    center: f32,
}

impl Move {
    fn world(&self, p: Point) -> Point {
        let w = self.from.point_to_world(p);
        [2.0 * self.center - w[0], w[1]]
    }

    fn point(&self, p: Point) -> Result<Point, MirrorError> {
        Ok(self.to.point_from_world(self.world(p))?)
    }
}

fn space_of(model: &Model, parent: Option<DeformerRef>) -> Result<Space, MirrorError> {
    Ok(Space::of(model, parent)?)
}

fn keyform_of<T>(
    model: &Model,
    grid: Option<&tsukumoe_core::keyform::KeyformGrid<T>>,
    key: &[usize],
    name: &str,
) -> Result<usize, MirrorError> {
    let no_key = || MirrorError::NoKey(name.to_owned());
    let grid = grid.ok_or_else(no_key)?;
    model
        .bindings()
        .get(grid.binding)
        .and_then(|b| b.index(key))
        .ok_or_else(no_key)
}

/// The key `element` is at under the sliders `at`, `None` if it has no keyforms
/// or the sliders are off its keys.
pub fn key_here(
    model: &Model,
    element: &ElementRef,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<Vec<usize>> {
    let place = match element {
        ElementRef::Mesh { name } => keyed::mesh_place(model, name, at),
        ElementRef::Deformer { name } => keyed::deformer_place(model, name, at),
    };
    match place? {
        keyed::Place::Key { key, .. } => Some(key),
        keyed::Place::Off { .. } => None,
    }
}

/// The x of the middle of the model: the centre of the meshes' base shapes in
/// the drawing space (0 for a model without meshes).
pub fn model_center(model: &Model) -> f32 {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for (_, _, mesh) in model.meshes().iter() {
        let Ok(space) = Space::of(model, mesh.parent) else {
            continue;
        };
        for &p in &mesh.positions {
            let x = space.point_to_world(p)[0];
            lo = lo.min(x);
            hi = hi.max(x);
        }
    }
    if lo > hi { 0.0 } else { (lo + hi) / 2.0 }
}

/// The mirror line: what was typed, else the centre of the model.
pub fn center(model: &Model, typed: &str) -> f32 {
    typed.trim().parse().unwrap_or_else(|_| model_center(model))
}

/// Remembers the keyform of `element` under the sliders as the mirror source.
pub fn set_source(
    model: &Model,
    view: &mut ViewState,
    element: &ElementRef,
) -> Result<(), &'static str> {
    let at = keyed::values(model, view);
    let key = key_here(model, element, &at).ok_or("param-err-not-on-key")?;
    view.mirror_source = Some((element.clone(), key));
    Ok(())
}

/// The op that pastes the mirror source, mirrored, onto the keyform of
/// `element` under the sliders; the error is a translation key.
pub fn paste(model: &Model, view: &ViewState, element: &ElementRef) -> Result<Op, &'static str> {
    let (source, from) = view.mirror_source.as_ref().ok_or("mirror-err-no-source")?;
    let at = keyed::values(model, view);
    let to = key_here(model, element, &at).ok_or("param-err-not-on-key")?;
    let center = center(model, &view.mirror_center);
    mirror_copy(model, source, from, element, &to, center).map_err(|e| e.key())
}

/// The command that gives the keyform of `target` at `to` the mirror image of
/// the keyform of `source` at `from`. Mirrors about `x = center`.
pub fn mirror_copy(
    model: &Model,
    source: &ElementRef,
    from: &[usize],
    target: &ElementRef,
    to: &[usize],
    center: f32,
) -> Result<Op, MirrorError> {
    let label = |a: &str, b: &str| format!("mirror_copy: {a} -> {b}");
    match (source, target) {
        (ElementRef::Mesh { name: a }, ElementRef::Mesh { name: b }) => {
            let get = |n: &String| {
                model
                    .meshes()
                    .find(n)
                    .and_then(|id| model.meshes().get(id))
                    .ok_or_else(|| MirrorError::Missing(n.clone()))
            };
            let (src, dst) = (get(a)?, get(b)?);
            let si = keyform_of(model, src.keyforms.as_ref(), from, a)?;
            keyform_of(model, dst.keyforms.as_ref(), to, b)?;
            let kf = &src.keyforms.as_ref().expect("keyform_of checked").keyforms[si];
            if src.positions.len() != dst.positions.len() {
                return Err(MirrorError::Shape);
            }
            let mv = Move {
                from: space_of(model, src.parent)?,
                to: space_of(model, dst.parent)?,
                center,
            };
            let pairs = pair(&mv, &src.positions, &dst.positions)?;
            let mut positions = vec![[0.0; 2]; dst.positions.len()];
            for (j, &i) in pairs.iter().enumerate() {
                positions[j] = mv.point(kf.positions[i])?;
            }
            Ok((
                Command::SetMeshKeyform {
                    name: b.clone(),
                    key: to.to_vec(),
                    positions,
                    opacity: kf.opacity,
                    draw_order: kf.draw_order,
                },
                label(a, b),
            ))
        }
        (ElementRef::Deformer { name: a }, ElementRef::Deformer { name: b }) => {
            match (model.warps().find(a), model.warps().find(b)) {
                (Some(wa), Some(wb)) => {
                    let (src, dst) = (
                        model.warps().get(wa).expect("found"),
                        model.warps().get(wb).expect("found"),
                    );
                    let si = keyform_of(model, src.keyforms.as_ref(), from, a)?;
                    keyform_of(model, dst.keyforms.as_ref(), to, b)?;
                    if (src.rows, src.columns) != (dst.rows, dst.columns) {
                        return Err(MirrorError::Shape);
                    }
                    let kf = &src.keyforms.as_ref().expect("checked").keyforms[si];
                    let mv = Move {
                        from: space_of(model, src.parent)?,
                        to: space_of(model, dst.parent)?,
                        center,
                    };
                    let columns = dst.columns as usize + 1;
                    let mut points = vec![[0.0; 2]; kf.points.len()];
                    for (n, slot) in points.iter_mut().enumerate() {
                        let (r, c) = (n / columns, n % columns);
                        *slot = mv.point(kf.points[r * columns + (columns - 1 - c)])?;
                    }
                    Ok((
                        Command::SetWarpKeyform {
                            name: b.clone(),
                            key: to.to_vec(),
                            points,
                            opacity: kf.opacity,
                        },
                        label(a, b),
                    ))
                }
                (None, None) => {
                    let get = |n: &String| {
                        model
                            .rotations()
                            .find(n)
                            .and_then(|id| model.rotations().get(id))
                            .ok_or_else(|| MirrorError::Missing(n.clone()))
                    };
                    let (src, dst) = (get(a)?, get(b)?);
                    let si = keyform_of(model, src.keyforms.as_ref(), from, a)?;
                    keyform_of(model, dst.keyforms.as_ref(), to, b)?;
                    let kf = &src.keyforms.as_ref().expect("checked").keyforms[si];
                    let mv = Move {
                        from: space_of(model, src.parent)?,
                        to: space_of(model, dst.parent)?,
                        center,
                    };
                    Ok((
                        Command::SetRotationKeyform {
                            name: b.clone(),
                            key: to.to_vec(),
                            base: RotationBase {
                                origin: mv.point(kf.origin)?,
                                angle_degrees: -kf.angle_degrees,
                                scale: kf.scale,
                                reflect_x: kf.reflect_x,
                                reflect_y: kf.reflect_y,
                            },
                            opacity: kf.opacity,
                        },
                        label(a, b),
                    ))
                }
                (Some(_), None) | (None, Some(_)) => Err(MirrorError::Kind),
            }
        }
        _ => Err(MirrorError::Kind),
    }
}

/// For each vertex of `dst` (base shape, in the target's space), the index of
/// the vertex of `src` whose mirror image is nearest to it. Fails unless the
/// pairing is one to one and every pair is within [`PAIR_TOLERANCE`].
fn pair(mv: &Move, src: &[Point], dst: &[Point]) -> Result<Vec<usize>, MirrorError> {
    let mirrored: Vec<Point> = src.iter().map(|&p| mv.world(p)).collect();
    let world: Vec<Point> = dst.iter().map(|&p| mv.to.point_to_world(p)).collect();
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &world {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let tolerance = (hi[0] - lo[0]).hypot(hi[1] - lo[1]) * PAIR_TOLERANCE;
    let mut taken = vec![false; src.len()];
    let mut pairs = Vec::with_capacity(dst.len());
    for w in &world {
        let (i, d) = mirrored
            .iter()
            .enumerate()
            .map(|(i, m)| (i, (m[0] - w[0]).hypot(m[1] - w[1])))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or(MirrorError::Unpaired)?;
        if d > tolerance || std::mem::replace(&mut taken[i], true) {
            return Err(MirrorError::Unpaired);
        }
        pairs.push(i);
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(m: &mut Model, name: &str, pts: Vec<Point>) {
        m.apply(Command::AddMesh {
            name: name.into(),
            part: "Part".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2],
        })
        .unwrap();
    }

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(Command::AddParameter {
            name: "P".into(),
            min: -1.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        m.apply(Command::AddPart {
            name: "Part".into(),
            parent: None,
        })
        .unwrap();
        // Left (x < 0) and right (x > 0) triangles, mirror images about x = 0.
        mesh(&mut m, "L", vec![[-1.0, 0.0], [-3.0, 0.0], [-1.0, 2.0]]);
        // The right one lists its vertices in another order.
        mesh(&mut m, "R", vec![[1.0, 2.0], [1.0, 0.0], [3.0, 0.0]]);
        let elements = ["L", "R"].map(|n| ElementRef::Mesh { name: n.into() });
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: elements.to_vec(),
        })
        .unwrap();
        m
    }

    fn mesh_ref(n: &str) -> ElementRef {
        ElementRef::Mesh { name: n.into() }
    }

    #[test]
    fn a_mesh_keyform_is_mirrored_onto_its_partner_by_nearest_vertex() {
        let mut m = model();
        m.apply(Command::SetMeshKeyform {
            name: "L".into(),
            key: vec![2],
            positions: vec![[-1.0, 0.5], [-3.0, 0.0], [-1.0, 2.0]],
            opacity: 0.5,
            draw_order: 3.0,
        })
        .unwrap();
        let (command, _) =
            mirror_copy(&m, &mesh_ref("L"), &[2], &mesh_ref("R"), &[2], 0.0).unwrap();
        let Command::SetMeshKeyform {
            name,
            key,
            positions,
            opacity,
            ..
        } = &command
        else {
            panic!("a mesh keyform");
        };
        assert_eq!(
            (name.as_str(), key.as_slice(), *opacity),
            ("R", &[2][..], 0.5)
        );
        // R's vertex 1 sits at L's vertex 0, mirrored: (1, 0) -> (1, 0.5).
        assert_eq!(positions, &[[1.0, 2.0], [1.0, 0.5], [3.0, 0.0]]);
        m.apply(command).unwrap();
    }

    #[test]
    fn a_mesh_without_a_partner_is_refused() {
        let mut m = model();
        mesh(&mut m, "Odd", vec![[5.0, 5.0], [6.0, 5.0], [5.0, 9.0]]);
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: vec![mesh_ref("Odd")],
        })
        .unwrap();
        let r = mirror_copy(&m, &mesh_ref("L"), &[0], &mesh_ref("Odd"), &[0], 0.0);
        assert_eq!(r.unwrap_err(), MirrorError::Unpaired);
    }

    #[test]
    fn the_center_line_moves_the_mirror() {
        let m = model();
        // About x = 1 the left triangle lands at 3, 5, 3: nothing there.
        let r = mirror_copy(&m, &mesh_ref("L"), &[0], &mesh_ref("R"), &[0], 1.0);
        assert_eq!(r.unwrap_err(), MirrorError::Unpaired);
    }

    #[test]
    fn kinds_must_match_and_keys_must_exist() {
        let m = model();
        let d = ElementRef::Deformer { name: "W".into() };
        assert_eq!(
            mirror_copy(&m, &mesh_ref("L"), &[0], &d, &[0], 0.0).unwrap_err(),
            MirrorError::Kind
        );
        assert_eq!(
            mirror_copy(&m, &mesh_ref("L"), &[9], &mesh_ref("R"), &[0], 0.0).unwrap_err(),
            MirrorError::NoKey("L".into())
        );
    }

    fn deformers() -> Model {
        let mut m = model();
        for (n, x) in [("WL", -2.0), ("WR", 0.0)] {
            m.apply(Command::AddWarpDeformer {
                name: n.into(),
                part: "Part".into(),
                parent: None,
                rows: 1,
                columns: 2,
                base_points: vec![
                    [x, 0.0],
                    [x + 1.0, 0.0],
                    [x + 2.0, 0.0],
                    [x, 1.0],
                    [x + 1.0, 1.0],
                    [x + 2.0, 1.0],
                ],
            })
            .unwrap();
        }
        for (n, x) in [("RL", -1.0), ("RR", 1.0)] {
            m.apply(Command::AddRotationDeformer {
                name: n.into(),
                part: "Part".into(),
                parent: None,
                base: RotationBase {
                    origin: [x, 3.0],
                    angle_degrees: 0.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                },
            })
            .unwrap();
        }
        let elements = ["WL", "WR", "RL", "RR"].map(|n| ElementRef::Deformer { name: n.into() });
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: elements.to_vec(),
        })
        .unwrap();
        m
    }

    fn deformer_ref(n: &str) -> ElementRef {
        ElementRef::Deformer { name: n.into() }
    }

    #[test]
    fn a_warp_grid_is_mirrored_and_its_columns_swapped() {
        let m = deformers();
        let (command, _) = mirror_copy(
            &m,
            &deformer_ref("WL"),
            &[0],
            &deformer_ref("WR"),
            &[0],
            0.0,
        )
        .unwrap();
        let Command::SetWarpKeyform { points, .. } = command else {
            panic!("a warp keyform");
        };
        // WL spans x -2..0; its mirror spans 0..2 with the columns reversed so
        // that column 0 is still the left one.
        assert_eq!(
            points,
            [
                [0.0, 0.0],
                [1.0, 0.0],
                [2.0, 0.0],
                [0.0, 1.0],
                [1.0, 1.0],
                [2.0, 1.0]
            ]
        );
    }

    #[test]
    fn a_rotation_keeps_its_scale_flips_the_angle_and_mirrors_the_origin() {
        let mut m = deformers();
        m.apply(Command::SetRotationKeyform {
            name: "RL".into(),
            key: vec![2],
            base: RotationBase {
                origin: [-1.0, 3.0],
                angle_degrees: 30.0,
                scale: 2.0,
                reflect_x: false,
                reflect_y: true,
            },
            opacity: 0.25,
        })
        .unwrap();
        let (command, _) = mirror_copy(
            &m,
            &deformer_ref("RL"),
            &[2],
            &deformer_ref("RR"),
            &[2],
            0.0,
        )
        .unwrap();
        let Command::SetRotationKeyform {
            base, opacity, key, ..
        } = command
        else {
            panic!("a rotation keyform");
        };
        assert_eq!(key, [2]);
        assert_eq!(opacity, 0.25);
        assert_eq!(base.origin, [1.0, 3.0]);
        assert_eq!((base.angle_degrees, base.scale), (-30.0, 2.0));
        assert!(base.reflect_y && !base.reflect_x);
        // A warp and a rotation do not mirror onto each other.
        assert_eq!(
            mirror_copy(
                &m,
                &deformer_ref("WL"),
                &[0],
                &deformer_ref("RR"),
                &[0],
                0.0
            )
            .unwrap_err(),
            MirrorError::Kind
        );
    }

    #[test]
    fn the_default_center_is_the_middle_of_the_meshes() {
        let m = model();
        assert_eq!(model_center(&m), 0.0);
        assert_eq!(center(&m, ""), 0.0);
        assert_eq!(center(&m, " 2.5 "), 2.5);
    }
}
