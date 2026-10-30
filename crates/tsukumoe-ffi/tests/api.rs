// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The C API called from Rust: results equal the `Runtime` used directly, bad
//! arguments give status codes, and the pointer rules hold.

use std::ffi::{CStr, CString, c_char};
use std::ptr::{null, null_mut};

use tsukumoe_core::arena::Arena;
use tsukumoe_core::elements::{Parameter, Part};
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Segment,
};
use tsukumoe_core::rig::{ArtMesh, MeshKeyform};
use tsukumoe_core::table::Table;
use tsukumoe_core::tmod::{self, AlignedBuf, PlaybackData, Tmod};
use tsukumoe_ffi::*;
use tsukumoe_runtime::{AutoConfig, BlinkConfig, BreathConfig, Priority, Runtime};

const QUAD: &[u8] = include_bytes!("../../../docs/specifications/samples/quad.tmod");
const TRI: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(tsukumoe_last_error_message()) }
        .to_str()
        .unwrap()
        .to_owned()
}

fn text(p: *const c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned()
}

/// Parameters P, Q, EyeL, EyeR, Breath; mesh `A` moves with P, mesh `B` is
/// fixed and masked by `A`; a motion on P, an expression on Q, one texture.
fn rich_model() -> Vec<u8> {
    let mut parts = Table::new();
    let part = parts.insert("Part", Part::default()).unwrap();
    let mut parameters = Table::new();
    let mut add = |name, min, max, default| {
        parameters
            .insert(name, Parameter::new(min, max, default))
            .unwrap()
    };
    let p = add("P", 0.0, 1.0, 0.0);
    let q = add("Q", -1.0, 1.0, 0.0);
    add("EyeL", 0.0, 1.0, 1.0);
    add("EyeR", 0.0, 1.0, 1.0);
    add("Breath", 0.0, 1.0, 0.0);
    let mut bindings = Arena::new();
    let binding = bindings.insert(
        Binding::new(vec![Axis {
            parameter: p,
            keys: vec![0.0, 1.0],
        }])
        .unwrap(),
    );
    let keyform = |x, opacity| MeshKeyform {
        positions: TRI.iter().map(|v| [v[0] + x, v[1]]).collect(),
        opacity,
        draw_order: 500.0,
        multiply_color: [1.0; 3],
        screen_color: [0.0; 3],
    };
    let mut meshes = Table::new();
    let mut a = ArtMesh::new(part, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]);
    a.keyforms = Some(KeyformGrid::new(
        binding,
        vec![keyform(0.0, 1.0), keyform(2.0, 0.5)],
    ));
    let a = meshes.insert("A", a).unwrap();
    let mut b = ArtMesh::new(part, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]);
    b.masks = vec![a];
    b.texture = 1;
    meshes.insert("B", b).unwrap();
    let mut motions = Table::new();
    motions
        .insert(
            "wave",
            Motion {
                duration: 2.0,
                fps: 30.0,
                looped: true,
                fade_in: 0.5,
                fade_out: 0.5,
                curves: vec![Curve {
                    target: CurveTarget::Parameter(p),
                    start: [0.0, 0.0],
                    segments: vec![Segment::Linear([1.0, 1.0]), Segment::Linear([2.0, 0.0])],
                    fade_in: None,
                    fade_out: None,
                }],
            },
        )
        .unwrap();
    let mut expressions = Table::new();
    expressions
        .insert(
            "tilt",
            Expression {
                fade_in: 0.5,
                fade_out: 0.5,
                parameters: vec![ExpressionParameter {
                    parameter: q,
                    blend: ExpressionBlend::Add,
                    value: 0.75,
                }],
            },
        )
        .unwrap();
    let m = Model::from_parts(ModelParts {
        parameters,
        parts,
        meshes,
        motions,
        expressions,
        bindings,
        ..ModelParts::default()
    })
    .unwrap();
    let png = Tmod::parse(&AlignedBuf::from_bytes(QUAD))
        .unwrap()
        .texture(0)
        .unwrap()
        .to_vec();
    let compiled = m.compile().unwrap();
    tmod::write_with(
        &compiled,
        &PlaybackData::from_model(&m).unwrap(),
        &[&png, &png],
    )
    .unwrap()
}

struct Handle(*mut TsukumoeRuntime);

impl Handle {
    fn load(bytes: &[u8]) -> Self {
        let mut h = null_mut();
        let s = unsafe { tsukumoe_runtime_load(bytes.as_ptr(), bytes.len(), &mut h) };
        assert_eq!(s, TsukumoeStatus::Ok, "{}", last_error());
        Self(h)
    }

