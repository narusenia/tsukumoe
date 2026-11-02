// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Making a glue from two vertex selections (M5-05c-2). One side is marked
//! first (`mesh.glue-source`), the other is the selection of the mesh being
//! edited (`mesh.glue`). Vertices are paired nearest first, one to one, by
//! where they are drawn now. The result is one `AddGlue` command; the model
//! is not changed here (ADR-0019).

use std::collections::BTreeSet;

use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Point;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::GlueNode;

use crate::mesh_tool::Op;
use crate::scene::Scene;
use crate::view::ViewState;

/// Where both ends of a node meet: halfway.
const WEIGHT: f32 = 0.5;

/// Pairs the vertices of `a` with those of `b`, nearest pair first, each used
/// once. The shorter side decides how many pairs there are.
pub fn pair(a: &[(u32, Point)], b: &[(u32, Point)]) -> Vec<(u32, u32)> {
    let mut all: Vec<(f32, u32, u32)> = a
        .iter()
        .flat_map(|&(i, p)| {
            b.iter()
                .map(move |&(j, q)| ((p[0] - q[0]).hypot(p[1] - q[1]), i, j))
        })
        .collect();
    all.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (mut used_a, mut used_b) = (BTreeSet::new(), BTreeSet::new());
    let mut out = Vec::new();
    for (_, i, j) in all {
        if used_a.insert(i) {
            if used_b.insert(j) {
                out.push((i, j));
            } else {
                used_a.remove(&i);
            }
        }
    }
    out.sort_unstable();
    out
}

/// A name for a new glue: `Glue`, else `Glue2`, `Glue3`, ...
pub fn unique_name(model: &Model) -> String {
    (1..)
        .map(|i| {
            if i == 1 {
                "Glue".to_owned()
            } else {
                format!("Glue{i}")
            }
        })
        .find(|n| model.glues().find(n).is_none())
        .expect("an unused name exists")
}

/// The op that glues the marked vertices of the source mesh to `vertices` of
/// `mesh`; the error is a translation key. The label ends with the glue's name.
pub fn create(
    model: &Model,
    view: &ViewState,
    mesh: &str,
    vertices: &BTreeSet<u32>,
) -> Result<Op, &'static str> {
    let (source, marked) = view.glue_source.as_ref().ok_or("glue-err-no-source")?;
    if source == mesh {
        return Err("glue-err-same-mesh");
    }
    if marked.is_empty() || vertices.is_empty() {
        return Err("glue-err-no-vertices");
    }
    let mut scene = Scene::new(model).map_err(|_| "glue-err-no-vertices")?;
    scene.apply(view);
    let drawn = |name: &str, picked: &BTreeSet<u32>| -> Option<Vec<(u32, Point)>> {
        let all = scene.drawn_positions(name)?;
        picked
            .iter()
            .map(|&i| all.get(i as usize).map(|&p| (i, p)))
            .collect()
    };
    let a = drawn(source, marked).ok_or("glue-err-no-vertices")?;
    let b = drawn(mesh, vertices).ok_or("glue-err-no-vertices")?;
    let nodes = pair(&a, &b)
        .into_iter()
        .map(|(index_a, index_b)| GlueNode {
            index_a,
            index_b,
            weight: WEIGHT,
        })
        .collect();
    let name = unique_name(model);
    Ok((
        Command::AddGlue {
            name: name.clone(),
            mesh_a: source.clone(),
            mesh_b: mesh.to_owned(),
            nodes,
            intensity: 1.0,
        },
        format!("mesh_glue: {name}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_nearest_first_and_uses_each_vertex_once() {
        let a = [(0, [0.0, 0.0]), (1, [10.0, 0.0])];
        let b = [(5, [9.0, 0.0]), (6, [1.0, 0.0]), (7, [50.0, 0.0])];
        assert_eq!(pair(&a, &b), vec![(0, 6), (1, 5)]);
        // The shorter side decides the count.
        assert_eq!(pair(&a[..1], &b).len(), 1);
        assert!(pair(&[], &b).is_empty());
    }

    #[test]
    fn creating_needs_a_source_on_another_mesh_and_vertices() {
        let model = Model::new();
        let mut view = ViewState::default();
        let none = BTreeSet::from([0]);
        assert_eq!(
            create(&model, &view, "B", &none).unwrap_err(),
            "glue-err-no-source"
        );
        view.glue_source = Some(("A".into(), BTreeSet::from([0])));
        assert_eq!(
            create(&model, &view, "A", &none).unwrap_err(),
            "glue-err-same-mesh"
        );
        assert_eq!(
            create(&model, &view, "B", &BTreeSet::new()).unwrap_err(),
            "glue-err-no-vertices"
        );
    }
}
