// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A minimal PSD reader. The bytes are untrusted: every length is checked
//! against the file and the limits before anything is allocated, and layers
//! are decoded one at a time, row by row.

use std::fmt;
use std::io::{Read, Seek, SeekFrom};

use crate::image::Image;

/// Bounds on what the reader accepts. The defaults only reject abnormal
/// input; a real document does not come near them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Longest side of the canvas in pixels.
    pub max_canvas: u32,
    pub max_layers: u32,
    /// Decoded size (RGBA8) of one layer.
    pub max_layer_bytes: u64,
    /// Decoded size (RGBA8) of all layers together.
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_canvas: 16384,
            max_layers: 4096,
            max_layer_bytes: 512 << 20,
            max_total_bytes: 16 << 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PsdError {
    Io(String),
    /// The signature or version is not PSD.
    NotPsd,
    /// Large Document Format.
    Psb,
    /// The data ends before a declared length.
    Truncated(&'static str),
    /// A limit was exceeded; the name says which.
    TooLarge(&'static str),
    UnsupportedDepth(u16),
    UnsupportedColorMode(u16),
    /// A layer channel uses a compression other than raw or RLE.
    UnsupportedCompression {
        layer: String,
        compression: u16,
    },
    Malformed(&'static str),
    /// Group start and end markers do not pair up.
    UnbalancedGroups,
}

impl fmt::Display for PsdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "read error: {e}"),
            Self::NotPsd => write!(f, "not a PSD file"),
            Self::Psb => write!(f, "PSB (large document format) is not supported"),
            Self::Truncated(what) => write!(f, "file is truncated ({what})"),
            Self::TooLarge(what) => write!(f, "limit exceeded: {what}"),
            Self::UnsupportedDepth(d) => write!(f, "unsupported bit depth {d} (8 and 16 only)"),
            Self::UnsupportedColorMode(m) => write!(
                f,
                "unsupported color mode {} (RGB only)",
                color_mode_name(*m)
            ),
            Self::UnsupportedCompression { layer, compression } => write!(
                f,
                "layer '{layer}' uses unsupported compression {compression} (raw and RLE only)"
            ),
            Self::Malformed(what) => write!(f, "malformed file: {what}"),
            Self::UnbalancedGroups => write!(f, "malformed file: unbalanced layer groups"),
        }
    }
}

impl std::error::Error for PsdError {}

fn color_mode_name(mode: u16) -> String {
    match mode {
        0 => "bitmap".into(),
        1 => "grayscale".into(),
        2 => "indexed".into(),
        4 => "CMYK".into(),
        7 => "multichannel".into(),
        8 => "duotone".into(),
        9 => "Lab".into(),
        n => n.to_string(),
    }
}

fn io(e: std::io::Error) -> PsdError {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        PsdError::Truncated("unexpected end of file")
    } else {
        PsdError::Io(e.to_string())
    }
}

/// A rectangle in canvas pixels; `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> u32 {
        (i64::from(self.right) - i64::from(self.left)).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (i64::from(self.bottom) - i64::from(self.top)).max(0) as u32
    }
}

/// What a PSD reader found but does not carry into the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// The layer has a layer mask (ignored).
    LayerMask,
    /// An adjustment or fill layer (ignored); the key names the kind.
    AdjustmentLayer(String),
    /// Layer effects such as shadows (ignored).
    LayerEffects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Index into [`Psd::entries`].
    pub entry: usize,
    pub kind: WarningKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub id: i16,
    /// Bytes at `offset`, including the 2-byte compression field.
    pub(crate) len: u32,
    pub(crate) offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Layer,
    Group,
}

/// A layer or a group, in the order of Photoshop's layer panel (top first;
/// a group comes before its children).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    pub name: String,
    /// Names of the enclosing groups, outermost first.
    pub path: Vec<String>,
    /// Index of the enclosing group in [`Psd::entries`].
    pub parent: Option<usize>,
    /// Position in the stack counted from the bottom; larger is nearer the top.
    pub z: u32,
    pub rect: Rect,
    pub opacity: u8,
    /// The blend mode key, such as `norm`, `mul `, `pass`.
    pub blend_key: [u8; 4],
    pub visible: bool,
    /// The layer is clipped to the layer below it.
    pub clipping: bool,
    pub(crate) channels: Vec<Channel>,
}