    fn index(&self, name: &str) -> u32 {
        let mut i = 0;
        let s = unsafe { tsukumoe_runtime_parameter_index(self.0, c(name).as_ptr(), &mut i) };
        assert_eq!(s, TsukumoeStatus::Ok);
        i
    }

    fn buffers(&self) -> TsukumoeDrawBuffers {
        let mut b = std::mem::MaybeUninit::uninit();
        let s = unsafe { tsukumoe_runtime_draw_buffers(self.0, b.as_mut_ptr()) };
        assert_eq!(s, TsukumoeStatus::Ok);
        unsafe { b.assume_init() }
    }

    fn drawable(&self, position: u32) -> TsukumoeDrawable {
        let mut d = std::mem::MaybeUninit::uninit();
        let s = unsafe { tsukumoe_runtime_drawable(self.0, position, d.as_mut_ptr()) };
        assert_eq!(s, TsukumoeStatus::Ok, "{}", last_error());
        unsafe { d.assume_init() }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        assert_eq!(
            unsafe { tsukumoe_runtime_destroy(self.0) },
            TsukumoeStatus::Ok
        );
    }
}

fn bits(p: *const f32, n: usize) -> Vec<u32> {
    unsafe { std::slice::from_raw_parts(p, n) }
        .iter()
        .map(|f| f.to_bits())
        .collect()
}

#[test]
fn the_c_api_matches_the_runtime_bit_for_bit() {
    let bytes = rich_model();
    let mut rt = Runtime::from_bytes(&bytes).unwrap();
    let h = Handle::load(&bytes);
    let ok = TsukumoeStatus::Ok;

    let mut blink = BlinkConfig::new("EyeL", "EyeR");
    blink.interval = 1.0;
    blink.seed = 7;
    rt.set_auto(&AutoConfig {
        blink: Some(blink),
        breath: Some(BreathConfig {
            parameter: "Breath".into(),
            cycle: 3.0,
            amplitude: 0.4,
            offset: 0.5,
        }),
    })
    .unwrap();
    let (l, r, br) = (c("EyeL"), c("EyeR"), c("Breath"));
    let config = TsukumoeAutoConfig {
        size: size_of::<TsukumoeAutoConfig>() as u32,
        flags: TSUKUMOE_AUTO_BLINK | TSUKUMOE_AUTO_BREATH,
        blink_seed: 7,
        blink_left_eye: l.as_ptr(),
        blink_right_eye: r.as_ptr(),
        blink_interval: 1.0,
        blink_close_time: 0.1,
        blink_closed_time: 0.05,
        blink_open_time: 0.2,
        breath_parameter: br.as_ptr(),
        breath_cycle: 3.0,
        breath_amplitude: 0.4,
        breath_offset: 0.5,
    };
    assert_eq!(unsafe { tsukumoe_runtime_set_auto(h.0, &config) }, ok);

    rt.play_motion("wave", 0, Priority::Normal).unwrap();
    let mut started = 9;
    let name = c("wave");
    assert_eq!(
        unsafe { tsukumoe_runtime_play_motion(h.0, name.as_ptr(), 0, 1, &mut started) },
        ok
    );
    assert_eq!(started, 1);
    rt.play_expression("tilt").unwrap();
    assert_eq!(
        unsafe { tsukumoe_runtime_play_expression(h.0, c("tilt").as_ptr()) },
        ok
    );
    let q = h.index("Q");
    let eye = h.index("EyeL");
    assert_eq!(q as usize, rt.parameter_index("Q").unwrap());

    let mut moved = std::collections::HashSet::new();
    for step in 0..180 {
        if step == 100 {
            rt.set_parameter(q as usize, -0.25);
            assert_eq!(unsafe { tsukumoe_runtime_set_parameter(h.0, q, -0.25) }, ok);
        }
        if step == 140 {
            rt.stop(0);
            assert_eq!(unsafe { tsukumoe_runtime_stop_motion(h.0, 0) }, ok);
        }
        rt.update(1.0 / 60.0);
        assert_eq!(unsafe { tsukumoe_runtime_update(h.0, 1.0 / 60.0) }, ok);

        let list = rt.draw_list();
        let b = h.buffers();
        assert_eq!(b.drawable_count as usize, list.len());
        assert_eq!(b.order_changed, list.order_changed());
        assert_eq!(
            unsafe { std::slice::from_raw_parts(b.order, list.len()) },
            list.order()
        );
        assert_eq!(
            bits(b.positions, b.vertex_count as usize * 2),
            bits(list.positions().as_ptr(), list.positions().len())
        );
        assert_eq!(
            bits(b.uvs, b.vertex_count as usize * 2),
            bits(list.uvs().as_ptr(), list.uvs().len())
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(b.indices, b.index_count as usize) },
            list.indices()
        );
        for (pos, d) in list.drawables().enumerate() {
            let c = h.drawable(pos as u32);
            assert_eq!(c.mesh as usize, d.mesh);
            assert_eq!(text(c.name), d.name);
            assert_eq!(c.texture, d.texture);
            assert_eq!(
                bits(c.positions, c.vertex_count as usize * 2),
                bits(d.positions.as_ptr(), d.positions.len())
            );
            assert_eq!(c.opacity.to_bits(), d.opacity.to_bits());
            assert_eq!(c.draw_order.to_bits(), d.draw_order.to_bits());
            assert_eq!(
                c.multiply_color.map(f32::to_bits),
                d.multiply_color.map(f32::to_bits)
            );
            assert_eq!(
                c.screen_color.map(f32::to_bits),
                d.screen_color.map(f32::to_bits)
            );
            assert_eq!(
                unsafe { std::slice::from_raw_parts(c.masks, c.mask_count as usize) },
                d.masks
            );
            assert_eq!(c.mask_inverted, d.mask_inverted);
            assert_eq!(c.geometry_changed, d.geometry_changed);
            assert_eq!(c.style_changed, d.style_changed);
            assert_eq!(
                (c.vertex_start as usize, c.index_start as usize),
                (d.vertex_start, d.index_start)
            );
        }
        moved.insert(bits(h.drawable(0).positions, 6));
        let (mut v, mut playing) = (0.0, 9);
        assert_eq!(
            unsafe { tsukumoe_runtime_parameter_value(h.0, eye, &mut v) },
            ok
        );
        assert_eq!(v.to_bits(), rt.parameter_values()[eye as usize].to_bits());
        assert_eq!(
            unsafe { tsukumoe_runtime_is_motion_playing(h.0, 0, &mut playing) },
            ok
        );
        assert_eq!(playing != 0, rt.is_playing(0));
    }
    // The moving mesh really moved, so the comparison above was not trivial.
    assert!(moved.len() > 10);
}

