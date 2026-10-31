// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The playback side of the module: no graphics, so it also runs in Node.

use tsukumoe_core::rig::BlendMode;
use tsukumoe_core::tmod::{AlignedBuf, Tmod};
use tsukumoe_runtime::{AutoConfig, BlinkConfig, BreathConfig, Priority, Runtime};
use wasm_bindgen::prelude::*;

use crate::err;

/// Numbers per mesh in the mesh table: texture, blend mode (0 normal, 1
/// additive, 2 multiply), double sided, vertex start, vertex count, index
/// start, index count, mask start, mask count, mask inverted, opacity, draw
/// order, multiply r g b, screen r g b, geometry changed, style changed.
const MESH_STRIDE: usize = 20;

/// A loaded `.tmod` with its playback state.
#[wasm_bindgen]
pub struct Model {
    pub(crate) runtime: Runtime<'static>,
    auto: AutoConfig,
    /// Embedded PNG files.
    pngs: Vec<Vec<u8>>,
    /// `MESH_STRIDE` numbers per mesh, refreshed by `update`.
    mesh_table: Vec<f32>,
    /// Mask mesh indices of all meshes, back to back.
    masks: Vec<u32>,
    /// First mask of each mesh in `masks`.
    mask_starts: Vec<f32>,
}

#[wasm_bindgen]
impl Model {
    /// Loads a `.tmod` and evaluates the first frame.
    pub fn load(tmod: &[u8]) -> Result<Model, JsValue> {
        let buf = AlignedBuf::from_bytes(tmod);
        let parsed = Tmod::parse(&buf).map_err(err)?;
        let pngs: Vec<Vec<u8>> = (0..parsed.texture_count())
            .map(|i| parsed.texture(i).map(<[u8]>::to_vec))
            .collect::<Option<_>>()
            .ok_or_else(|| err("texture outside the data"))?;
        let mut model = Model {
            runtime: Runtime::from_bytes(tmod).map_err(err)?,
            auto: AutoConfig::default(),
            pngs,
            mesh_table: Vec::new(),
            masks: Vec::new(),
            mask_starts: Vec::new(),
        };
        model.runtime.update(0.0);
        {
            let list = model.runtime.draw_list();
            for i in 0..list.len() {
                model.mask_starts.push(model.masks.len() as f32);
                model.masks.extend_from_slice(list.drawable(i).masks);
            }
        }
        model.fill_mesh_table();
        Ok(model)
    }

    pub fn parameter_names(&self) -> Vec<String> {
        let m = self.runtime.model();
        (0..m.parameters.len())
            .map(|i| m.parameter_name(i).to_owned())
            .collect()
    }

    /// `[min, max, default]` of every parameter, back to back.
    pub fn parameter_ranges(&self) -> Vec<f32> {
        let m = self.runtime.model();
        m.parameters
            .iter()
            .flat_map(|p| [p.min, p.max, p.default])
            .collect()
    }

    /// The index of parameter `name`, `undefined` when there is none.
    pub fn parameter_index(&self, name: &str) -> Option<u32> {
        self.runtime.parameter_index(name).map(|i| i as u32)
    }

    /// Holds parameter `index` at `value` until `clear_parameter`. False when
    /// the index is out of range.
    pub fn set_parameter(&mut self, index: u32, value: f32) -> bool {
        self.runtime.set_parameter(index as usize, value)
    }

    pub fn clear_parameter(&mut self, index: u32) -> bool {
        self.runtime.clear_parameter(index as usize)
    }

    /// The parameter values of the last update, before drivers and clamping.
    pub fn parameter_values(&self) -> Vec<f32> {
        self.runtime.parameter_values().to_vec()
    }

    pub fn motion_names(&self) -> Vec<String> {
        let p = self.runtime.playback();
        (0..p.motions.len())
            .map(|i| p.motion_name(i).to_owned())
            .collect()
    }

    pub fn expression_names(&self) -> Vec<String> {
        let p = self.runtime.playback();
        (0..p.expressions.len())
            .map(|i| p.expression_name(i).to_owned())
            .collect()
    }

    /// `priority`: 0 idle, 1 normal, 2 force. False when a motion of a higher
    /// priority keeps playing.
    pub fn play_motion(&mut self, name: &str, layer: u32, priority: u32) -> Result<bool, JsValue> {
        let priority = match priority {
            0 => Priority::Idle,
            1 => Priority::Normal,
            2 => Priority::Force,
            p => return Err(err(format!("priority {p} is not 0, 1 or 2"))),
        };
        self.runtime
            .play_motion(name, layer as usize, priority)
            .map_err(err)
    }

    pub fn stop_motion(&mut self, layer: u32) {
        self.runtime.stop(layer as usize);
    }

    pub fn is_motion_playing(&self, layer: u32) -> bool {
        self.runtime.is_playing(layer as usize)
    }

    pub fn play_expression(&mut self, name: &str) -> Result<(), JsValue> {
        self.runtime.play_expression(name).map_err(err)
    }

    pub fn stop_expression(&mut self, name: &str) -> Result<(), JsValue> {
        self.runtime.stop_expression(name).map_err(err)
    }