impl Entry {
    pub fn blend_key_str(&self) -> String {
        self.blend_key.iter().map(|&b| char::from(b)).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Psd {
    pub width: u32,
    pub height: u32,
    /// 8 or 16.
    pub depth: u16,
    pub entries: Vec<Entry>,
    pub warnings: Vec<Warning>,
}

struct Cur<'a, R> {
    r: &'a mut R,
    pos: u64,
    end: u64,
}

impl<R: Read + Seek> Cur<'_, R> {
    fn need(&self, n: u64, what: &'static str) -> Result<(), PsdError> {
        if self.pos.checked_add(n).is_some_and(|e| e <= self.end) {
            Ok(())
        } else {
            Err(PsdError::Truncated(what))
        }
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], PsdError> {
        self.need(N as u64, what)?;
        let mut b = [0u8; N];
        self.r.read_exact(&mut b).map_err(io)?;
        self.pos += N as u64;
        Ok(b)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, PsdError> {
        Ok(self.array::<1>(what)?[0])
    }
    fn u16(&mut self, what: &'static str) -> Result<u16, PsdError> {
        Ok(u16::from_be_bytes(self.array(what)?))
    }
    fn i16(&mut self, what: &'static str) -> Result<i16, PsdError> {
        Ok(i16::from_be_bytes(self.array(what)?))
    }
    fn u32(&mut self, what: &'static str) -> Result<u32, PsdError> {
        Ok(u32::from_be_bytes(self.array(what)?))
    }
    fn i32(&mut self, what: &'static str) -> Result<i32, PsdError> {
        Ok(i32::from_be_bytes(self.array(what)?))
    }

    /// Reads `n` bytes. The length is checked against the section first.
    fn bytes(&mut self, n: u64, what: &'static str) -> Result<Vec<u8>, PsdError> {
        self.need(n, what)?;
        let mut v = vec![0u8; n as usize];
        self.r.read_exact(&mut v).map_err(io)?;
        self.pos += n;
        Ok(v)
    }

    fn seek(&mut self, to: u64, what: &'static str) -> Result<(), PsdError> {
        if to > self.end {
            return Err(PsdError::Truncated(what));
        }
        if to != self.pos {
            self.r.seek(SeekFrom::Start(to)).map_err(io)?;
            self.pos = to;
        }
        Ok(())
    }

    fn skip(&mut self, n: u64, what: &'static str) -> Result<(), PsdError> {
        self.need(n, what)?;
        self.seek(self.pos + n, what)
    }
}

const SIG: [u8; 4] = *b"8BIM";
const SIG64: [u8; 4] = *b"8B64";

/// Adjustment and fill layer keys (their pixels are not in the channels).
const ADJUSTMENT_KEYS: &[&[u8; 4]] = &[
    b"brit", b"levl", b"curv", b"expA", b"vibA", b"hue ", b"hue2", b"blnc", b"blwh", b"phfl",
    b"mixr", b"clrL", b"nvrt", b"post", b"thrs", b"grdm", b"selc", b"SoCo", b"GdFl", b"PtFl",
];

impl Psd {
    /// Reads the header and the layer structure. No pixel is decoded.
    pub fn read<R: Read + Seek>(r: &mut R, limits: &Limits) -> Result<Self, PsdError> {
        let file_len = r.seek(SeekFrom::End(0)).map_err(io)?;
        r.seek(SeekFrom::Start(0)).map_err(io)?;
        let mut c = Cur {
            r,
            pos: 0,
            end: file_len,
        };

        if &c.array::<4>("signature")? != b"8BPS" {
            return Err(PsdError::NotPsd);
        }
        match c.u16("version")? {
            1 => {}
            2 => return Err(PsdError::Psb),
            _ => return Err(PsdError::NotPsd),
        }
        c.skip(6, "header")?;
        let _channels = c.u16("header")?;
        let height = c.u32("header")?;
        let width = c.u32("header")?;
        let depth = c.u16("header")?;
        let mode = c.u16("header")?;
        if width == 0 || height == 0 {
            return Err(PsdError::Malformed("empty canvas"));
        }
        if width > limits.max_canvas || height > limits.max_canvas {
            return Err(PsdError::TooLarge("canvas size"));
        }
        if depth != 8 && depth != 16 {
            return Err(PsdError::UnsupportedDepth(depth));
        }
        if mode != 3 {
            return Err(PsdError::UnsupportedColorMode(mode));
        }

        for what in ["color mode data", "image resources"] {
            let n = u64::from(c.u32(what)?);
            c.skip(n, what)?;
        }

        let mut psd = Psd {
            width,
            height,
            depth,
            entries: Vec::new(),
            warnings: Vec::new(),
        };
        let section = u64::from(c.u32("layer and mask section")?);
        c.need(section, "layer and mask section")?;
        if section == 0 {
            return Ok(psd);
        }
        let section_end = c.pos + section;
        c.end = section_end;

        let info_len = u64::from(c.u32("layer info")?);
        c.need(info_len, "layer info")?;
        let (info_start, info_end) = if info_len > 0 {
            (c.pos, c.pos + info_len)
        } else {
            // 16 / 32-bit files keep the layers in an additional block.
            match find_wide_layer_info(&mut c)? {
                Some(range) => range,
                None => return Ok(psd),
            }
        };
        c.seek(info_start, "layer info")?;
        c.end = info_end;
        psd.read_layers(&mut c, limits)?;
        Ok(psd)
    }

