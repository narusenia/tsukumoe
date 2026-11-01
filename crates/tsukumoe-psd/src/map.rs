// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `sources/psd-map.json`: what the last import took from the PSD and which
//! model elements it made, so a reimport can find them again.

use serde::{Deserialize, Serialize};

pub const MAP_PATH: &str = "sources/psd-map.json";
pub const MAP_VERSION: u32 = 1;
/// The model units one canvas pixel takes (1000 pixels are 1 unit).
pub const UNITS_PER_PIXEL: f32 = 0.001;
/// The largest map file read.
pub const MAX_MAP_BYTES: usize = 64 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Where the PSD was when it was imported. The PSD is not copied.
    pub path: String,
    pub sha256: String,
    pub canvas: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRec {
    /// Group path with `/`.
    pub key: String,
    /// Name of the `Part` made for the group.
    pub part: String,
    pub opacity: u8,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerRec {
    /// Group path and layer name with `/`; `#n` after it for the n-th of
    /// several layers with the same path and name (the top one has no suffix).
    pub key: String,
    pub mesh: String,
    /// Name of the `Part` the mesh was put in.
    pub part: String,
    /// Path of the texture file in the project.
    pub texture: String,
    /// The trimmed image's rectangle on the canvas: left, top, right, bottom.
    pub rect: [i32; 4],
    /// Hash of the trimmed image.
    pub image_hash: String,
    /// The PSD's values at the last import.
    pub opacity: u8,
    pub blend: String,
    pub visible: bool,
    /// Stack position from the bottom; the mesh's draw order.
    pub z: u32,
    /// Key of the layer this one is clipped to, when that became a mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PsdMap {
    pub version: u32,
    pub source: Source,
    pub units_per_pixel: f32,
    /// The `Part` every top-level group and layer is put under.
    pub root_part: String,
    pub groups: Vec<GroupRec>,
    pub layers: Vec<LayerRec>,
}

impl PsdMap {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_MAP_BYTES {
            return Err("psd-map.json is too large".into());
        }
        let map: Self = serde_json::from_slice(bytes).map_err(|e| format!("psd-map.json: {e}"))?;
        if map.version != MAP_VERSION {
            return Err(format!("psd-map.json: unsupported version {}", map.version));
        }
        if !map.units_per_pixel.is_finite() || map.units_per_pixel <= 0.0 {
            return Err("psd-map.json: bad units_per_pixel".into());
        }
        Ok(map)
    }

    pub fn to_json(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec_pretty(self).expect("map serializes");
        out.push(b'\n');
        out
    }
}
