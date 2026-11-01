// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::writer::{GroupSpec, LayerSpec, Node, PsdSpec, write};
use crate::*;

fn px(v: u8) -> [u8; 4] {
    [v, v.wrapping_mul(3), 255 - v, 200]
}

fn sample() -> PsdSpec {
    let mut hidden = LayerSpec::solid("hidden", 0, 0, 2, 2, px(9));
    hidden.visible = false;
    hidden.opacity = 128;
    hidden.blend = *b"mul ";
    let mut clip = LayerSpec::solid("clip", 3, 3, 2, 1, px(7));
    clip.clipping = true;
    let nested = GroupSpec::new(
        "inner 内側",
        vec![Node::Layer(LayerSpec::solid("日本語", 1, 2, 3, 2, px(40)))],
    );
    let mut outer = GroupSpec::new("outer", vec![Node::Group(nested), Node::Layer(hidden)]);
    outer.visible = false;
    outer.opacity = 100;
    PsdSpec::new(
        8,
        8,
        vec![
            Node::Layer(clip),
            Node::Group(outer),
            Node::Layer(LayerSpec::solid("bg", 0, 0, 8, 8, px(1))),
        ],
    )
}

fn read(bytes: &[u8]) -> Result<Psd, PsdError> {
    Psd::read(&mut Cursor::new(bytes), &Limits::default())
}

fn find<'a>(psd: &'a Psd, name: &str) -> (usize, &'a Entry) {
    psd.entries
        .iter()
        .enumerate()
        .find(|(_, e)| e.name == name)
        .unwrap_or_else(|| panic!("no entry {name}"))
}

fn check_sample(bytes: &[u8]) {
    let psd = read(bytes).unwrap();
    assert_eq!((psd.width, psd.height), (8, 8));
    let names: Vec<_> = psd.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        ["clip", "outer", "inner 内側", "日本語", "hidden", "bg"]
    );
    let (_, clip) = find(&psd, "clip");
    assert!(clip.clipping && clip.visible && clip.kind == EntryKind::Layer);
    let (_, outer) = find(&psd, "outer");
    assert_eq!(
        (outer.kind, outer.visible, outer.opacity),
        (EntryKind::Group, false, 100)
    );
    assert_eq!(outer.parent, None);
    let (jp_index, jp) = find(&psd, "日本語");
    assert_eq!(jp.path, ["outer", "inner 内側"]);
    assert_eq!(jp.parent, Some(2));
    assert_eq!(
        (jp.rect.left, jp.rect.top, jp.rect.width(), jp.rect.height()),
        (1, 2, 3, 2)
    );
    let (hi, hidden) = find(&psd, "hidden");
    assert_eq!(
        (
            hidden.visible,
            hidden.opacity,
            hidden.blend_key_str().as_str()
        ),
        (false, 128, "mul ")
    );
    assert_eq!(hidden.path, ["outer"]);
    // z grows toward the top: bg < hidden < 日本語 < clip
    assert!(find(&psd, "bg").1.z < hidden.z && hidden.z < jp.z && jp.z < clip.z);

    let limits = Limits::default();
    let mut cur = Cursor::new(bytes);
    let img = psd.decode_layer(&mut cur, jp_index, &limits).unwrap();
    assert_eq!((img.width, img.height), (3, 2));
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|p| *p == px(40)));
    let img = psd.decode_layer(&mut cur, hi, &limits).unwrap();
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|p| *p == px(9)));
    assert!(psd.decode_layer(&mut cur, 1, &limits).is_err()); // a group
}

#[test]
fn reads_raw_rle_and_16_bit_files() {
    for (rle, depth16) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut spec = sample();
        spec.rle = rle;
        spec.depth16 = depth16;
        check_sample(&write(&spec));
    }
}

#[test]
fn layers_in_an_lr16_block_are_found() {
    let mut spec = sample();
    spec.depth16 = true;
    spec.lr16 = true;
    check_sample(&write(&spec));
}