#[test]
fn names_counts_and_textures_are_reported() {
    let h = Handle::load(&rich_model());
    let n = |f: unsafe extern "C" fn(*const TsukumoeRuntime, *mut u32) -> TsukumoeStatus| {
        let mut n = 0;
        assert_eq!(unsafe { f(h.0, &mut n) }, TsukumoeStatus::Ok);
        n
    };
    assert_eq!(n(tsukumoe_runtime_parameter_count), 5);
    assert_eq!(n(tsukumoe_runtime_motion_count), 1);
    assert_eq!(n(tsukumoe_runtime_expression_count), 1);
    assert_eq!(n(tsukumoe_runtime_texture_count), 2);
    let mut info = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { tsukumoe_runtime_parameter_info(h.0, 1, info.as_mut_ptr()) },
        TsukumoeStatus::Ok
    );
    let info = unsafe { info.assume_init() };
    assert_eq!(
        (
            text(info.name),
            info.min,
            info.max,
            info.default_value,
            info.repeat
        ),
        ("Q".into(), -1.0, 1.0, 0.0, false)
    );
    let mut name = null();
    assert_eq!(
        unsafe { tsukumoe_runtime_motion_name(h.0, 0, &mut name) },
        TsukumoeStatus::Ok
    );
    assert_eq!(text(name), "wave");
    assert_eq!(
        unsafe { tsukumoe_runtime_expression_name(h.0, 0, &mut name) },
        TsukumoeStatus::Ok
    );
    assert_eq!(text(name), "tilt");

    let (mut data, mut len) = (null(), 0);
    assert_eq!(
        unsafe { tsukumoe_runtime_texture(h.0, 1, &mut data, &mut len) },
        TsukumoeStatus::Ok
    );
    let png = unsafe { std::slice::from_raw_parts(data, len as usize) };
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(h.buffers().drawable_count, 2);
}

