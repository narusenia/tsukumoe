// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The project's texture pages as egui textures, for the small mesh pictures
//! in the tree rows and the hover card. The pages are in the order the
//! viewport loads them, so a mesh's `texture` index picks the same page.

use egui::{Rect, TextureHandle, TextureOptions, pos2};
use tsukumoe_core::model::Model;
use tsukumoe_gpu::Texture;

#[derive(Clone, Default)]
pub struct Atlas {
    pages: Vec<TextureHandle>,
}

impl std::fmt::Debug for Atlas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Atlas({} pages)", self.pages.len())
    }
}

impl Atlas {
    /// Pages that do not decode are left out, so later indices shift; the
    /// viewport reports that error itself.
    pub fn load(ctx: &egui::Context, files: &std::collections::BTreeMap<String, Vec<u8>>) -> Self {
        let pages = files
            .iter()
            .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
            .filter_map(|(path, bytes)| {
                let t = Texture::from_png(bytes).ok()?;
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [t.width as usize, t.height as usize],
                    &t.rgba,
                );
                Some(ctx.load_texture(path, image, TextureOptions::LINEAR))
            })
            .collect();
        Self { pages }
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// The page and the part of it (UV rectangle) a mesh's vertices cover.
    pub fn picture(&self, model: &Model, mesh: &str) -> Option<(&TextureHandle, Rect)> {
        let m = model
            .meshes()
            .find(mesh)
            .and_then(|id| model.meshes().get(id))?;
        let page = self.pages.get(m.texture as usize)?;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &m.uvs {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        (lo[0] < hi[0] && lo[1] < hi[1]).then(|| {
            (
                page,
                Rect::from_min_max(pos2(lo[0], lo[1]), pos2(hi[0], hi[1])),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

    #[test]
    fn a_mesh_picture_is_the_uv_box_of_its_page() {
        let mut m = Model::new();
        m.apply(Command::AddPart {
            name: "P".into(),
            parent: None,
        })
        .unwrap();
        m.apply(Command::AddMesh {
            name: "M".into(),
            part: "P".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            uvs: vec![[0.25, 0.5], [0.75, 0.5], [0.25, 1.0]],
            indices: vec![0, 1, 2],
        })
        .unwrap();
        let ctx = egui::Context::default();
        let png = tsukumoe_gpu::Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        }
        .to_png();
        let files = [("textures/a.png".to_owned(), png)].into();
        let atlas = Atlas::load(&ctx, &files);
        let (_, uv) = atlas.picture(&m, "M").unwrap();
        assert_eq!(uv, Rect::from_min_max(pos2(0.25, 0.5), pos2(0.75, 1.0)));
        assert!(atlas.picture(&m, "none").is_none());
        assert!(Atlas::default().picture(&m, "M").is_none());
    }
}