#[test]
fn rle_handles_long_runs_and_noise() {
    let mut pixels = Vec::new();
    for i in 0..300u32 {
        pixels.extend_from_slice(&if i < 200 { [5, 5, 5, 255] } else { px(i as u8) });
    }
    let mut layer = LayerSpec::solid("wide", 0, 0, 300, 1, [0; 4]);
    layer.pixels = pixels.clone();
    let mut spec = PsdSpec::new(300, 1, vec![Node::Layer(layer)]);
    spec.rle = true;
    let bytes = write(&spec);
    let psd = read(&bytes).unwrap();
    let img = psd
        .decode_layer(&mut Cursor::new(&bytes), 0, &Limits::default())
        .unwrap();
    assert_eq!(img.rgba, pixels);
}

#[test]
fn a_layer_without_alpha_channel_is_opaque_and_transparent_layers_have_no_bounds() {
    let transparent = LayerSpec::solid("clear", 0, 0, 3, 3, [10, 20, 30, 0]);
    let mut solid = LayerSpec::solid("dot", 0, 0, 4, 4, [0, 0, 0, 0]);
    solid.pixels[(2 * 4 + 1) * 4 + 3] = 255;
    let bytes = write(&PsdSpec::new(
        4,
        4,
        vec![Node::Layer(solid), Node::Layer(transparent)],
    ));
    let psd = read(&bytes).unwrap();
    let limits = Limits::default();
    let dot = psd
        .decode_layer(&mut Cursor::new(&bytes), 0, &limits)
        .unwrap();
    let r = dot.opaque_bounds().unwrap();
    assert_eq!((r.left, r.top, r.right, r.bottom), (1, 2, 2, 3));
    assert_eq!(dot.crop(r).rgba, [0, 0, 0, 255]);
    let clear = psd
        .decode_layer(&mut Cursor::new(&bytes), 1, &limits)
        .unwrap();
    assert_eq!(clear.opaque_bounds(), None);
}

#[test]
fn unsupported_features_are_clear_errors() {
    let mut spec = sample();
    spec.psb = true;
    assert_eq!(read(&write(&spec)).unwrap_err(), PsdError::Psb);
    let mut spec = sample();
    spec.color_mode = 4;
    assert_eq!(
        read(&write(&spec)).unwrap_err(),
        PsdError::UnsupportedColorMode(4)
    );
    let mut spec = sample();
    spec.compression_override = Some(2);
    let bytes = write(&spec);
    let psd = read(&bytes).unwrap();
    let (i, _) = find(&psd, "bg");
    let e = psd
        .decode_layer(&mut Cursor::new(&bytes), i, &Limits::default())
        .unwrap_err();
    assert!(matches!(
        e,
        PsdError::UnsupportedCompression { compression: 2, .. }
    ));
    assert!(e.to_string().contains("ZIP") || e.to_string().contains("compression"));
    assert_eq!(read(b"hello").unwrap_err(), PsdError::NotPsd);
}

#[test]
fn mask_adjustment_and_effects_are_warnings() {
    let mut a = LayerSpec::solid("masked", 0, 0, 1, 1, px(1));
    a.layer_mask = true;
    let mut b = LayerSpec::solid("adj", 0, 0, 1, 1, px(2));
    b.adjustment = Some(*b"levl");
    let mut c = LayerSpec::solid("fx", 0, 0, 1, 1, px(3));
    c.effects = true;
    let bytes = write(&PsdSpec::new(
        2,
        2,
        vec![Node::Layer(a), Node::Layer(b), Node::Layer(c)],
    ));
    let psd = read(&bytes).unwrap();
    assert_eq!(
        psd.warnings,
        [
            Warning {
                entry: 0,
                kind: WarningKind::LayerMask
            },
            Warning {
                entry: 1,
                kind: WarningKind::AdjustmentLayer("levl".into())
            },
            Warning {
                entry: 2,
                kind: WarningKind::LayerEffects
            },
        ]
    );
}