#[test]
fn bad_arguments_return_status_codes() {
    let bytes = rich_model();
    let h = Handle::load(&bytes);
    let (ok, null_arg, invalid, range, not_found) = (
        TsukumoeStatus::Ok,
        TsukumoeStatus::NullArgument,
        TsukumoeStatus::InvalidData,
        TsukumoeStatus::OutOfRange,
        TsukumoeStatus::NotFound,
    );
    let mut out = null_mut();
    let mut n = 0u32;
    let mut f = 0f32;
    let name = c("nothing");
    let bad_utf8 = CString::new(vec![0xff, 0xfe]).unwrap();

    unsafe {
        // NULL
        assert_eq!(tsukumoe_runtime_load(null(), 4, &mut out), null_arg);
        assert!(out.is_null());
        assert_eq!(
            tsukumoe_runtime_load(bytes.as_ptr(), bytes.len(), null_mut()),
            null_arg
        );
        assert_eq!(tsukumoe_runtime_update(null_mut(), 0.1), null_arg);
        assert_eq!(last_error(), "runtime is NULL");
        assert_eq!(tsukumoe_runtime_parameter_count(h.0, null_mut()), null_arg);
        assert_eq!(
            tsukumoe_runtime_parameter_index(h.0, null(), &mut n),
            null_arg
        );
        assert_eq!(tsukumoe_runtime_play_expression(h.0, null()), null_arg);
        assert_eq!(tsukumoe_runtime_set_auto(h.0, null()), null_arg);
        assert_eq!(tsukumoe_runtime_drawable(h.0, 0, null_mut()), null_arg);
        assert_eq!(tsukumoe_runtime_destroy(null_mut()), ok);

        // broken data
        for broken in [
            &bytes[..0],
            &bytes[..20],
            &bytes[..bytes.len() - 1],
            b"not a tmod file at all, but long enough to read a header from",
        ] {
            assert_eq!(
                tsukumoe_runtime_load(broken.as_ptr(), broken.len(), &mut out),
                invalid
            );
            assert!(out.is_null());
            assert!(!last_error().is_empty());
        }
        let mut flipped = bytes.clone();
        flipped[0] ^= 0xff;
        assert_eq!(
            tsukumoe_runtime_load(flipped.as_ptr(), flipped.len(), &mut out),
            invalid
        );
        assert_eq!(
            tsukumoe_runtime_load(bytes.as_ptr(), usize::MAX, &mut out),
            range
        );

        // out of range
        assert_eq!(
            tsukumoe_runtime_parameter_info(h.0, 5, &mut std::mem::zeroed()),
            range
        );
        assert_eq!(last_error(), "parameter 5 is out of range (count 5)");
        assert_eq!(tsukumoe_runtime_set_parameter(h.0, 99, 1.0), range);
        assert_eq!(tsukumoe_runtime_clear_parameter(h.0, 99), range);
        assert_eq!(tsukumoe_runtime_parameter_value(h.0, 99, &mut f), range);
        assert_eq!(tsukumoe_runtime_motion_name(h.0, 1, &mut null()), range);
        assert_eq!(tsukumoe_runtime_texture(h.0, 2, &mut null(), &mut n), range);
        assert_eq!(tsukumoe_runtime_stop_motion(h.0, 16), range);
        assert_eq!(tsukumoe_runtime_is_motion_playing(h.0, 16, &mut n), range);
        assert_eq!(
            tsukumoe_runtime_drawable(h.0, 2, &mut std::mem::zeroed()),
            range
        );
        let wave = c("wave");
        assert_eq!(
            tsukumoe_runtime_play_motion(h.0, wave.as_ptr(), 16, 1, null_mut()),
            range
        );
        assert_eq!(
            tsukumoe_runtime_play_motion(h.0, wave.as_ptr(), 0, 3, null_mut()),
            range
        );
        assert_eq!(
            tsukumoe_runtime_play_motion(h.0, wave.as_ptr(), 0, -1, null_mut()),
            range
        );

        // missing names
        assert_eq!(
            tsukumoe_runtime_parameter_index(h.0, name.as_ptr(), &mut n),
            not_found
        );
        assert_eq!(last_error(), "no parameter named 'nothing'");
        assert_eq!(
            tsukumoe_runtime_play_motion(h.0, name.as_ptr(), 0, 1, null_mut()),
            not_found
        );
        assert_eq!(
            tsukumoe_runtime_play_expression(h.0, name.as_ptr()),
            not_found
        );
        assert_eq!(
            tsukumoe_runtime_stop_expression(h.0, name.as_ptr()),
            not_found
        );
        assert_eq!(
            tsukumoe_runtime_parameter_index(h.0, bad_utf8.as_ptr(), &mut n),
            invalid
        );

        // set_auto: size, unknown parameter, bad number; nothing changes on an error
        let eye = c("EyeL");
        let mut config: TsukumoeAutoConfig = std::mem::zeroed();
        config.flags = TSUKUMOE_AUTO_BLINK;
        (
            config.blink_interval,
            config.blink_close_time,
            config.blink_closed_time,
            config.blink_open_time,
        ) = (1.0, 0.1, 0.05, 0.2);
        config.blink_left_eye = eye.as_ptr();
        config.blink_right_eye = name.as_ptr();
        assert_eq!(tsukumoe_runtime_set_auto(h.0, &config), range);
        config.size = size_of::<TsukumoeAutoConfig>() as u32;
        assert_eq!(tsukumoe_runtime_set_auto(h.0, &config), not_found);
        config.blink_right_eye = eye.as_ptr();
        config.blink_interval = 0.0;
        assert_eq!(tsukumoe_runtime_set_auto(h.0, &config), range);
        config.blink_interval = 1.0;
        assert_eq!(tsukumoe_runtime_set_auto(h.0, &config), ok);
        config.flags = 0;
        assert_eq!(tsukumoe_runtime_set_auto(h.0, &config), ok);
    }
    // The handle still works after all of it.
    assert_eq!(unsafe { tsukumoe_runtime_update(h.0, 0.1) }, ok);
}

