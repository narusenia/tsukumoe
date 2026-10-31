// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tracking mapping output through the runtime and a parameter driver: the
//! mapped parameters are set, and the driver's expression sees them.

use tsukumoe_core::driver::Driver;
use tsukumoe_core::elements::{Parameter, Part, Point};
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::rig::{ArtMesh, MeshKeyform};
use tsukumoe_core::tmod::PlaybackData;
use tsukumoe_runtime::Runtime;
use tsukumoe_tracking::{Mapper, Overrides, TrackingFrame, arkit_index, default_rules};

fn quad(x: f32) -> Vec<Point> {
    vec![[x, 0.0], [x + 1.0, 0.0], [x + 1.0, 1.0], [x, 1.0]]
}

/// Standard-ID parameters; `Out` moves the quad from x = 0 to x = 10 and is
/// driven by `expression`.
fn model(expression: &str) -> Model {
    let mut p = ModelParts::default();
    let part = p.parts.insert("Part", Part::default()).unwrap();
    let mut add = |name, min, max, default| {
        p.parameters
            .insert(name, Parameter::new(min, max, default))
            .unwrap()
    };
    add("ParamAngleX", -30.0, 30.0, 0.0);
    add("ParamMouthOpenY", 0.0, 1.0, 0.0);
    let out = add("Out", 0.0, 1.0, 0.0);
    let binding = p.bindings.insert(
        Binding::new(vec![Axis {
            parameter: out,
            keys: vec![0.0, 1.0],
        }])
        .unwrap(),
    );
    let mut mesh = ArtMesh::new(part, quad(0.0), vec![[0.5, 0.5]; 4], vec![0, 1, 2, 0, 2, 3]);
    let keyform = |x| MeshKeyform {
        positions: quad(x),
        opacity: 1.0,
        draw_order: 500.0,
        multiply_color: [1.0; 3],
        screen_color: [0.0; 3],
    };
    mesh.keyforms = Some(KeyformGrid::new(binding, vec![keyform(0.0), keyform(10.0)]));
    p.meshes.insert("Quad", mesh).unwrap();
    p.drivers
        .insert(
            "D",
            Driver {
                output: out,
                expression: expression.into(),
            },
        )
        .unwrap();
    Model::from_parts(p).unwrap()
}

fn runtime(m: &Model) -> Runtime<'static> {
    Runtime::new(m.compile().unwrap(), PlaybackData::from_model(m).unwrap()).unwrap()
}

fn names(rt: &Runtime) -> Vec<String> {
    let m = rt.model();
    (0..m.parameters.len())
        .map(|i| m.parameter_name(i).to_string())
        .collect()
}

/// The x of the quad's first vertex after feeding `frame` through `mapper`.
fn quad_x(rt: &mut Runtime, mapper: &Mapper, frame: &TrackingFrame) -> f32 {
    let mut out = Vec::with_capacity(mapper.len());
    mapper.apply(frame, &mut out);
    for &(i, v) in &out {
        assert!(rt.set_parameter(i, v));
    }
    rt.update(0.016);
    rt.draw_list().drawable(0).positions[0]
}

#[test]
fn mapped_parameters_feed_the_driver_expression() {
    let m = model("ParamMouthOpenY + ParamAngleX / 100");
    let mut rt = runtime(&m);
    let (mapper, skipped) = Mapper::bind(&default_rules(), &names(&rt));
    assert_eq!(mapper.len(), 2);
    assert!(skipped > 0);

    let mut f = TrackingFrame::default();
    f.blend[arkit_index("jawOpen").unwrap()] = 0.4;
    let (s, c) = (20f32.to_radians() / 2.0).sin_cos();
    f.head_rotation = [0.0, s, 0.0, c]; // yaw 20 degrees -> ParamAngleX = 20
    // Out = 0.4 + 20 / 100 = 0.6 -> x = 6
    let x = quad_x(&mut rt, &mapper, &f);
    assert!((x - 6.0).abs() < 1e-3, "{x}");
    let angle = rt.parameter_index("ParamAngleX").unwrap();
    assert!((rt.parameter_values()[angle] - 20.0).abs() < 1e-3);
}

#[test]
fn json_overrides_change_what_the_driver_sees() {
    let m = model("ParamMouthOpenY");
    let mut rt = runtime(&m);
    let json =
        r#"{"rules":[{"output":"ParamMouthOpenY","terms":[{"input":"jawOpen","weight":0.5}]}]}"#;
    let rules = Overrides::from_json(json).unwrap().resolve();
    let (mapper, _) = Mapper::bind(&rules, &names(&rt));
    let mut f = TrackingFrame::default();
    f.blend[arkit_index("jawOpen").unwrap()] = 1.0;
    let x = quad_x(&mut rt, &mapper, &f);
    assert!((x - 5.0).abs() < 1e-3, "{x}");
}
