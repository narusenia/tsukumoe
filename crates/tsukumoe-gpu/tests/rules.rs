// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! One test per rule of `docs/specifications/rendering.md`. Each scene is a
//! few quads over the view `(0, 0, 1, 1)` and the expected pixels come from
//! the formulas of section 4. Skipped when the machine has no adapter.

use tsukumoe_core::elements::{Part, Point};
use tsukumoe_core::eval::Frame;
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::rig::{ArtMesh, BlendMode};
use tsukumoe_gpu::{Headless, Image, Texture};

const TOLERANCE: u8 = 2;

fn gpu() -> Option<Headless> {
    match Headless::new() {
        Ok(g) => Some(g),
        Err(e) => {
            // CI sets this so that a missing adapter cannot pass silently.
            assert!(
                std::env::var_os("TSUKUMOE_REQUIRE_GPU").is_none(),
                "no adapter: {e}"
            );
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// A 1x1 texture of straight-alpha `rgba`.
fn solid(rgba: [u8; 4]) -> Texture {
    Texture {
        width: 1,
        height: 1,
        rgba: rgba.to_vec(),
    }
}

const FRONT: [u32; 6] = [0, 1, 2, 0, 2, 3];
const BACK: [u32; 6] = [0, 2, 1, 0, 3, 2];

/// A counter-clockwise quad over `[x0, x1] x [y0, y1]` sampling the centre of
/// its texture.
fn quad(m: &mut ModelParts, rect: [f32; 4], texture: u32) -> ArtMesh {
    let part = m
        .parts
        .insert(format!("part{}", m.parts.len()), Part::default())
        .unwrap();
    let [x0, y0, x1, y1] = rect;
    let positions: Vec<Point> = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut mesh = ArtMesh::new(part, positions, vec![[0.5, 0.5]; 4], FRONT.to_vec());
    mesh.texture = texture;
    mesh
}

const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn render(g: &mut Headless, textures: &[Texture], m: ModelParts, w: u32, h: u32) -> Image {
    let compiled = Model::from_parts(m).unwrap().compile().unwrap();
    let mut frame = Frame::new(&compiled);
    frame.evaluate(&compiled);
    g.set_textures(textures).unwrap();
    g.render(&frame.draw_list(&compiled), [0.0, 0.0, 1.0, 1.0], w, h)
        .unwrap()
}

fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    img.rgba[i..i + 4].try_into().unwrap()
}

#[track_caller]
fn assert_px(img: &Image, x: u32, y: u32, want: [u8; 4]) {
    let got = px(img, x, y);
    assert!(
        got.iter()
            .zip(want)
            .all(|(g, w)| g.abs_diff(w) <= TOLERANCE),
        "pixel ({x}, {y}): got {got:?}, want {want:?}"
    );
}

#[track_caller]
fn assert_all(img: &Image, want: [u8; 4]) {
    for y in 0..img.height {
        for x in 0..img.width {
            assert_px(img, x, y, want);
        }
    }
}

#[test]
fn an_empty_scene_is_transparent() {
    let Some(mut g) = gpu() else { return };
    let img = render(&mut g, &[], ModelParts::default(), 2, 2);
    assert_all(&img, [0; 4]);
}

#[test]
fn texture_colors_are_premultiplied_and_opacity_scales_all_channels() {
    let Some(mut g) = gpu() else { return };
    let mut m = ModelParts::default();
    let mut a = quad(&mut m, FULL, 0);
    a.opacity = 0.5;
    m.meshes.insert("a", a).unwrap();
    // straight (200, 100, 50, 128) -> premultiplied (100, 50, 25, 128), halved
    let img = render(&mut g, &[solid([200, 100, 50, 128])], m, 2, 2);
    assert_all(&img, [50, 25, 13, 64]);
}

#[test]
fn multiply_then_screen_color() {
    let Some(mut g) = gpu() else { return };
    let mut m = ModelParts::default();
    let mut a = quad(&mut m, FULL, 0);
    a.multiply_color = [0.5, 1.0, 0.0];
    a.screen_color = [0.0, 0.0, 0.5];
    m.meshes.insert("a", a).unwrap();
    // c = (255, 255, 255) * (.5, 1, 0) = (.5, 1, 0); screen (0, 0, .5) with alpha 1:
    // b = 0 + .5 - 0 = .5
    let img = render(&mut g, &[solid([255, 255, 255, 255])], m, 2, 2);
    assert_all(&img, [128, 255, 128, 255]);
}

fn two_layers(mode: BlendMode) -> ModelParts {
    let mut m = ModelParts::default();
    let bottom = quad(&mut m, FULL, 0);
    m.meshes.insert("bottom", bottom).unwrap();
    let mut top = quad(&mut m, FULL, 1);
    top.blend_mode = mode;
    top.draw_order = 600.0;
    m.meshes.insert("top", top).unwrap();
    m
}

#[test]
fn normal_blend_is_source_over() {
    let Some(mut g) = gpu() else { return };
    let tex = [solid([255, 0, 0, 255]), solid([0, 0, 255, 128])];
    let img = render(&mut g, &tex, two_layers(BlendMode::Normal), 2, 2);
    // top premultiplied (0, 0, 128, 128); red * (1 - 128/255)
    assert_all(&img, [127, 0, 128, 255]);
}

#[test]
fn additive_blend_adds_color_and_keeps_alpha() {
    let Some(mut g) = gpu() else { return };
    let tex = [solid([100, 0, 0, 255]), solid([0, 100, 0, 255])];
    let img = render(&mut g, &tex, two_layers(BlendMode::Additive), 2, 2);
    assert_all(&img, [100, 100, 0, 255]);
}

#[test]
fn multiply_blend_multiplies_the_backdrop() {
    let Some(mut g) = gpu() else { return };
    let tex = [solid([200, 200, 200, 255]), solid([128, 255, 0, 255])];
    let img = render(&mut g, &tex, two_layers(BlendMode::Multiply), 2, 2);
    assert_all(&img, [100, 200, 0, 255]);
}

#[test]
fn drawing_order_decides_which_is_on_top() {
    let Some(mut g) = gpu() else { return };
    let mut m = ModelParts::default();
    let mut red = quad(&mut m, FULL, 0);
    red.draw_order = 600.0;
    m.meshes.insert("red", red).unwrap();
    let green = quad(&mut m, FULL, 1);
    m.meshes.insert("green", green).unwrap();
    let tex = [solid([255, 0, 0, 255]), solid([0, 255, 0, 255])];
    assert_all(&render(&mut g, &tex, m, 2, 2), [255, 0, 0, 255]);
}

#[test]
fn back_faces_are_culled_unless_double_sided() {
    let Some(mut g) = gpu() else { return };
    for (double_sided, want) in [(false, [0; 4]), (true, [255, 0, 0, 255])] {
        let mut m = ModelParts::default();
        let mut a = quad(&mut m, FULL, 0);
        a.indices = BACK.to_vec();
        a.double_sided = double_sided;
        m.meshes.insert("a", a).unwrap();
        assert_all(&render(&mut g, &[solid([255, 0, 0, 255])], m, 2, 2), want);
    }
}

#[test]
fn drawing_space_is_y_up_and_v_zero_is_the_top_row() {
    let Some(mut g) = gpu() else { return };
    let mut m = ModelParts::default();
    // the upper half samples v = 0.1 (row 0: red), the lower half v = 0.9 (row 1: green)
    for (name, rect, v) in [
        ("up", [0.0, 0.5, 1.0, 1.0], 0.1),
        ("down", [0.0, 0.0, 1.0, 0.5], 0.9),
    ] {
        let mut a = quad(&mut m, rect, 0);
        a.uvs = vec![[0.5, v]; 4];
        m.meshes.insert(name, a).unwrap();
    }
    let tex = Texture {
        width: 1,
        height: 2,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
    };
    let img = render(&mut g, &[tex], m, 2, 2);
    assert_px(&img, 0, 0, [255, 0, 0, 255]);
    assert_px(&img, 1, 0, [255, 0, 0, 255]);
    assert_px(&img, 0, 1, [0, 255, 0, 255]);
    assert_px(&img, 1, 1, [0, 255, 0, 255]);
}

/// A white quad clipped by a mask covering the left half. The mask mesh has
/// opacity 0, so only its shape matters.
fn masked(inverted: bool) -> ModelParts {
    let mut m = ModelParts::default();
    let mut mask = quad(&mut m, [0.0, 0.0, 0.5, 1.0], 1);
    mask.opacity = 0.0;
    let mask = m.meshes.insert("mask", mask).unwrap();
    let mut a = quad(&mut m, FULL, 0);
    a.masks = vec![mask];
    a.mask_inverted = inverted;
    m.meshes.insert("a", a).unwrap();
    m
}

#[test]
fn a_mask_keeps_what_it_covers_and_inversion_keeps_the_rest() {
    let Some(mut g) = gpu() else { return };
    let tex = [solid([255, 255, 255, 255]), solid([0, 0, 0, 255])];
    let white = [255; 4];
    let img = render(&mut g, &tex, masked(false), 4, 2);
    assert_px(&img, 0, 0, white);
    assert_px(&img, 1, 1, white);
    assert_px(&img, 2, 0, [0; 4]);
    assert_px(&img, 3, 1, [0; 4]);
    let img = render(&mut g, &tex, masked(true), 4, 2);
    assert_px(&img, 0, 0, [0; 4]);
    assert_px(&img, 3, 1, white);
}

#[test]
fn the_mask_value_is_the_texture_alpha_of_the_mask_mesh() {
    let Some(mut g) = gpu() else { return };
    let tex = [solid([255, 255, 255, 255]), solid([0, 0, 0, 128])];
    let img = render(&mut g, &tex, masked(false), 4, 2);
    assert_px(&img, 0, 0, [128, 128, 128, 128]);
}

#[test]
fn several_masks_combine_by_maximum() {
    let Some(mut g) = gpu() else { return };
    let mut m = ModelParts::default();
    let mut left = quad(&mut m, [0.0, 0.0, 0.5, 1.0], 1);
    left.opacity = 0.0;
    let left = m.meshes.insert("left", left).unwrap();
    let mut right = quad(&mut m, [0.5, 0.0, 1.0, 1.0], 1);
    right.opacity = 0.0;
    let right = m.meshes.insert("right", right).unwrap();
    let mut a = quad(&mut m, FULL, 0);
    a.masks = vec![left, right];
    m.meshes.insert("a", a).unwrap();
    let tex = [solid([255, 255, 255, 255]), solid([0, 0, 0, 255])];
    assert_all(&render(&mut g, &tex, m, 4, 2), [255; 4]);
}

#[test]
fn a_missing_texture_is_an_error() {
    let Some(mut g) = gpu() else { return };
    let mut m = ModelParts::default();
    let a = quad(&mut m, FULL, 5);
    m.meshes.insert("a", a).unwrap();
    let compiled = Model::from_parts(m).unwrap().compile().unwrap();
    let mut frame = Frame::new(&compiled);
    frame.evaluate(&compiled);
    let err = g.render(&frame.draw_list(&compiled), [0.0, 0.0, 1.0, 1.0], 2, 2);
    assert!(matches!(err, Err(tsukumoe_gpu::Error::MissingTexture(5))));
}

#[test]
fn an_image_exports_as_png() {
    let img = Image {
        width: 1,
        height: 1,
        rgba: vec![50, 25, 13, 64],
    };
    let png = img.to_png();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
