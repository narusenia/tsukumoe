// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Reads every layer once, one at a time, and keeps only what planning
//! needs: the trimmed rectangle and a hash of the pixels.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use serde::Serialize;
use tsukumoe_core::rig::BlendMode;

use crate::image::Image;
use crate::reader::{EntryKind, Limits, Psd, PsdError, WarningKind};

/// Something the import cannot carry over exactly. Never silent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Note {
    pub code: &'static str,
    /// The key of the layer or group the note is about.
    pub layer: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScannedGroup {
    pub key: String,
    pub name: String,
    /// Index into [`Scan::groups`].
    pub parent: Option<usize>,
    pub opacity: u8,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScannedLayer {
    pub key: String,
    pub name: String,
    /// Index into [`Scan::groups`].
    pub group: Option<usize>,
    /// Stack position from the bottom.
    pub z: u32,
    /// Trimmed rectangle on the canvas (left, top, right, bottom); `None`
    /// when the layer is fully transparent or has no pixels.
    pub rect: Option<[i32; 4]>,
    pub image_hash: String,
    pub opacity: u8,
    /// The PSD blend key.
    pub blend: String,
    pub visible: bool,
    /// Index into [`Scan::layers`] of the layer this one is clipped to.
    pub clip_base: Option<usize>,
    pub clipping: bool,
    /// Index of the entry in [`Psd::entries`], to decode it again.
    pub entry: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scan {
    pub canvas: [u32; 2],
    /// In the order of the layer panel (top first; a parent before its children).
    pub groups: Vec<ScannedGroup>,
    /// Top first.
    pub layers: Vec<ScannedLayer>,
    pub notes: Vec<Note>,
}

/// The model blend mode of a PSD blend key; `None` when it has no counterpart.
pub fn blend_mode(key: &str) -> Option<BlendMode> {
    match key {
        "norm" | "pass" => Some(BlendMode::Normal),
        "mul " => Some(BlendMode::Multiply),
        "lddg" => Some(BlendMode::Additive),
        _ => None,
    }
}

fn numbered(counts: &mut BTreeMap<String, u32>, base: String) -> String {
    let n = counts.entry(base.clone()).or_insert(0);
    let key = if *n == 0 { base } else { format!("{base}#{n}") };
    *n += 1;
    key
}

impl Scan {
    /// Decodes each layer in turn (and drops it) to find its trimmed
    /// rectangle and hash. `progress` is called with (done, total).
    pub fn new<R: Read + Seek>(
        psd: &Psd,
        r: &mut R,
        limits: &Limits,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<Self, PsdError> {
        let mut scan = Scan {
            canvas: [psd.width, psd.height],
            groups: Vec::new(),
            layers: Vec::new(),
            notes: Vec::new(),
        };
        let mut group_of_entry: BTreeMap<usize, usize> = BTreeMap::new();
        let mut layer_of_entry: BTreeMap<usize, usize> = BTreeMap::new();
        let (mut group_counts, mut layer_counts) = (BTreeMap::new(), BTreeMap::new());
        let total = psd.entries.len();
        // One buffer for every layer: only one layer's pixels are alive.
        let mut image = Image::empty();
        let key_of = |scan: &Scan, parent: Option<usize>, name: &str| match parent {
            Some(g) => format!("{}/{name}", scan.groups[g].key),
            None => name.to_owned(),
        };
        for (i, e) in psd.entries.iter().enumerate() {
            progress(i, total);
            let parent = e.parent.map(|p| group_of_entry[&p]);
            match e.kind {
                EntryKind::Group => {
                    let base = key_of(&scan, parent, &e.name);
                    let key = numbered(&mut group_counts, base);
                    group_of_entry.insert(i, scan.groups.len());
                    scan.groups.push(ScannedGroup {
                        key,
                        name: e.name.clone(),
                        parent,
                        opacity: e.opacity,
                        visible: e.visible,
                    });
                }
                EntryKind::Layer => {
                    let base = key_of(&scan, parent, &e.name);
                    let key = numbered(&mut layer_counts, base);
                    psd.decode_layer_into(r, i, limits, &mut image)?;
                    let bounds = image.opaque_bounds();
                    let (rect, image_hash) = match bounds {
                        Some(b) => {
                            let hash = image.hash_rect(b);
                            (
                                Some([
                                    e.rect.left + b.left,
                                    e.rect.top + b.top,
                                    e.rect.left + b.right,
                                    e.rect.top + b.bottom,
                                ]),
                                hash,
                            )
                        }
                        None => (None, String::new()),
                    };
                    layer_of_entry.insert(i, scan.layers.len());
                    scan.layers.push(ScannedLayer {
                        key,
                        name: e.name.clone(),
                        group: parent,
                        z: e.z,
                        rect,
                        image_hash,
                        opacity: e.opacity,
                        blend: e.blend_key_str(),
                        visible: e.visible,
                        clip_base: None,
                        clipping: e.clipping,
                        entry: i,
                    });
                }
            }
        }
        progress(total, total);

        // The base of a clipped layer is the next layer below it in the same
        // group that is not itself clipped.
        let within = |mut j: usize, p: Option<usize>| loop {
            let parent = psd.entries[j].parent;
            if parent == p {
                return true;
            }
            match parent {
                Some(q) => j = q,
                None => return false,
            }
        };
        for (li, layer) in scan.layers.clone().iter().enumerate() {
            if !layer.clipping {
                continue;
            }
            let p = psd.entries[layer.entry].parent;
            let mut base = None;
            for j in layer.entry + 1..psd.entries.len() {
                if !within(j, p) {
                    break;
                }
                if psd.entries[j].parent == p && !psd.entries[j].clipping {
                    base = Some(j);
                    break;
                }
            }
            scan.layers[li].clip_base = base.and_then(|j| layer_of_entry.get(&j).copied());
        }

        scan.reader_notes(psd);
        scan.notes.sort();
        Ok(scan)
    }

    fn reader_notes(&mut self, psd: &Psd) {
        let key_of_entry = |scan: &Scan, entry: usize| {
            scan.layers
                .iter()
                .find(|l| l.entry == entry)
                .map(|l| l.key.clone())
                .unwrap_or_else(|| psd.entries[entry].name.clone())
        };
        for w in &psd.warnings {
            let (code, message) = match &w.kind {
                WarningKind::LayerMask => ("layer_mask", "layer mask ignored".to_owned()),
                WarningKind::AdjustmentLayer(k) => (
                    "adjustment_layer",
                    format!("adjustment layer '{k}' ignored"),
                ),
                WarningKind::LayerEffects => ("layer_effects", "layer effects ignored".to_owned()),
            };
            let layer = key_of_entry(self, w.entry);
            self.notes.push(Note {
                code,
                layer,
                message,
            });
        }
        let mut notes = Vec::new();
        for l in &self.layers {
            if l.rect.is_none() {
                notes.push(Note {
                    code: "empty_layer",
                    layer: l.key.clone(),
                    message: "fully transparent: no mesh is made".into(),
                });
            }
            if blend_mode(&l.blend).is_none() {
                notes.push(Note {
                    code: "blend_mode",
                    layer: l.key.clone(),
                    message: if l.clipping && self.clipping_problem(l).is_none() {
                        format!(
                            "blend mode '{}' has no counterpart: Normal (clipped to its base, but the color is approximate)",
                            l.blend
                        )
                    } else {
                        format!("blend mode '{}' has no counterpart: Normal", l.blend)
                    },
                });
            }
            if l.clipping
                && let Some(reason) = self.clipping_problem(l)
            {
                notes.push(Note {
                    code: "clipping",
                    layer: l.key.clone(),
                    message: format!("clipping ignored: {reason}"),
                });
            }
        }
        self.notes.extend(notes);
    }

    /// Why clipping `layer` to its base would not look like the PSD, if so.
    /// The runtime mask is the alpha of the base's texture, ignoring the
    /// base's opacity and blend mode (`rendering.md` section 3). The mask
    /// factor multiplies the layer's premultiplied color before the blend, so
    /// Normal, Multiply and Additive layers clip like the PSD's.
    pub fn clipping_problem(&self, layer: &ScannedLayer) -> Option<&'static str> {
        let Some(base) = layer.clip_base.map(|b| &self.layers[b]) else {
            return Some("the base is not a layer");
        };
        if base.rect.is_none() || layer.rect.is_none() {
            return Some("the base or the layer has no pixels");
        }
        if !base.visible {
            return Some("the base is hidden");
        }
        if base.opacity != 255 {
            return Some("the base is not fully opaque");
        }
        if blend_mode(&base.blend) != Some(BlendMode::Normal) {
            return Some("the base has a blend mode");
        }
        None
    }
}
