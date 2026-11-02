// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Draw order and masks of meshes, as the inspector edits them: reordering a
//! mesh among the meshes of its part, and the meshes it may be masked by.

use tsukumoe_core::command::Command;
use tsukumoe_core::elements::ParameterId;
use tsukumoe_core::model::Model;

/// Where a mesh goes among the meshes of its part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Front,
    Forward,
    Backward,
    Back,
}

/// The highest draw order the inspector's field takes.
const MAX_ORDER: f32 = 1000.0;

/// The draw orders that give `name` its new place among the meshes of its
/// part, back to front; the other parts keep theirs. The part's own draw
/// orders (as drawn at `at`, made strictly increasing) are handed out again in
/// the new sequence, so meshes of other parts stay where they were relative to
/// the group. `None` if the mesh is not found, or already is there.
pub fn reorder(
    model: &Model,
    name: &str,
    place: Place,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<Command> {
    let part = model.meshes().get(model.meshes().find(name)?)?.part;
    let mut group: Vec<(String, f32)> = model
        .meshes()
        .iter()
        .filter(|(_, _, m)| m.part == part)
        .map(|(_, n, m)| {
            let order = crate::keyed::mesh_attrs(model, n, at).map_or(m.draw_order, |a| a.1);
            (n.to_owned(), order)
        })
        .collect();
    group.sort_by(|a, b| a.1.total_cmp(&b.1));
    let from = group.iter().position(|(n, _)| n == name)?;
    let to = match place {
        Place::Front => group.len() - 1,
        Place::Forward => (from + 1).min(group.len() - 1),
        Place::Backward => from.saturating_sub(1),
        Place::Back => 0,
    };
    if to == from {
        return None;
    }
    let mut slots: Vec<f32> = group.iter().map(|g| g.1).collect();
    for i in 1..slots.len() {
        slots[i] = slots[i].max(slots[i - 1] + 1.0);
    }
    // Leave the room above the top slot when strictness pushed it past the range.
    let over = (slots[slots.len() - 1] - MAX_ORDER).max(0.0);
    let item = group.remove(from);
    group.insert(to, item);
    let commands: Vec<Command> = group
        .iter()
        .zip(&slots)
        .filter_map(|((n, old), &slot)| {
            let order = (slot - over).max(0.0);
            (order != *old).then(|| Command::SetMeshDrawOrder {
                name: n.clone(),
                draw_order: order,
            })
        })
        .collect();
    (!commands.is_empty()).then_some(Command::Batch { commands })
}

/// The meshes `name` could be masked by: not itself, not one it masks
/// already, and none that is masked by it (which would be a loop).
pub fn mask_candidates(model: &Model, name: &str) -> Vec<String> {
    let Some(me) = model.meshes().find(name) else {
        return Vec::new();
    };
    let masked_by_me = |start: tsukumoe_core::rig::MeshId| {
        let mut stack = vec![start];
        let mut seen = Vec::new();
        while let Some(id) = stack.pop() {
            if id == me {
                return true;
            }
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            if let Some(m) = model.meshes().get(id) {
                stack.extend(m.masks.iter().copied());
            }
        }
        false
    };
    let current = model.meshes().get(me).map(|m| m.masks.clone());
    model
        .meshes()
        .iter()
        .filter(|(id, _, _)| {
            *id != me && !current.as_ref().is_some_and(|c| c.contains(id)) && !masked_by_me(*id)
        })
        .map(|(_, n, _)| n.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parts `P` (meshes A 100, B 200, C 300) and `Q` (D 250).
    fn model() -> Model {
        let mut m = Model::default();
        for part in ["P", "Q"] {
            m.apply(Command::AddPart {
                name: part.into(),
                parent: None,
            })
            .unwrap();
        }
        for (name, part, order) in [
            ("A", "P", 100.0),
            ("B", "P", 200.0),
            ("C", "P", 300.0),
            ("D", "Q", 250.0),
        ] {
            m.apply(Command::AddMesh {
                name: name.into(),
                part: part.into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
            })
            .unwrap();
            m.apply(Command::SetMeshDrawOrder {
                name: name.into(),
                draw_order: order,
            })
            .unwrap();
        }
        m
    }

    fn orders(c: Command) -> Vec<(String, f32)> {
        let Command::Batch { commands } = c else {
            panic!("a batch");
        };
        commands
            .into_iter()
            .map(|c| match c {
                Command::SetMeshDrawOrder { name, draw_order } => (name, draw_order),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_mesh_moves_among_its_part_and_other_parts_keep_their_orders() {
        let m = model();
        let at = |_: ParameterId| 0.0;
        // A to the front: A takes C's slot, B and C step back.
        let c = reorder(&m, "A", Place::Front, &at).unwrap();
        assert_eq!(
            orders(c),
            [
                ("B".into(), 100.0),
                ("C".into(), 200.0),
                ("A".into(), 300.0)
            ]
        );
        // One step forward swaps with the next one only.
        let c = reorder(&m, "A", Place::Forward, &at).unwrap();
        assert_eq!(orders(c), [("B".into(), 100.0), ("A".into(), 200.0)]);
        // Already at the end: nothing to do.
        assert!(reorder(&m, "C", Place::Front, &at).is_none());
        assert!(reorder(&m, "A", Place::Back, &at).is_none());
        // D is alone in its part.
        assert!(reorder(&m, "D", Place::Front, &at).is_none());
    }

    #[test]
    fn equal_orders_become_distinct_and_the_result_is_applied() {
        let mut m = model();
        for n in ["A", "B", "C"] {
            m.apply(Command::SetMeshDrawOrder {
                name: n.into(),
                draw_order: 500.0,
            })
            .unwrap();
        }
        let c = reorder(&m, "A", Place::Back, &|_| 0.0);
        // A is first among equals already (stable by index): no move.
        assert!(c.is_none());
        let c = reorder(&m, "C", Place::Back, &|_| 0.0).unwrap();
        m.apply(c).unwrap();
        let order = |n: &str| {
            m.meshes()
                .get(m.meshes().find(n).unwrap())
                .unwrap()
                .draw_order
        };
        assert!(order("C") < order("A") && order("A") < order("B"));
    }

    #[test]
    fn mask_candidates_leave_out_itself_current_masks_and_loops() {
        let mut m = model();
        m.apply(Command::SetMeshMasks {
            name: "B".into(),
            masks: vec!["A".into()],
            inverted: false,
        })
        .unwrap();
        m.apply(Command::SetMeshMasks {
            name: "C".into(),
            masks: vec!["B".into()],
            inverted: false,
        })
        .unwrap();
        // A may not be masked by B or C (they are masked by A), only by D.
        assert_eq!(mask_candidates(&m, "A"), ["D"]);
        // C already has B; A and D are free.
        assert_eq!(mask_candidates(&m, "C"), ["A", "D"]);
    }
}