    fn read_layers<R: Read + Seek>(
        &mut self,
        c: &mut Cur<'_, R>,
        limits: &Limits,
    ) -> Result<(), PsdError> {
        let count = u32::from(c.i16("layer count")?.unsigned_abs());
        if count == 0 {
            return Ok(());
        }
        if count > limits.max_layers {
            return Err(PsdError::TooLarge("layer count"));
        }

        // Records are stored bottom first. Entries are built top first.
        struct Raw {
            rect: Rect,
            channels: Vec<Channel>,
            opacity: u8,
            blend_key: [u8; 4],
            clipping: bool,
            visible: bool,
            name: String,
            divider: u32,
            mask: bool,
            adjustment: Option<[u8; 4]>,
            effects: bool,
        }
        let mut raws: Vec<Raw> = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let rect = Rect {
                top: c.i32("layer record")?,
                left: c.i32("layer record")?,
                bottom: c.i32("layer record")?,
                right: c.i32("layer record")?,
            };
            if rect.right < rect.left || rect.bottom < rect.top {
                return Err(PsdError::Malformed("layer rectangle"));
            }
            let nch = c.u16("layer record")?;
            if nch > 64 {
                return Err(PsdError::Malformed("channel count"));
            }
            let mut channels = Vec::with_capacity(usize::from(nch));
            for _ in 0..nch {
                let id = c.i16("channel info")?;
                let len = c.u32("channel info")?;
                if len < 2 {
                    return Err(PsdError::Malformed("channel length"));
                }
                channels.push(Channel { id, len, offset: 0 });
            }
            if c.array::<4>("blend signature")? != SIG {
                return Err(PsdError::Malformed("blend mode signature"));
            }
            let blend_key = c.array::<4>("blend key")?;
            let opacity = c.u8("opacity")?;
            let clipping = c.u8("clipping")? != 0;
            let flags = c.u8("flags")?;
            c.u8("filler")?;
            let extra = u64::from(c.u32("extra data")?);
            c.need(extra, "extra data")?;
            let ex_end = c.pos + extra;
            let saved_end = c.end;
            c.end = ex_end;

            let mask_len = u64::from(c.u32("layer mask data")?);
            c.skip(mask_len, "layer mask data")?;
            let ranges_len = u64::from(c.u32("blending ranges")?);
            c.skip(ranges_len, "blending ranges")?;
            let name_len = u64::from(c.u8("layer name")?);
            let pascal = c.bytes(name_len, "layer name")?;
            let pad = (4 - (1 + name_len) % 4) % 4;
            c.skip(pad.min(c.end - c.pos), "layer name")?;
            let mut raw = Raw {
                rect,
                channels,
                opacity,
                blend_key,
                clipping,
                visible: flags & 2 == 0,
                name: pascal.iter().map(|&b| char::from(b)).collect(),
                divider: 0,
                mask: mask_len > 0,
                adjustment: None,
                effects: false,
            };
            read_blocks(
                c,
                &mut raw.name,
                &mut raw.divider,
                &mut raw.adjustment,
                &mut raw.effects,
            )?;
            c.end = saved_end;
            c.seek(ex_end, "extra data")?;
            raws.push(raw);
        }

