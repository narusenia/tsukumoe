// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::collections::BTreeMap;
use std::io::Cursor;

use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Point;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::BlendMode;

use crate::writer::{GroupSpec, LayerSpec, Node, PsdSpec, write};
use crate::*;

/// A project in memory: the model, its texture files and the map.
#[derive(Default)]
struct Proj {
    model: Model,
    textures: BTreeMap<String, Vec<u8>>,
    map: Option<PsdMap>,
}

fn source() -> Source {
    Source {
        path: "x.psd".into(),
        sha256: "0".into(),
        canvas: [0, 0],
    }
}

fn scan_of(bytes: &[u8]) -> (Psd, Scan) {
    let limits = Limits::default();
    let psd = Psd::read(&mut Cursor::new(bytes), &limits).unwrap();
    let scan = Scan::new(&psd, &mut Cursor::new(bytes), &limits, |_, _| {}).unwrap();
    (psd, scan)
}

fn make_plan(p: &Proj, bytes: &[u8], choices: &Choices) -> (Psd, Scan, Plan) {
    let (psd, scan) = scan_of(bytes);
    let textures: Vec<String> = p.textures.keys().cloned().collect();
    let plan = plan(
        &scan,
        &Existing {
            model: &p.model,
            textures: &textures,
        },
        p.map.as_ref(),
        choices,
        &source(),
    );
    (psd, scan, plan)
}

/// Plans, applies the batch, then writes the textures and the map.
fn import(p: &mut Proj, bytes: &[u8], choices: &Choices) -> Plan {
    let (psd, scan, plan) = make_plan(p, bytes, choices);
    p.model.apply(plan.batch()).unwrap();
    let mut files = Vec::new();
    render_textures(
        &plan,
        &scan,
        &psd,
        &mut Cursor::new(bytes),
        &Limits::default(),
        |path, png| {
            files.push((path.to_owned(), png));
            Ok(())
        },
    )
    .unwrap();
    p.textures.extend(files);
    p.map = Some(plan.map.clone());
    assert!(p.model.validate().is_empty());
    plan
}

fn px(v: u8) -> [u8; 4] {
    [v, 50, 255 - v, 255]
}

fn layer(name: &str, l: i32, t: i32, w: u32, h: u32, v: u8) -> Node {
    Node::Layer(LayerSpec::solid(name, l, t, w, h, px(v)))
}

fn spec(nodes: Vec<Node>) -> Vec<u8> {
    write(&PsdSpec::new(100, 100, nodes))
}

fn base() -> Vec<u8> {
    let mut hidden = LayerSpec::solid("hidden", 0, 0, 4, 4, px(3));
    hidden.visible = false;
    hidden.opacity = 128;
    hidden.blend = *b"mul ";
    spec(vec![
        Node::Group(GroupSpec::new(
            "Face",
            vec![
                layer("eye", 10, 20, 8, 4, 10),
                Node::Group(GroupSpec::new("Hair", vec![Node::Layer(hidden)])),
            ],
        )),
        layer("bg", 0, 0, 100, 100, 1),
    ])
}

fn mesh<'a>(m: &'a Model, name: &str) -> &'a tsukumoe_core::rig::ArtMesh {
    m.meshes()
        .get(
            m.meshes()
                .find(name)
                .unwrap_or_else(|| panic!("no mesh {name}")),
        )
        .unwrap()
}

fn action<'a>(plan: &'a Plan, key: &str) -> &'a Action {
    &plan
        .items
        .iter()
        .find(|i| i.key == key)
        .unwrap_or_else(|| panic!("no item {key}"))
        .action
}