#[test]
fn what_the_runtime_returns_follows_the_lifetime_rules() {
    let mut bytes = rich_model();
    let h = Handle::load(&bytes);
    // The runtime copied the data: the caller's buffer may go away.
    bytes.fill(0xAA);
    drop(bytes);

    let (mut name, mut data, mut len) = (null(), null(), 0);
    let mut info = std::mem::MaybeUninit::uninit();
    unsafe {
        assert_eq!(
            tsukumoe_runtime_motion_name(h.0, 0, &mut name),
            TsukumoeStatus::Ok
        );
        assert_eq!(
            tsukumoe_runtime_texture(h.0, 0, &mut data, &mut len),
            TsukumoeStatus::Ok
        );
        assert_eq!(
            tsukumoe_runtime_parameter_info(h.0, 0, info.as_mut_ptr()),
            TsukumoeStatus::Ok
        );
    }
    let param_name = unsafe { info.assume_init() }.name;
    let (png, mesh_name) = (
        unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec(),
        text(h.drawable(0).name),
    );

    // Names and textures stay valid through updates and playback calls (until destroy)...
    for _ in 0..10 {
        assert_eq!(
            unsafe { tsukumoe_runtime_update(h.0, 0.1) },
            TsukumoeStatus::Ok
        );
        let wave = c("wave");
        assert_eq!(
            unsafe { tsukumoe_runtime_play_motion(h.0, wave.as_ptr(), 0, 1, null_mut()) },
            TsukumoeStatus::Ok
        );
    }
    assert_eq!(text(name), "wave");
    assert_eq!(text(param_name), "P");
    assert_eq!(
        unsafe { std::slice::from_raw_parts(data, len as usize) },
        png
    );
    assert_eq!(text(h.drawable(0).name), mesh_name);

    // ...and the draw buffers are read in place: one update later they show the new frame.
    let set = |v| {
        assert_eq!(
            unsafe { tsukumoe_runtime_set_parameter(h.0, 0, v) },
            TsukumoeStatus::Ok
        )
    };
    set(1.0);
    unsafe { tsukumoe_runtime_update(h.0, 0.0) };
    let moved = bits(h.buffers().positions, 6);
    set(0.0);
    unsafe { tsukumoe_runtime_update(h.0, 0.0) };
    assert_ne!(bits(h.buffers().positions, 6), moved);

    // Two runtimes are independent, and a failed call on one leaves the other alone.
    let other = Handle::load(&rich_model());
    assert_eq!(
        unsafe { tsukumoe_runtime_set_parameter(other.0, 99, 0.0) },
        TsukumoeStatus::OutOfRange
    );
    assert_eq!(
        unsafe { tsukumoe_runtime_update(h.0, 0.1) },
        TsukumoeStatus::Ok
    );
    assert_eq!(last_error(), "");
}

#[test]
fn the_version_is_one_zero() {
    assert_eq!(tsukumoe_abi_version(), 1 << 16);
}