        // Channel image data follows the records, layer by layer.
        let mut cursor = c.pos;
        let mut total = 0u64;
        for raw in &mut raws {
            let layer_bytes = u64::from(raw.rect.width()) * u64::from(raw.rect.height()) * 4;
            if layer_bytes > limits.max_layer_bytes {
                return Err(PsdError::TooLarge("layer size"));
            }
            total = total.saturating_add(layer_bytes);
            if total > limits.max_total_bytes {
                return Err(PsdError::TooLarge("total layer size"));
            }
            for ch in &mut raw.channels {
                ch.offset = cursor;
                cursor = cursor
                    .checked_add(u64::from(ch.len))
                    .filter(|&e| e <= c.end)
                    .ok_or(PsdError::Truncated("channel image data"))?;
            }
        }

        // Walk from the top. A divider of kind 1 or 2 opens a group whose
        // name is on its record; kind 3 closes it.
        let mut stack: Vec<usize> = Vec::new();
        for (stored, raw) in raws.into_iter().enumerate().rev() {
            let z = stored as u32;
            let parent = stack.last().copied();
            let path: Vec<String> = stack
                .iter()
                .map(|&i| self.entries[i].name.clone())
                .collect();
            if raw.divider == 3 {
                if stack.pop().is_none() {
                    return Err(PsdError::UnbalancedGroups);
                }
                continue;
            }
            let index = self.entries.len();
            let is_group = matches!(raw.divider, 1 | 2);
            if raw.mask {
                self.warnings.push(Warning {
                    entry: index,
                    kind: WarningKind::LayerMask,
                });
            }
            if let Some(key) = raw.adjustment {
                self.warnings.push(Warning {
                    entry: index,
                    kind: WarningKind::AdjustmentLayer(
                        key.iter().map(|&b| char::from(b)).collect(),
                    ),
                });
            }
            if raw.effects {
                self.warnings.push(Warning {
                    entry: index,
                    kind: WarningKind::LayerEffects,
                });
            }
            self.entries.push(Entry {
                kind: if is_group {
                    EntryKind::Group
                } else {
                    EntryKind::Layer
                },
                name: raw.name,
                path,
                parent,
                z,
                rect: raw.rect,
                opacity: raw.opacity,
                blend_key: raw.blend_key,
                visible: raw.visible,
                clipping: raw.clipping,
                channels: raw.channels,
            });
            if is_group {
                stack.push(index);
            }
        }
        if !stack.is_empty() {
            return Err(PsdError::UnbalancedGroups);
        }
        Ok(())
    }

    /// Decodes one layer to RGBA8 (straight alpha, 16-bit reduced to 8).
    /// Only this layer's rows are read; memory is the result plus one row.
    pub fn decode_layer<R: Read + Seek>(
        &self,
        r: &mut R,
        index: usize,
        limits: &Limits,
    ) -> Result<Image, PsdError> {
        let mut out = Image::empty();
        self.decode_layer_into(r, index, limits, &mut out)?;
        Ok(out)
    }