#[test]
fn a_first_import_builds_parts_meshes_and_one_undo_removes_it_all() {
    let mut p = Proj::default();
    let plan = import(&mut p, &base(), &Choices::default());
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    let parts: Vec<_> = p
        .model
        .parts()
        .iter()
        .map(|(_, n, _)| n.to_owned())
        .collect();
    assert_eq!(parts, ["PSD", "Face", "Face/Hair"]);
    let face = p.model.parts().find("Face").unwrap();
    let hair = p
        .model
        .parts()
        .get(p.model.parts().find("Face/Hair").unwrap())
        .unwrap();
    assert_eq!(hair.parent, Some(face));
    let root = p.model.parts().find("PSD").unwrap();
    assert_eq!(p.model.parts().get(face).unwrap().parent, Some(root));
    assert_eq!(mesh(&p.model, "bg").part, root);
    assert_eq!(mesh(&p.model, "Face/eye").part, face);

    let eye = mesh(&p.model, "Face/eye");
    // y is up; the canvas's top left is the origin; 1000 px are 1 unit.
    let near = |a: Point, b: Point| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    assert!(near(eye.positions[0], [0.010, -0.020]));
    assert!(near(eye.positions[2], [0.018, -0.024]));
    assert_eq!(eye.uvs, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let hidden = mesh(&p.model, "Face/Hair/hidden");
    assert!(!hidden.visible && hidden.blend_mode == BlendMode::Multiply);
    assert!((hidden.opacity - 128.0 / 255.0).abs() < 1e-6);
    // Stacking: bg is the bottom, then hidden, then eye.
    assert!(mesh(&p.model, "bg").draw_order < hidden.draw_order);
    assert!(hidden.draw_order < eye.draw_order);
    // One texture per layer, in path order.
    assert_eq!(p.textures.len(), 3);
    let mut textures: Vec<u32> = p.model.meshes().iter().map(|(_, _, m)| m.texture).collect();
    textures.sort_unstable();
    assert_eq!(textures, [0, 1, 2]);
    let map = p.map.as_ref().unwrap();
    assert_eq!(map.layers.len(), 3);
    assert!(p.model.compile().is_ok());

    assert!(p.model.undo());
    assert!(p.model == Model::new());
}

#[test]
fn plans_are_deterministic_and_a_reimport_of_the_same_psd_is_empty() {
    let mut p = Proj::default();
    let first = make_plan(&p, &base(), &Choices::default()).2;
    assert_eq!(first, make_plan(&p, &base(), &Choices::default()).2);
    import(&mut p, &base(), &Choices::default());
    let again = make_plan(&p, &base(), &Choices::default()).2;
    assert!(again.is_empty(), "{:?}", again.commands);
    assert!(again.items.iter().all(|i| i.action == Action::Unchanged));
}

#[test]
fn same_names_are_told_apart_from_the_top() {
    let bytes = spec(vec![
        layer("x", 0, 0, 2, 2, 1),
        layer("x", 5, 5, 2, 2, 2),
        layer("x", 9, 9, 2, 2, 3),
    ]);
    let mut p = Proj::default();
    import(&mut p, &bytes, &Choices::default());
    let names: Vec<_> = p
        .model
        .meshes()
        .iter()
        .map(|(_, n, _)| n.to_owned())
        .collect();
    assert_eq!(names, ["x#2", "x#1", "x"]);
    // The top layer is "x" and has the highest draw order.
    assert!(mesh(&p.model, "x").draw_order > mesh(&p.model, "x#2").draw_order);
    assert!(make_plan(&p, &bytes, &Choices::default()).2.is_empty());
}

#[test]
fn new_and_vanished_layers_and_choices() {
    let mut p = Proj::default();
    import(&mut p, &base(), &Choices::default());
    // "eye" vanishes, "mouth" is new.
    let next = spec(vec![
        Node::Group(GroupSpec::new(
            "Face",
            vec![layer("mouth", 10, 30, 6, 3, 20)],
        )),
        layer("bg", 0, 0, 100, 100, 1),
    ]);
    let plan = make_plan(&p, &next, &Choices::default()).2;
    assert_eq!(
        action(&plan, "Face/mouth"),
        &Action::AddLayer { restored: false }
    );
    assert_eq!(
        action(&plan, "Face/eye"),
        &Action::Missing { remove: false }
    );
    assert!(plan.notes.is_empty());
    // Keep (the default): the eye mesh stays.
    let mut keep = Proj {
        model: Model::new(),
        ..Proj::default()
    };
    import(&mut keep, &base(), &Choices::default());
    import(&mut keep, &next, &Choices::default());
    assert!(keep.model.meshes().find("Face/eye").is_some());
    assert!(keep.model.meshes().find("Face/mouth").is_some());
    assert_eq!(keep.map.as_ref().unwrap().layers.len(), 4);
    // Remove, for one layer.
    let choices = Choices {
        missing: [("Face/eye".to_owned(), true)].into(),
        ..Choices::default()
    };
    import(&mut p, &next, &choices);
    assert!(p.model.meshes().find("Face/eye").is_none());
    assert!(p.model.meshes().find("Face/mouth").is_some());
    assert!(
        p.map
            .as_ref()
            .unwrap()
            .layers
            .iter()
            .all(|l| l.key != "Face/eye")
    );
    // Undo brings the eye back.
    assert!(p.model.undo());
    assert!(p.model.meshes().find("Face/eye").is_some());
    assert!(p.model.meshes().find("Face/mouth").is_none());
}

#[test]
fn a_renamed_layer_is_matched_by_hand_and_keeps_its_mesh() {
    let mut p = Proj::default();
    import(&mut p, &base(), &Choices::default());
    let next = spec(vec![
        Node::Group(GroupSpec::new(
            "Face",
            vec![layer("eyeL", 10, 20, 8, 4, 10)],
        )),
        layer("bg", 0, 0, 100, 100, 1),
    ]);
    let plan = make_plan(&p, &next, &Choices::default()).2;
    let (missing, added) = plan.renamable();
    assert!(missing.contains(&"Face/eye") && added == ["Face/eyeL"]);
    let choices = Choices {
        rename: [("Face/eye".to_owned(), "Face/eyeL".to_owned())].into(),
        ..Choices::default()
    };
    let plan = import(&mut p, &next, &choices);
    assert!(matches!(
        action(&plan, "Face/eyeL"),
        Action::UpdateLayer { renamed_from: Some(from), image: false, .. } if from == "Face/eye"
    ));
    assert!(p.model.meshes().find("Face/eyeL").is_none());
    assert!(p.model.meshes().find("Face/eye").is_some());
    let map = p.map.as_ref().unwrap();
    assert!(
        map.layers
            .iter()
            .any(|l| l.key == "Face/eyeL" && l.mesh == "Face/eye")
    );
    assert!(make_plan(&p, &next, &Choices::default()).2.is_empty());
}

#[test]
fn a_changed_image_keeps_vertices_rig_and_canvas_pixel_positions() {
    let mut p = Proj::default();
    import(&mut p, &base(), &Choices::default());
    // The user edits the eye: moves a vertex and adds a second mesh point of view.
    let moved: Vec<Point> = vec![[0.5, 0.5], [1.5, 0.5], [1.5, 1.5], [0.5, 1.5]];
    p.model
        .apply(Command::SetMeshPositions {
            name: "Face/eye".into(),
            positions: moved.clone(),
        })
        .unwrap();
    // A UV the user placed at the canvas pixel (14, 22) of the old image (8x4 at 10,20).
    p.model
        .apply(Command::SetMeshUvs {
            name: "Face/eye".into(),
            uvs: vec![[0.5, 0.5], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        })
        .unwrap();
    // The new image is bigger and elsewhere: 20x10 at (4, 18).
    let next = spec(vec![
        Node::Group(GroupSpec::new(
            "Face",
            vec![
                layer("eye", 4, 18, 20, 10, 99),
                Node::Group(GroupSpec::new(
                    "Hair",
                    vec![{
                        let mut h = LayerSpec::solid("hidden", 0, 0, 4, 4, px(3));
                        h.visible = false;
                        h.opacity = 128;
                        h.blend = *b"mul ";
                        Node::Layer(h)
                    }],
                )),
            ],
        )),
        layer("bg", 0, 0, 100, 100, 1),
    ]);
    let before_tex = mesh(&p.model, "Face/eye").texture;
    let plan = import(&mut p, &next, &Choices::default());
    assert!(matches!(
        action(&plan, "Face/eye"),
        Action::UpdateLayer { image: true, .. }
    ));
    let eye = mesh(&p.model, "Face/eye");
    assert_eq!(eye.positions, moved, "vertices are not touched");
    assert_eq!(
        eye.texture, before_tex,
        "the texture file is replaced in place"
    );
    // Canvas pixel (14, 22) is now at ((14-4)/20, (22-18)/10).
    assert_eq!(eye.uvs[0], [0.5, 0.4]);
    assert_eq!(eye.uvs[1], [(18.0 - 4.0) / 20.0, (20.0 - 18.0) / 10.0]);
    let rec = p
        .map
        .as_ref()
        .unwrap()
        .layers
        .iter()
        .find(|l| l.key == "Face/eye")
        .unwrap();
    assert_eq!(rec.rect, [4, 18, 24, 28]);
    let png = &p.textures[&rec.texture];
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(make_plan(&p, &next, &Choices::default()).2.is_empty());
}

#[test]
fn psd_changes_update_unedited_values_only() {
    let mut p = Proj::default();
    import(&mut p, &base(), &Choices::default());
    // The user edits the opacity of the eye, not its blend mode.
    p.model
        .apply(Command::SetMeshOpacity {
            name: "Face/eye".into(),
            opacity: 0.3,
        })
        .unwrap();
    let mut eye = LayerSpec::solid("eye", 10, 20, 8, 4, px(10));
    eye.opacity = 100;
    eye.blend = *b"lddg";
    let mut hidden = LayerSpec::solid("hidden", 0, 0, 4, 4, px(3));
    hidden.visible = true; // was hidden
    hidden.opacity = 128;
    hidden.blend = *b"mul ";
    let next = spec(vec![
        Node::Group(GroupSpec::new(
            "Face",
            vec![
                Node::Layer(eye),
                Node::Group(GroupSpec::new("Hair", vec![Node::Layer(hidden)])),
            ],
        )),
        layer("bg", 0, 0, 100, 100, 1),
    ]);
    let plan = import(&mut p, &next, &Choices::default());
    let Action::UpdateLayer {
        set,
        kept,
        image: false,
        ..
    } = action(&plan, "Face/eye")
    else {
        panic!("{:?}", action(&plan, "Face/eye"))
    };
    assert_eq!(
        (set.as_slice(), kept.as_slice()),
        (&["blend_mode"][..], &["opacity"][..])
    );
    let eye = mesh(&p.model, "Face/eye");
    assert_eq!(eye.blend_mode, BlendMode::Additive);
    assert!((eye.opacity - 0.3).abs() < 1e-6, "the user's opacity stays");
    assert!(mesh(&p.model, "Face/Hair/hidden").visible);
}

#[test]
fn unsupported_blend_modes_warn_and_pass_is_silent() {
    let mut over = LayerSpec::solid("over", 0, 0, 2, 2, px(1));
    over.blend = *b"over";
    let bytes = spec(vec![
        Node::Group(GroupSpec::new("G", vec![Node::Layer(over)])),
        layer("n", 0, 0, 1, 1, 2),
    ]);
    let mut p = Proj::default();
    let plan = import(&mut p, &bytes, &Choices::default());
    assert_eq!(plan.notes.len(), 1);
    assert_eq!(plan.notes[0].code, "blend_mode");
    assert_eq!(mesh(&p.model, "G/over").blend_mode, BlendMode::Normal);
}

#[test]
fn clipping_becomes_a_mask_only_when_the_result_matches() {
    let mut clipped = LayerSpec::solid("shade", 0, 0, 4, 4, px(5));
    clipped.clipping = true;
    let ok = spec(vec![
        Node::Layer(clipped.clone()),
        layer("base", 0, 0, 8, 8, 6),
    ]);
    let mut p = Proj::default();
    let plan = import(&mut p, &ok, &Choices::default());
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    let base_id = p.model.meshes().find("base").unwrap();
    assert_eq!(mesh(&p.model, "shade").masks, [base_id]);
    assert!(make_plan(&p, &ok, &Choices::default()).2.is_empty());

    // Multiply and Additive clipped layers are masked too: the mask factor
    // scales the premultiplied source before the blend.
    for key in [b"mul ", b"lddg"] {
        let mut c = clipped.clone();
        c.blend = *key;
        let bytes = spec(vec![Node::Layer(c), layer("base", 0, 0, 8, 8, 6)]);
        let mut p = Proj::default();
        let plan = import(&mut p, &bytes, &Choices::default());
        assert!(plan.notes.is_empty(), "{:?}", plan.notes);
        let base_id = p.model.meshes().find("base").unwrap();
        assert_eq!(mesh(&p.model, "shade").masks, [base_id]);
    }

    // A blend with no counterpart falls back to Normal: still clipped, and
    // the note says the color is approximate.
    let mut over = clipped.clone();
    over.blend = *b"over";
    let bytes = spec(vec![Node::Layer(over), layer("base", 0, 0, 8, 8, 6)]);
    let mut p = Proj::default();
    let plan = import(&mut p, &bytes, &Choices::default());
    assert_eq!(plan.notes.len(), 1);
    assert_eq!(plan.notes[0].code, "blend_mode");
    assert!(plan.notes[0].message.contains("approximate"));
    assert_eq!(mesh(&p.model, "shade").masks.len(), 1);
    assert_eq!(mesh(&p.model, "shade").blend_mode, BlendMode::Normal);

    // A translucent base likewise.
    let mut base = LayerSpec::solid("base", 0, 0, 8, 8, px(6));
    base.opacity = 100;
    let bytes = spec(vec![Node::Layer(clipped), Node::Layer(base)]);
    let mut p = Proj::default();
    let plan = import(&mut p, &bytes, &Choices::default());
    assert!(plan.notes.iter().any(|n| n.code == "clipping"));
}

#[test]
fn transparent_layers_get_no_mesh_and_a_note() {
    let bytes = spec(vec![
        Node::Layer(LayerSpec::solid("clear", 0, 0, 3, 3, [1, 2, 3, 0])),
        layer("dot", 0, 0, 3, 3, 1),
    ]);
    let mut p = Proj::default();
    let plan = import(&mut p, &bytes, &Choices::default());
    assert!(
        plan.notes
            .iter()
            .any(|n| n.code == "empty_layer" && n.layer == "clear")
    );
    assert!(p.model.meshes().find("clear").is_none());
    assert_eq!(p.textures.len(), 1);
}

#[test]
fn a_failed_apply_changes_nothing() {
    let mut p = Proj::default();
    let (_, _, plan) = make_plan(&p, &base(), &Choices::default());
    p.model.apply(plan.batch()).unwrap();
    let after = p.model.history().commands().count();
    // Applying the same first-import plan again collides with existing names.
    assert!(p.model.apply(plan.batch()).is_err());
    assert_eq!(p.model.history().commands().count(), after);
    assert_eq!(p.model.parts().len(), 3);
    assert_eq!(p.model.meshes().len(), 3);
}

#[test]
fn texture_files_added_in_the_middle_renumber_other_meshes() {
    // An existing project texture that sorts after the import's files.
    let mut p = Proj::default();
    p.textures.insert("textures/zeta.png".into(), vec![1]);
    p.model
        .apply(Command::AddPart {
            name: "P".into(),
            parent: None,
        })
        .unwrap();
    p.model
        .apply(Command::AddMesh {
            name: "Old".into(),
            part: "P".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
        })
        .unwrap();
    assert_eq!(mesh(&p.model, "Old").texture, 0);
    import(&mut p, &base(), &Choices::default());
    // "zeta.png" is now the last of four files.
    assert_eq!(mesh(&p.model, "Old").texture, 3);
    let files: Vec<_> = p.textures.keys().cloned().collect();
    assert_eq!(files.last().unwrap(), "textures/zeta.png");
}
