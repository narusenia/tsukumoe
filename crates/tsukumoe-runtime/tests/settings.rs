// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Model settings read back through the runtime: the info, the standard
//! parameters and the hit areas, which follow a deformed mesh.

use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_core::settings::StandardRole;
use tsukumoe_core::tmod;
use tsukumoe_runtime::Runtime;

fn model() -> Model {
    let mut m = Model::new();
    let steps = [
        Command::AddParameter {
            name: "P".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        },
        Command::AddPart {
            name: "Part".into(),
            parent: None,
        },
        Command::AddMesh {
            name: "Head".into(),
            part: "Part".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
        },
        Command::AddMesh {
            name: "Body".into(),
            part: "Part".into(),
            positions: vec![[-1.0, -3.0], [1.0, -3.0], [0.0, -1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
        },
        Command::SetModelInfo {
            title: "Demo".into(),
            author: "A".into(),
            license: "CC0".into(),
            notes: String::new(),
        },
        Command::AddHitArea {
            name: "Head".into(),
            mesh: "Head".into(),
        },
        Command::AddHitArea {
            name: "Body".into(),
            mesh: "Body".into(),
        },
        Command::SetStandardParameter {
            role: StandardRole::MouthOpen,
            parameter: Some("P".into()),
        },
    ];
    for step in steps {
        m.apply(step).unwrap();
    }
    m
}

fn runtime(m: &Model) -> Runtime<'static> {
    let bytes = tmod::write(&m.compile().unwrap(), &[]).unwrap();
    Runtime::from_bytes(&bytes).unwrap()
}

#[test]
fn the_runtime_reads_the_info_and_the_standard_parameters() {
    let rt = runtime(&model());
    let info = rt.model_info();
    assert_eq!(
        (info.title, info.author, info.license),
        ("Demo", "A", "CC0")
    );
    assert_eq!(
        rt.standard_parameter(StandardRole::MouthOpen),
        rt.parameter_index("P")
    );
    assert_eq!(rt.standard_parameter(StandardRole::AngleX), None);
    assert_eq!(rt.hit_areas().collect::<Vec<_>>(), ["Head", "Body"]);
}

#[test]
fn a_model_without_settings_has_none() {
    let rt = runtime(&Model::new());
    assert_eq!(rt.model_info().title, "");
    assert_eq!(rt.hit_areas().count(), 0);
    assert!(rt.hit_test(0.0, 0.0).is_empty());
}

#[test]
fn a_hit_test_names_the_areas_under_the_point() {
    let mut rt = runtime(&model());
    rt.update(0.0);
    assert_eq!(rt.hit_test(0.25, 0.25), ["Head"]);
    assert_eq!(rt.hit_test(0.0, -2.0), ["Body"]);
    assert!(rt.hit_test(5.0, 5.0).is_empty());
    // The edge counts as a hit.
    assert_eq!(rt.hit_test(1.0, 0.0), ["Head"]);
}

#[test]
fn a_hit_area_follows_the_mesh_as_it_moves() {
    let mut m = model();
    m.apply(Command::BindParameter {
        parameter: "P".into(),
        elements: vec![tsukumoe_core::command::ElementRef::Mesh {
            name: "Head".into(),
        }],
    })
    .unwrap();
    // Key 1 of the parameter moves the head 10 to the right.
    m.apply(Command::SetMeshKeyform {
        name: "Head".into(),
        key: vec![1],
        positions: vec![[10.0, 0.0], [11.0, 0.0], [10.0, 1.0]],
        opacity: 1.0,
        draw_order: 500.0,
    })
    .unwrap();
    let mut rt = runtime(&m);
    rt.update(0.0);
    assert_eq!(rt.hit_test(10.25, 0.25), Vec::<&str>::new());
    rt.set_parameter(rt.parameter_index("P").unwrap(), 1.0);
    rt.update(0.0);
    assert_eq!(rt.hit_test(10.25, 0.25), ["Head"]);
    assert!(rt.hit_test(0.25, 0.25).is_empty());
}
