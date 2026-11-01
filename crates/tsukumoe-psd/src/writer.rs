// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A minimal PSD writer for tests. It writes only what the reader reads, so
//! no third-party file is needed to test it. Not part of the public API
//! unless the `test-writer` feature is on.

/// A layer to write. `pixels` is RGBA8, `width * height * 4` bytes.
#[derive(Debug, Clone)]
pub struct LayerSpec {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub opacity: u8,
    pub blend: [u8; 4],
    pub visible: bool,
    pub clipping: bool,
    /// Write a (dummy) layer mask record.
    pub layer_mask: bool,
    /// Write an adjustment layer block with this key.
    pub adjustment: Option<[u8; 4]>,
    pub effects: bool,
    /// Also write the name as UTF-16 (`luni`); otherwise only the Pascal name.
    pub unicode_name: bool,
}

impl LayerSpec {
    /// A layer filled with one color.
    pub fn solid(name: &str, left: i32, top: i32, width: u32, height: u32, rgba: [u8; 4]) -> Self {
        Self {
            name: name.into(),
            left,
            top,
            width,
            height,
            pixels: rgba.repeat((width * height) as usize),
            opacity: 255,
            blend: *b"norm",
            visible: true,
            clipping: false,
            layer_mask: false,
            adjustment: None,
            effects: false,
            unicode_name: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupSpec {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub blend: [u8; 4],
    pub open: bool,
    /// Top first, like the layer panel.
    pub children: Vec<Node>,
}

impl GroupSpec {
    pub fn new(name: &str, children: Vec<Node>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 255,
            blend: *b"pass",
            open: true,
            children,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Node {
    Layer(LayerSpec),
    Group(GroupSpec),
}

#[derive(Debug, Clone)]
pub struct PsdSpec {
    pub width: u32,
    pub height: u32,
    /// Write 16-bit samples (each 8-bit value `v` becomes `v * 257`).
    pub depth16: bool,
    pub rle: bool,
    /// Compression code to write instead of raw / RLE (for rejection tests).
    pub compression_override: Option<u16>,
    /// Color mode to write (3 is RGB).
    pub color_mode: u16,
    /// Write version 2 (PSB).
    pub psb: bool,
    /// Keep the layers in an `Lr16` block (as Photoshop does for 16-bit files).
    pub lr16: bool,
    /// Top first.
    pub nodes: Vec<Node>,
}

impl PsdSpec {
    pub fn new(width: u32, height: u32, nodes: Vec<Node>) -> Self {
        Self {
            width,
            height,
            depth16: false,
            rle: false,
            compression_override: None,
            color_mode: 3,
            psb: false,
            lr16: false,
            nodes,
        }
    }
}

struct Record {
    name: String,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    opacity: u8,
    blend: [u8; 4],
    visible: bool,
    clipping: bool,
    layer_mask: bool,
    adjustment: Option<[u8; 4]>,
    effects: bool,
    unicode_name: bool,
    divider: u32,
}

fn flatten(nodes: &[Node], out: &mut Vec<Record>) {
    // `out` is bottom first, so walk the top-first list backwards; a group is
    // its end marker, its children, then its start record.
    for node in nodes.iter().rev() {
        match node {
            Node::Layer(l) => out.push(Record {
                name: l.name.clone(),
                left: l.left,
                top: l.top,
                width: l.width,
                height: l.height,
                pixels: l.pixels.clone(),
                opacity: l.opacity,
                blend: l.blend,
                visible: l.visible,
                clipping: l.clipping,
                layer_mask: l.layer_mask,
                adjustment: l.adjustment,
                effects: l.effects,
                unicode_name: l.unicode_name,
                divider: 0,
            }),
            Node::Group(g) => {
                let marker = |name: &str, divider| Record {
                    name: name.into(),
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 0,
                    pixels: Vec::new(),
                    opacity: g.opacity,
                    blend: g.blend,
                    visible: g.visible,
                    clipping: false,
                    layer_mask: false,
                    adjustment: None,
                    effects: false,
                    unicode_name: true,
                    divider,
                };
                out.push(marker("</Layer group>", 3));
                flatten(&g.children, out);
                out.push(marker(&g.name, if g.open { 1 } else { 2 }));
            }
        }
    }
}

fn be16(v: &mut Vec<u8>, n: u16) {
    v.extend_from_slice(&n.to_be_bytes());
}
fn be32(v: &mut Vec<u8>, n: u32) {
    v.extend_from_slice(&n.to_be_bytes());
}

fn packbits(row: &[u8]) -> Vec<u8> {
    // Literal runs only, plus runs of identical bytes: enough to exercise both.
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && row[i + run] == row[i] && run < 128 {
            run += 1;
        }
        if run >= 3 {
            out.push((1 - run as i32) as i8 as u8);
            out.push(row[i]);
            i += run;
        } else {
            let mut lit = 1;
            while i + lit < row.len()
                && lit < 128
                && !(i + lit + 2 < row.len()
                    && row[i + lit] == row[i + lit + 1]
                    && row[i + lit] == row[i + lit + 2])
            {
                lit += 1;
            }
            out.push((lit - 1) as u8);
            out.extend_from_slice(&row[i..i + lit]);
            i += lit;
        }
    }
    out
}

/// The bytes of one channel: compression field, then the data.
fn channel(spec: &PsdSpec, r: &Record, slot: usize) -> Vec<u8> {
    let (w, h) = (r.width as usize, r.height as usize);
    let mut out = Vec::new();
    let code = spec
        .compression_override
        .unwrap_or(if spec.rle { 1 } else { 0 });
    be16(&mut out, code);
    let rows: Vec<Vec<u8>> = (0..h)
        .map(|y| {
            let mut row = Vec::new();
            for x in 0..w {
                let v = r.pixels[(y * w + x) * 4 + slot];
                if spec.depth16 {
                    row.extend_from_slice(&(u16::from(v) * 257).to_be_bytes());
                } else {
                    row.push(v);
                }
            }
            row
        })
        .collect();
    if code == 1 {
        let packed: Vec<Vec<u8>> = rows.iter().map(|r| packbits(r)).collect();
        for p in &packed {
            be16(&mut out, p.len() as u16);
        }
        packed.iter().for_each(|p| out.extend_from_slice(p));
    } else {
        rows.iter().for_each(|r| out.extend_from_slice(r));
    }
    out
}

fn block(out: &mut Vec<u8>, key: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(key);
    be32(out, data.len() as u32);
    out.extend_from_slice(data);
}

/// Writes the PSD.
pub fn write(spec: &PsdSpec) -> Vec<u8> {
    let mut records = Vec::new();
    flatten(&spec.nodes, &mut records);

    let mut out = Vec::new();
    out.extend_from_slice(b"8BPS");
    be16(&mut out, if spec.psb { 2 } else { 1 });
    out.extend_from_slice(&[0; 6]);
    be16(&mut out, 3);
    be32(&mut out, spec.height);
    be32(&mut out, spec.width);
    be16(&mut out, if spec.depth16 { 16 } else { 8 });
    be16(&mut out, spec.color_mode);
    be32(&mut out, 0); // color mode data
    be32(&mut out, 0); // image resources

    let mut recs = Vec::new();
    let mut data = Vec::new();
    for r in &records {
        let chans: Vec<(i16, Vec<u8>)> = if r.width * r.height == 0 {
            [0i16, 1, 2, -1]
                .map(|id| (id, 0u16.to_be_bytes().to_vec()))
                .to_vec()
        } else {
            [(0i16, 0usize), (1, 1), (2, 2), (-1, 3)]
                .map(|(id, slot)| (id, channel(spec, r, slot)))
                .to_vec()
        };
        for v in [
            r.top,
            r.left,
            r.top + r.height as i32,
            r.left + r.width as i32,
        ] {
            recs.extend_from_slice(&v.to_be_bytes());
        }
        be16(&mut recs, chans.len() as u16);
        for (id, bytes) in &chans {
            recs.extend_from_slice(&id.to_be_bytes());
            be32(&mut recs, bytes.len() as u32);
        }
        recs.extend_from_slice(b"8BIM");
        recs.extend_from_slice(&r.blend);
        recs.push(r.opacity);
        recs.push(u8::from(r.clipping));
        recs.push(if r.visible { 0 } else { 2 });
        recs.push(0);

        let mut extra = Vec::new();
        if r.layer_mask {
            be32(&mut extra, 20);
            extra.extend_from_slice(&[0; 20]);
        } else {
            be32(&mut extra, 0);
        }
        be32(&mut extra, 0); // blending ranges
        let pascal: Vec<u8> = r.name.bytes().filter(|b| b.is_ascii()).take(255).collect();
        extra.push(pascal.len() as u8);
        extra.extend_from_slice(&pascal);
        let pad = (4 - (1 + pascal.len()) % 4) % 4;
        extra.extend(std::iter::repeat_n(0u8, pad));
        if r.unicode_name {
            let units: Vec<u16> = r.name.encode_utf16().collect();
            let mut d = Vec::new();
            be32(&mut d, units.len() as u32);
            units.iter().for_each(|u| be16(&mut d, *u));
            while d.len() % 4 != 0 {
                d.push(0);
            }
            block(&mut extra, b"luni", &d);
        }
        if r.divider != 0 {
            let mut d = Vec::new();
            be32(&mut d, r.divider);
            block(&mut extra, b"lsct", &d);
        }
        if let Some(key) = r.adjustment {
            block(&mut extra, &key, &[0; 4]);
        }
        if r.effects {
            block(&mut extra, b"lrFX", &[0; 4]);
        }
        be32(&mut recs, extra.len() as u32);
        recs.extend_from_slice(&extra);
        for (_, bytes) in chans {
            data.extend_from_slice(&bytes);
        }
    }

    let mut info = Vec::new();
    be16(&mut info, records.len() as u16);
    info.extend_from_slice(&recs);
    info.extend_from_slice(&data);
    if info.len() % 2 != 0 {
        info.push(0);
    }
    let mut section = Vec::new();
    if spec.lr16 {
        be32(&mut section, 0);
        be32(&mut section, 0); // global layer mask info
        block(&mut section, b"Lr16", &info);
    } else {
        be32(&mut section, info.len() as u32);
        section.extend_from_slice(&info);
        be32(&mut section, 0); // global layer mask info
    }
    be32(&mut out, section.len() as u32);
    out.extend_from_slice(&section);
    out
}