    /// Like [`Psd::decode_layer`], into `out`, whose buffer is reused when it
    /// is big enough: scanning many layers then allocates once for the
    /// largest instead of once per layer.
    pub fn decode_layer_into<R: Read + Seek>(
        &self,
        r: &mut R,
        index: usize,
        limits: &Limits,
        out: &mut Image,
    ) -> Result<(), PsdError> {
        let entry = self
            .entries
            .get(index)
            .ok_or(PsdError::Malformed("no such layer"))?;
        if entry.kind != EntryKind::Layer {
            return Err(PsdError::Malformed("a group has no pixels"));
        }
        let (w, h) = (entry.rect.width() as usize, entry.rect.height() as usize);
        if w == 0 || h == 0 {
            *out = Image::empty();
            return Ok(());
        }
        let bytes = w as u64 * h as u64 * 4;
        if bytes > limits.max_layer_bytes {
            return Err(PsdError::TooLarge("layer size"));
        }
        let bps = usize::from(self.depth / 8);
        let row_len = w * bps;
        let mut rgba = std::mem::take(&mut out.rgba);
        rgba.clear();
        if rgba.capacity() < bytes as usize {
            note_allocation();
        }
        rgba.resize(bytes as usize, 0);
        let mut seen = [false; 4];
        let mut row = vec![0u8; row_len];
        let mut packed = Vec::new();
        for ch in &entry.channels {
            let slot = match ch.id {
                0 => 0,
                1 => 1,
                2 => 2,
                -1 => 3,
                _ => continue,
            };
            if seen[slot] {
                return Err(PsdError::Malformed("duplicate channel"));
            }
            seen[slot] = true;
            r.seek(SeekFrom::Start(ch.offset)).map_err(io)?;
            let mut c = Cur {
                r,
                pos: ch.offset,
                end: ch.offset + u64::from(ch.len),
            };
            let compression = c.u16("channel compression")?;
            match compression {
                0 => {
                    for y in 0..h {
                        c.need(row_len as u64, "raw channel data")?;
                        c.r.read_exact(&mut row).map_err(io)?;
                        c.pos += row_len as u64;
                        store_row(&mut rgba, &row, y, w, slot, bps);
                    }
                }
                1 => {
                    let table = c.bytes(2 * h as u64, "RLE row table")?;
                    let counts: Vec<u16> = table
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect();
                    let sum: u64 = counts.iter().map(|&n| u64::from(n)).sum();
                    if sum > c.end - c.pos {
                        return Err(PsdError::Malformed("RLE rows exceed the channel"));
                    }
                    for (y, &count) in counts.iter().enumerate() {
                        if usize::from(count) > 2 * row_len + 2 {
                            return Err(PsdError::Malformed("RLE row length"));
                        }
                        packed.resize(usize::from(count), 0);
                        c.need(u64::from(count), "RLE row")?;
                        c.r.read_exact(&mut packed).map_err(io)?;
                        c.pos += u64::from(count);
                        unpack_row(&packed, &mut row)?;
                        store_row(&mut rgba, &row, y, w, slot, bps);
                    }
                }
                other => {
                    return Err(PsdError::UnsupportedCompression {
                        layer: entry.name.clone(),
                        compression: other,
                    });
                }
            }
        }
        if !(seen[0] && seen[1] && seen[2]) {
            return Err(PsdError::Malformed("layer lacks color channels"));
        }
        if !seen[3] {
            rgba.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p[3] = 255);
        }
        *out = Image {
            width: w as u32,
            height: h as u32,
            rgba,
        };
        Ok(())
    }
}