#[test]
fn limits_reject_before_allocating() {
    let bytes = write(&sample());
    let tight = |f: fn(&mut Limits)| {
        let mut l = Limits::default();
        f(&mut l);
        Psd::read(&mut Cursor::new(&bytes), &l).unwrap_err()
    };
    assert_eq!(
        tight(|l| l.max_canvas = 4),
        PsdError::TooLarge("canvas size")
    );
    assert_eq!(
        tight(|l| l.max_layers = 2),
        PsdError::TooLarge("layer count")
    );
    assert_eq!(
        tight(|l| l.max_layer_bytes = 16),
        PsdError::TooLarge("layer size")
    );
    assert_eq!(
        tight(|l| l.max_total_bytes = 100),
        PsdError::TooLarge("total layer size")
    );
}

/// Offset of the layer count (i16) in a file from the writer.
fn layer_count_at(bytes: &[u8]) -> usize {
    // header 26, color mode 4, resources 4, section length 4, info length 4
    let _ = bytes;
    26 + 4 + 4 + 4 + 4
}

#[test]
fn a_huge_declared_layer_count_is_rejected_without_allocating() {
    let mut bytes = write(&sample());
    let at = layer_count_at(&bytes);
    bytes[at..at + 2].copy_from_slice(&0x7fffu16.to_be_bytes());
    assert_eq!(read(&bytes).unwrap_err(), PsdError::TooLarge("layer count"));
    // Under a raised limit the lie is found by the length checks instead.
    let limits = Limits {
        max_layers: u32::MAX,
        ..Limits::default()
    };
    assert!(Psd::read(&mut Cursor::new(&bytes), &limits).is_err());
}

#[test]
fn a_huge_declared_layer_size_is_rejected() {
    let mut bytes = write(&sample());
    // First record: top, left, bottom, right. Make the first layer enormous.
    let at = layer_count_at(&bytes) + 2 + 8;
    bytes[at..at + 4].copy_from_slice(&i32::MAX.to_be_bytes());
    assert!(read(&bytes).is_err());
}

#[test]
fn inconsistent_rle_rows_are_errors() {
    let mut spec = PsdSpec::new(
        4,
        2,
        vec![Node::Layer(LayerSpec::solid("a", 0, 0, 4, 2, px(5)))],
    );
    spec.rle = true;
    let bytes = write(&spec);
    let psd = read(&bytes).unwrap();
    let table = psd.entries[0].channels[0].offset as usize + 2;
    let mut bad = bytes.clone();
    // Row 0 now claims a length that does not fill the row.
    bad[table..table + 2].copy_from_slice(&1u16.to_be_bytes());
    let psd = read(&bad).unwrap();
    assert!(
        psd.decode_layer(&mut Cursor::new(&bad), 0, &Limits::default())
            .is_err()
    );
    // A row longer than the channel.
    let mut bad = bytes;
    bad[table..table + 2].copy_from_slice(&u16::MAX.to_be_bytes());
    assert!(
        psd.decode_layer(&mut Cursor::new(&bad), 0, &Limits::default())
            .is_err()
    );
}

fn decode_everything(bytes: &[u8]) {
    let limits = Limits {
        max_canvas: 64,
        max_layers: 64,
        max_layer_bytes: 1 << 16,
        max_total_bytes: 1 << 20,
    };
    if let Ok(psd) = Psd::read(&mut Cursor::new(bytes), &limits) {
        for i in 0..psd.entries.len() {
            let _ = psd.decode_layer(&mut Cursor::new(bytes), i, &limits);
        }
    }
}

#[test]
fn mutated_and_truncated_input_never_panics() {
    for rle in [false, true] {
        let mut spec = sample();
        spec.rle = rle;
        let bytes = write(&spec);
        for n in 0..bytes.len() {
            let r = catch_unwind(AssertUnwindSafe(|| decode_everything(&bytes[..n])));
            assert!(r.is_ok(), "truncated at {n}");
        }
        for i in 0..bytes.len() {
            for v in [0x00u8, 0xff, 0x80, 0x01] {
                let mut m = bytes.clone();
                m[i] = v;
                let r = catch_unwind(AssertUnwindSafe(|| decode_everything(&m)));
                assert!(r.is_ok(), "byte {i} set to {v:#x}");
            }
        }
    }
}