    /// Turns blinking on with the default timing (see `BlinkConfig::new`),
    /// except for `interval` seconds between blinks. The other automatic
    /// motion is kept.
    pub fn set_blink(
        &mut self,
        left_eye: &str,
        right_eye: &str,
        interval: f32,
    ) -> Result<(), JsValue> {
        let mut blink = BlinkConfig::new(left_eye, right_eye);
        blink.interval = interval;
        self.apply_auto(AutoConfig {
            blink: Some(blink),
            breath: self.auto.breath.clone(),
        })
    }

    pub fn set_breath(
        &mut self,
        parameter: &str,
        cycle: f32,
        amplitude: f32,
        offset: f32,
    ) -> Result<(), JsValue> {
        self.apply_auto(AutoConfig {
            blink: self.auto.blink.clone(),
            breath: Some(BreathConfig {
                parameter: parameter.to_owned(),
                cycle,
                amplitude,
                offset,
            }),
        })
    }

    /// Turns all automatic motion off.
    pub fn clear_auto(&mut self) -> Result<(), JsValue> {
        self.apply_auto(AutoConfig::default())
    }

    /// Advances by `dt` seconds and evaluates the frame.
    pub fn update(&mut self, dt: f32) {
        self.runtime.update(dt);
        self.fill_mesh_table();
    }

    pub fn texture_count(&self) -> u32 {
        self.pngs.len() as u32
    }

    /// A copy of the PNG file of texture `index`.
    pub fn texture_png(&self, index: u32) -> Option<Vec<u8>> {
        self.pngs.get(index as usize).cloned()
    }

    pub fn mesh_names(&self) -> Vec<String> {
        let m = self.runtime.model();
        (0..m.meshes.len())
            .map(|i| m.mesh_name(i).to_owned())
            .collect()
    }

    pub fn mesh_count(&self) -> u32 {
        self.runtime.draw_list().len() as u32
    }

    /// Number of vertices of all meshes.
    pub fn vertex_count(&self) -> u32 {
        (self.runtime.draw_list().positions().len() / 2) as u32
    }

    pub fn index_count(&self) -> u32 {
        self.runtime.draw_list().indices().len() as u32
    }

    /// Whether the drawing order differs from the previous update.
    pub fn order_changed(&self) -> bool {
        self.runtime.draw_list().order_changed()
    }

    /// Byte offset in the module's memory of the mesh indices back to front
    /// (`u32`, `mesh_count` of them). Valid until the next call that may grow
    /// the memory; the package re-reads `memory.buffer` for every view.
    pub fn order_ptr(&self) -> usize {
        self.runtime.draw_list().order().as_ptr() as usize
    }

    /// `f32` x, y per vertex, all meshes in mesh order; `2 * vertex_count`.
    pub fn positions_ptr(&self) -> usize {
        self.runtime.draw_list().positions().as_ptr() as usize
    }

    /// `f32` u, v per vertex; `2 * vertex_count`.
    pub fn uvs_ptr(&self) -> usize {
        self.runtime.draw_list().uvs().as_ptr() as usize
    }

    /// `u32` triangle list, local to each mesh; `index_count`.
    pub fn indices_ptr(&self) -> usize {
        self.runtime.draw_list().indices().as_ptr() as usize
    }

    /// `f32`, `MESH_STRIDE` numbers per mesh (see `mesh_stride`).
    pub fn mesh_table_ptr(&self) -> usize {
        self.mesh_table.as_ptr() as usize
    }

    pub fn mesh_stride(&self) -> u32 {
        MESH_STRIDE as u32
    }

    /// `u32` mask mesh indices of all meshes; a mesh's are `mask_count` from
    /// `mask_start` of its table row.
    pub fn masks_ptr(&self) -> usize {
        self.masks.as_ptr() as usize
    }

    pub fn mask_total(&self) -> u32 {
        self.masks.len() as u32
    }

    /// The bounding box `[x0, y0, x1, y1]` of the current draw list.
    pub fn bounds(&self) -> Vec<f32> {
        self.bounds_array().to_vec()
    }

    pub(crate) fn bounds_array(&self) -> [f32; 4] {
        let list = self.runtime.draw_list();
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in list.positions().as_chunks::<2>().0 {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
        b
    }
}

impl Model {
    fn apply_auto(&mut self, config: AutoConfig) -> Result<(), JsValue> {
        self.runtime.set_auto(&config).map_err(err)?;
        self.auto = config;
        Ok(())
    }

    fn fill_mesh_table(&mut self) {
        let list = self.runtime.draw_list();
        self.mesh_table.clear();
        for (i, start) in self.mask_starts.iter().enumerate() {
            let d = list.drawable(i);
            self.mesh_table.extend_from_slice(&[
                d.texture as f32,
                match d.blend_mode {
                    BlendMode::Normal => 0.0,
                    BlendMode::Additive => 1.0,
                    BlendMode::Multiply => 2.0,
                },
                f32::from(u8::from(d.double_sided)),
                d.vertex_start as f32,
                (d.positions.len() / 2) as f32,
                d.index_start as f32,
                d.indices.len() as f32,
                *start,
                d.masks.len() as f32,
                f32::from(u8::from(d.mask_inverted)),
                d.opacity,
                d.draw_order,
                d.multiply_color[0],
                d.multiply_color[1],
                d.multiply_color[2],
                d.screen_color[0],
                d.screen_color[1],
                d.screen_color[2],
                f32::from(u8::from(d.geometry_changed)),
                f32::from(u8::from(d.style_changed)),
            ]);
        }
    }
}
