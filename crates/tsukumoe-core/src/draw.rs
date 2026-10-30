// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The draw list: the renderer-independent output of a frame. It is a view
//! onto the compiled model and the evaluated frame, so reading it copies
//! nothing. The rules are in `docs/specifications/draw-list.md`.

use crate::compiled::CompiledModel;
use crate::eval::Frame;
use crate::rig::BlendMode;

pub struct DrawList<'a> {
    model: &'a CompiledModel<'a>,
    frame: &'a Frame,
}

/// One mesh to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawable<'a> {
    pub mesh: usize,
    pub name: &'a str,
    pub texture: u32,
    pub blend_mode: BlendMode,
    pub double_sided: bool,
    /// Vertex positions in drawing space, `x, y` interleaved.
    pub positions: &'a [f32],
    /// `u, v` interleaved.
    pub uvs: &'a [f32],
    /// Triangle list, local to this mesh's vertices.
    pub indices: &'a [u32],
    /// Index of this mesh's first vertex in [`DrawList::positions`] and
    /// [`DrawList::uvs`] (a float offset is twice this).
    pub vertex_start: usize,
    /// Index of this mesh's first index in [`DrawList::indices`].
    pub index_start: usize,
    /// Mesh indices of the meshes clipping this one.
    pub masks: &'a [u32],
    pub mask_inverted: bool,
    /// Final opacity, with the opacity of the parents multiplied in.
    pub opacity: f32,
    pub draw_order: f32,
    pub multiply_color: [f32; 3],
    pub screen_color: [f32; 3],
    /// Positions differ from the previous frame.
    pub geometry_changed: bool,
    /// Opacity, draw order or a color differs from the previous frame.
    pub style_changed: bool,
}

impl Frame {
    /// The draw list of the last [`Frame::evaluate`].
    pub fn draw_list<'a>(&'a self, model: &'a CompiledModel<'a>) -> DrawList<'a> {
        DrawList { model, frame: self }
    }
}

impl<'a> DrawList<'a> {
    pub fn len(&self) -> usize {
        self.model.meshes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.model.meshes.is_empty()
    }

