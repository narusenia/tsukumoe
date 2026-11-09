// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Texture atlases at export: images packed into pages, UVs moved, and
//! the project's own files left alone.

use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_core::tmod::{AlignedBuf, Tmod};
use tsukumoe_format::{AtlasOptions, Manifest, Project, runtime_bytes_with, write_model};

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let rgba: Vec<u8> = (0..width * height).flat_map(|_| color).collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .unwrap();
    out
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    (info.width, info.height, buf[..info.buffer_size()].to_vec())
}

/// Colours of the textures; texture `i` is a flat square of its own colour.
const COLORS: [[u8; 4]; 5] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 0, 255],
    [0, 255, 255, 255],
];

/// One triangle-pair mesh per texture, its UVs filling the texture, except
/// that mesh 3 stretches its UVs past 1 and mesh `unused` is not made.
fn project(sizes: &[(u32, u32)], tile: Option<usize>) -> Project {
    let mut m = Model::new();
    m.apply(Command::AddPart {
        name: "P".into(),
        parent: None,
    })
    .unwrap();
    let mut project = Project::new(Manifest::new(None));
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let reach = if tile == Some(i) { 2.0 } else { 1.0 };
        m.apply(Command::AddMesh {
            name: format!("M{i}"),
            part: "P".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            uvs: vec![[0.0, 0.0], [reach, 0.0], [0.0, reach], [reach, reach]],
            indices: vec![0, 1, 2, 2, 1, 3],
        })
        .unwrap();
        m.apply(Command::SetMeshTexture {
            name: format!("M{i}"),
            texture: i as u32,
        })
        .unwrap();
        project.files.insert(
            format!("textures/t{i:03}.png"),
            png(w, h, COLORS[i % COLORS.len()]),
        );
    }
    write_model(&m, &mut project).unwrap();
    project
}