thread_local! {
    static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_allocation() {
    ALLOCATIONS.with(|c| c.set(c.get() + 1));
}

/// How many times a layer buffer had to grow on this thread (for tests that
/// check buffers are reused).
pub fn layer_buffer_allocations() -> usize {
    ALLOCATIONS.with(std::cell::Cell::get)
}

fn store_row(rgba: &mut [u8], row: &[u8], y: usize, w: usize, slot: usize, bps: usize) {
    let out = &mut rgba[y * w * 4..(y + 1) * w * 4];
    if bps == 1 {
        for (x, &v) in row.iter().enumerate() {
            out[x * 4 + slot] = v;
        }
    } else {
        for (x, v) in row.as_chunks::<2>().0.iter().enumerate() {
            let v = u32::from(u16::from_be_bytes([v[0], v[1]]));
            out[x * 4 + slot] = ((v * 255 + 32767) / 65535) as u8;
        }
    }
}

/// PackBits: fills `out` exactly or fails.
fn unpack_row(packed: &[u8], out: &mut [u8]) -> Result<(), PsdError> {
    let bad = PsdError::Malformed("RLE row does not fill the row");
    let (mut i, mut o) = (0usize, 0usize);
    while i < packed.len() {
        let n = packed[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            if i + len > packed.len() || o + len > out.len() {
                return Err(bad);
            }
            out[o..o + len].copy_from_slice(&packed[i..i + len]);
            i += len;
            o += len;
        } else if n != -128 {
            let len = 1 - n as isize;
            let len = len as usize;
            if i >= packed.len() || o + len > out.len() {
                return Err(bad);
            }
            out[o..o + len].fill(packed[i]);
            i += 1;
            o += len;
        }
    }
    if o == out.len() { Ok(()) } else { Err(bad) }
}

/// Finds the `Lr16` / `Lr32` block that holds the layer info of 16 and 32
/// bit files. The reader is just past the (empty) layer info.
fn find_wide_layer_info<R: Read + Seek>(
    c: &mut Cur<'_, R>,
) -> Result<Option<(u64, u64)>, PsdError> {
    // Global layer mask info.
    let n = u64::from(c.u32("global layer mask info")?);
    c.skip(n, "global layer mask info")?;
    while c.pos + 12 <= c.end {
        let sig = c.array::<4>("additional info")?;
        if sig != SIG && sig != SIG64 {
            return Ok(None);
        }
        let key = c.array::<4>("additional info")?;
        let len = u64::from(c.u32("additional info")?);
        c.need(len, "additional info")?;
        if &key == b"Lr16" || &key == b"Lr32" {
            return Ok(Some((c.pos, c.pos + len)));
        }
        let next = c.pos + len;
        c.seek(next, "additional info")?;
        // Blocks are padded to a multiple of four.
        let aligned = next.next_multiple_of(4);
        if aligned > next && aligned <= c.end {
            c.seek(aligned, "additional info")?;
        }
    }
    Ok(None)
}

/// Reads the tagged blocks of a layer record up to the end of its extra data.
fn read_blocks<R: Read + Seek>(
    c: &mut Cur<'_, R>,
    name: &mut String,
    divider: &mut u32,
    adjustment: &mut Option<[u8; 4]>,
    effects: &mut bool,
) -> Result<(), PsdError> {
    let ex_end = c.end;
    while c.pos + 12 <= ex_end {
        let sig = c.array::<4>("layer block")?;
        if sig != SIG && sig != SIG64 {
            break;
        }
        let key = c.array::<4>("layer block")?;
        let len = u64::from(c.u32("layer block")?);
        c.need(len, "layer block")?;
        let start = c.pos;
        let end = start + len;
        match &key {
            b"luni" if len >= 4 => {
                let n = u64::from(c.u32("unicode name")?);
                if n.checked_mul(2).is_none_or(|b| b > len - 4) {
                    return Err(PsdError::Malformed("unicode name length"));
                }
                let raw = c.bytes(n * 2, "unicode name")?;
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .collect();
                *name = String::from_utf16_lossy(&units);
            }
            b"lsct" | b"lsdk" if len >= 4 => *divider = c.u32("section divider")?,
            b"lrFX" | b"lfx2" | b"lmfx" => *effects = true,
            k if ADJUSTMENT_KEYS.contains(&k) => *adjustment = Some(*k),
            _ => {}
        }
        c.seek(end, "layer block")?;
        // The next block starts right after, or after padding to 2 or 4.
        let mut next = None;
        for cand in [end, end.next_multiple_of(2), end.next_multiple_of(4)] {
            if cand + 12 > ex_end {
                continue;
            }
            c.seek(cand, "layer block")?;
            let probe = c.array::<4>("layer block")?;
            c.seek(cand, "layer block")?;
            if probe == SIG || probe == SIG64 {
                next = Some(cand);
                break;
            }
        }
        if next.is_none() {
            break;
        }
    }
    Ok(())
}