    /// Mesh indices, back to front.
    pub fn order(&self) -> &'a [u32] {
        &self.frame.order
    }

    /// The drawing order differs from the previous frame.
    pub fn order_changed(&self) -> bool {
        self.frame.order_changed
    }

    /// All meshes' positions, `x, y` interleaved, in mesh index order.
    pub fn positions(&self) -> &'a [f32] {
        self.frame.positions()
    }

    pub fn uvs(&self) -> &'a [f32] {
        &self.model.mesh_uvs
    }

    pub fn indices(&self) -> &'a [u32] {
        &self.model.mesh_indices
    }

    pub fn drawable(&self, mesh: usize) -> Drawable<'a> {
        let (model, frame) = (self.model, self.frame);
        let def = &model.meshes[mesh];
        let (start, count) = (def.vertex_start as usize, def.vertex_count as usize);
        let vertices = start * 2..(start + count) * 2;
        let a = frame.mesh_attrs(mesh);
        Drawable {
            mesh,
            name: model.mesh_name(mesh),
            texture: def.texture,
            blend_mode: def.blend_mode(),
            double_sided: def.double_sided != 0,
            positions: &frame.positions()[vertices.clone()],
            uvs: &model.mesh_uvs[vertices],
            indices: &model.mesh_indices
                [def.index_start as usize..(def.index_start + def.index_count) as usize],
            vertex_start: start,
            index_start: def.index_start as usize,
            masks: &model.mesh_masks
                [def.mask_start as usize..(def.mask_start + def.mask_count) as usize],
            mask_inverted: def.mask_inverted != 0,
            opacity: a[0],
            draw_order: a[1],
            multiply_color: [a[2], a[3], a[4]],
            screen_color: [a[5], a[6], a[7]],
            geometry_changed: frame.geometry_changed[mesh],
            style_changed: frame.style_changed[mesh],
        }
    }

    /// The drawables back to front.
    pub fn drawables(&self) -> impl Iterator<Item = Drawable<'a>> + use<'a, '_> {
        self.order().iter().map(|&m| self.drawable(m as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Parameter, Part, Point};
    use crate::keyform::{Axis, Binding, KeyformGrid};
    use crate::model::Model;
    use crate::rig::{ArtMesh, MeshKeyform};

    const TRI: [Point; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];

    fn keyform(x: f32, opacity: f32, draw_order: f32) -> MeshKeyform {
        MeshKeyform {
            positions: TRI.iter().map(|p| [p[0] + x, p[1]]).collect(),
            opacity,
            draw_order,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
        }
    }

    /// Meshes "A", "B", "C" (draw order 500 each). "A" is driven by `P` from
    /// `keyforms[0]` to `keyforms[1]`; the others are static.
    fn model(keyforms: [MeshKeyform; 2]) -> Model {
        let mut m = Model::new();
        let part = m.parts.insert("Part", Part::default()).unwrap();
        let p = m
            .parameters
            .insert("P", Parameter::new(0.0, 1.0, 0.0))
            .unwrap();
        let b = m.bindings.insert(
            Binding::new(vec![Axis {
                parameter: p,
                keys: vec![0.0, 1.0],
            }])
            .unwrap(),
        );
        let mut a = ArtMesh::new(part, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]);
        a.keyforms = Some(KeyformGrid::new(b, keyforms.to_vec()));
        let a = m.meshes.insert("A", a).unwrap();
        let mut b = ArtMesh::new(part, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]);
        b.masks = vec![a];
        b.mask_inverted = true;
        b.texture = 3;
        b.blend_mode = BlendMode::Additive;
        b.double_sided = true;
        m.meshes.insert("B", b).unwrap();
        m.meshes
            .insert(
                "C",
                ArtMesh::new(part, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]),
            )
            .unwrap();
        m
    }

    fn flat() -> [MeshKeyform; 2] {
        [keyform(0.0, 1.0, 500.0), keyform(0.0, 1.0, 500.0)]
    }

    fn evaluate(c: &CompiledModel, f: &mut Frame, p: f32) {
        f.set_parameter(0, p);
        f.evaluate(c);
    }

    #[test]
    fn static_data_is_passed_through() {
        let c = model(flat()).compile().unwrap();
        let mut f = Frame::new(&c);
        evaluate(&c, &mut f, 0.0);
        let list = f.draw_list(&c);
        assert_eq!(list.len(), 3);
        assert_eq!(list.indices(), [0, 1, 2, 0, 1, 2, 0, 1, 2]);
        assert_eq!(list.uvs().len(), 18);
        assert_eq!(list.positions().len(), 18);

        let b = list.drawable(1);
        assert_eq!(b.name, "B");
        assert_eq!(b.texture, 3);
        assert_eq!(b.blend_mode, BlendMode::Additive);
        assert!(b.double_sided && b.mask_inverted);
        assert_eq!(b.masks, [0]);
        assert_eq!((b.vertex_start, b.index_start), (3, 3));
        assert_eq!(b.indices, [0, 1, 2]);
        assert_eq!(b.uvs, [0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        assert_eq!((b.opacity, b.draw_order), (1.0, 500.0));
        assert!(list.drawable(0).masks.is_empty());
    }

    #[test]
    fn drawing_order_is_by_draw_order_then_mesh_index() {
        let c = model([keyform(0.0, 1.0, 100.0), keyform(0.0, 1.0, 900.0)])
            .compile()
            .unwrap();
        let mut f = Frame::new(&c);
        evaluate(&c, &mut f, 0.0);
        assert_eq!(f.draw_list(&c).order(), [0, 1, 2]);
        evaluate(&c, &mut f, 1.0);
        assert_eq!(f.draw_list(&c).order(), [1, 2, 0]);
        let names: Vec<_> = f.draw_list(&c).drawables().map(|d| d.name).collect();
        assert_eq!(names, ["B", "C", "A"]);
        // Mid-way A is at 500, tied with the others: mesh index decides.
        evaluate(&c, &mut f, 0.5);
        assert_eq!(f.draw_list(&c).order(), [0, 1, 2]);
    }

    #[test]
    fn change_flags_follow_the_previous_frame() {
        let c = model([keyform(0.0, 1.0, 100.0), keyform(2.0, 0.5, 900.0)])
            .compile()
            .unwrap();
        let mut f = Frame::new(&c);
        evaluate(&c, &mut f, 0.0);
        let list = f.draw_list(&c);
        assert!(list.order_changed());
        assert!(
            list.drawables()
                .all(|d| d.geometry_changed && d.style_changed)
        );

        evaluate(&c, &mut f, 0.0);
        let list = f.draw_list(&c);
        assert!(!list.order_changed());
        assert!(
            list.drawables()
                .all(|d| !d.geometry_changed && !d.style_changed)
        );

        evaluate(&c, &mut f, 0.25);
        let list = f.draw_list(&c);
        let flags: Vec<_> = (0..3)
            .map(|i| {
                (
                    list.drawable(i).geometry_changed,
                    list.drawable(i).style_changed,
                )
            })
            .collect();
        assert_eq!(flags, [(true, true), (false, false), (false, false)]);
        assert!(!list.order_changed());

        evaluate(&c, &mut f, 1.0);
        assert!(f.draw_list(&c).order_changed());
    }

    #[test]
    fn style_changes_without_geometry_changes() {
        let c = model([keyform(0.0, 1.0, 500.0), keyform(0.0, 0.0, 500.0)])
            .compile()
            .unwrap();
        let mut f = Frame::new(&c);
        evaluate(&c, &mut f, 0.0);
        evaluate(&c, &mut f, 1.0);
        let a = f.draw_list(&c).drawable(0);
        assert!(!a.geometry_changed && a.style_changed);
        assert_eq!(a.opacity, 0.0);
    }
}