/// The colour a mesh shows at the middle of its UVs, read from the packed file.
fn colour_of(bytes: &[u8], mesh: usize) -> [u8; 4] {
    let buf = AlignedBuf::from_bytes(bytes);
    let tmod = Tmod::parse(&buf).unwrap();
    let c = &tmod.model;
    let def = c.meshes[mesh];
    let uvs = &c.mesh_uvs[def.vertex_start as usize * 2..][..def.vertex_count as usize * 2];
    let (u, v) = (
        uvs.chunks(2).map(|p| p[0]).sum::<f32>() / def.vertex_count as f32,
        uvs.chunks(2).map(|p| p[1]).sum::<f32>() / def.vertex_count as f32,
    );
    let (w, h, rgba) = decode(tmod.texture(def.texture as usize).unwrap());
    let (x, y) = (
        ((u * w as f32) as u32).min(w - 1),
        ((v * h as f32) as u32).min(h - 1),
    );
    let at = (y * w + x) as usize * 4;
    [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
}

fn texture_count(bytes: &[u8]) -> usize {
    let buf = AlignedBuf::from_bytes(bytes);
    Tmod::parse(&buf).unwrap().texture_count()
}

#[test]
fn many_textures_become_a_page_and_each_mesh_still_shows_its_own() {
    let sizes = [(20, 10), (30, 30), (8, 40), (25, 25), (16, 16)];
    let project = project(&sizes, None);
    let (bytes, report) = runtime_bytes_with(&project, Some(&AtlasOptions::default())).unwrap();
    assert_eq!((report.before, report.after), (5, 1));
    assert_eq!(texture_count(&bytes), 1);
    for (i, colour) in COLORS.iter().enumerate().take(sizes.len()) {
        assert_eq!(&colour_of(&bytes, i), colour, "mesh {i}");
    }
    // Same input, same bytes.
    let again = runtime_bytes_with(&project, Some(&AtlasOptions::default())).unwrap();
    assert_eq!(again.0, bytes);
}

#[test]
fn without_an_atlas_the_textures_are_embedded_as_they_are() {
    let project = project(&[(20, 10), (30, 30)], None);
    let (bytes, report) = runtime_bytes_with(&project, None).unwrap();
    assert_eq!((report.before, report.after), (2, 2));
    assert_eq!(texture_count(&bytes), 2);
    let buf = AlignedBuf::from_bytes(&bytes);
    let tmod = Tmod::parse(&buf).unwrap();
    assert_eq!(
        tmod.texture(0).unwrap(),
        &project.files["textures/t000.png"][..]
    );
}

#[test]
fn one_texture_gains_nothing_and_stays_as_it_was() {
    let project = project(&[(20, 10)], None);
    let (bytes, report) = runtime_bytes_with(&project, Some(&AtlasOptions::default())).unwrap();
    assert_eq!((report.before, report.after), (1, 1));
    let buf = AlignedBuf::from_bytes(&bytes);
    assert_eq!(
        Tmod::parse(&buf).unwrap().texture(0).unwrap(),
        &project.files["textures/t000.png"][..]
    );
}

#[test]
fn a_texture_a_mesh_tiles_stays_alone() {
    let sizes = [(20, 10), (30, 30), (8, 40)];
    let project = project(&sizes, Some(1));
    let (bytes, report) = runtime_bytes_with(&project, Some(&AtlasOptions::default())).unwrap();
    assert_eq!(
        (report.before, report.after),
        (3, 2),
        "a page and the tiled one"
    );
    assert_eq!(texture_count(&bytes), 2);
    for (i, colour) in COLORS.iter().enumerate().take(sizes.len()) {
        assert_eq!(&colour_of(&bytes, i), colour, "mesh {i}");
    }
}

#[test]
fn an_image_bigger_than_a_page_stays_alone_and_the_rest_are_packed() {
    let sizes = [(100, 100), (10, 10), (12, 12), (9, 9)];
    let project = project(&sizes, None);
    let options = AtlasOptions {
        max_size: 64,
        padding: 1,
    };
    let (bytes, report) = runtime_bytes_with(&project, Some(&options)).unwrap();
    assert_eq!(report.after, 2);
    assert_eq!(report.pages.len(), 1);
    for (i, colour) in COLORS.iter().enumerate().take(sizes.len()) {
        assert_eq!(&colour_of(&bytes, i), colour, "mesh {i}");
    }
}

#[test]
fn a_texture_no_mesh_uses_is_left_out() {
    let mut project = project(&[(20, 10), (30, 30)], None);
    project
        .files
        .insert("textures/t999.png".into(), png(8, 8, [1, 2, 3, 255]));
    let (_, report) = runtime_bytes_with(&project, Some(&AtlasOptions::default())).unwrap();
    assert_eq!((report.before, report.after), (3, 1));
}

#[test]
fn a_texture_that_does_not_decode_is_an_error_naming_it() {
    let mut project = project(&[(20, 10), (30, 30)], None);
    project
        .files
        .insert("textures/t000.png".into(), b"not a png".to_vec());
    let error = runtime_bytes_with(&project, Some(&AtlasOptions::default())).unwrap_err();
    assert!(error.to_string().contains("texture 0"), "{error}");
}

#[test]
fn the_padding_around_an_image_repeats_its_edge() {
    let project = project(&[(8, 8), (8, 8)], None);
    let options = AtlasOptions {
        max_size: 64,
        padding: 2,
    };
    let (bytes, _) = runtime_bytes_with(&project, Some(&options)).unwrap();
    let buf = AlignedBuf::from_bytes(&bytes);
    let tmod = Tmod::parse(&buf).unwrap();
    let (w, _, rgba) = decode(tmod.texture(0).unwrap());
    // Every pixel the images do not cover but their padding does has a colour.
    let filled = rgba.chunks(4).filter(|p| p[3] == 255).count();
    assert!(filled > 2 * 8 * 8, "{filled}");
    assert!(w >= 8);
}
