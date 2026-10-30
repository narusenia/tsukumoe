// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The runtime format `.tmod`: a [`CompiledModel`] laid out as flat,
//! little-endian, 16-byte-aligned arrays, so a loader validates once and then
//! evaluates straight from the buffer. The byte layout is specified in
//! `docs/specifications/file-formats.md` (section 2).

use std::borrow::Cow;
use std::fmt;

use bytemuck::Pod;

use crate::compiled::{CompiledModel, StrRef};
use crate::layout::LayoutError;

#[cfg(target_endian = "big")]
compile_error!("the .tmod format is little-endian and is read in place; big-endian is unsupported");

pub const MAGIC: [u8; 4] = *b"TMOD";
pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 0;
pub const HEADER_LEN: usize = 32;
pub const ENTRY_LEN: usize = 32;
/// Every section starts on a multiple of this many bytes.
pub const ALIGN: usize = 16;
/// Most sections a file may declare.
pub const MAX_SECTIONS: u32 = 256;

/// Header flag bits this reader understands. None are defined in 1.0.
pub const KNOWN_REQUIRED_FLAGS: u32 = 0;
/// Section table flag: a reader that does not know the section must reject the file.
pub const SECTION_REQUIRED: u32 = 1;

pub mod id {
    pub const META: u32 = 0x01;
    pub const STRINGS: u32 = 0x02;
    pub const PARAMETER_NAMES: u32 = 0x03;
    pub const PARAMETERS: u32 = 0x04;
    pub const BINDINGS: u32 = 0x05;
    pub const AXES: u32 = 0x06;
    pub const AXIS_KEYS: u32 = 0x07;
    pub const DRIVERS: u32 = 0x08;
    pub const OPS: u32 = 0x09;
    pub const PART_PARENTS: u32 = 0x0A;
    pub const PART_OPACITY: u32 = 0x0B;
    pub const MESH_NAMES: u32 = 0x0C;
    pub const MESHES: u32 = 0x0D;
    pub const MESH_BASE_POSITIONS: u32 = 0x0E;
    pub const MESH_UVS: u32 = 0x0F;
    pub const MESH_INDICES: u32 = 0x10;
    pub const MESH_MASKS: u32 = 0x11;
    pub const MESH_KF_POSITIONS: u32 = 0x12;
    pub const MESH_KF_ATTRS: u32 = 0x13;
    pub const WARPS: u32 = 0x14;
    pub const WARP_BASE_POINTS: u32 = 0x15;
    pub const WARP_KF_POINTS: u32 = 0x16;
    pub const WARP_KF_OPACITY: u32 = 0x17;
    pub const ROTATIONS: u32 = 0x18;
    pub const ROTATION_KF: u32 = 0x19;
    pub const DEFORMER_ORDER: u32 = 0x1A;
    pub const BONE_NAMES: u32 = 0x1B;
    pub const BONES: u32 = 0x1C;
    pub const BONE_KF: u32 = 0x1D;
    pub const INFLUENCES: u32 = 0x1E;
    pub const PHYSICS: u32 = 0x1F;
    pub const PHYSICS_INPUTS: u32 = 0x20;
    pub const PHYSICS_OUTPUTS: u32 = 0x21;
    pub const PHYSICS_PARTICLES: u32 = 0x22;
    pub const GLUES: u32 = 0x23;
    pub const GLUE_NODES: u32 = 0x24;
    pub const GLUE_KF_INTENSITY: u32 = 0x25;
    pub const BLENDS: u32 = 0x26;
    pub const BLEND_KEYS: u32 = 0x27;
    pub const BLEND_WEIGHTS: u32 = 0x28;
    pub const BLEND_DELTAS: u32 = 0x29;
    pub const BLEND_REFS: u32 = 0x2A;
    /// Optional: one `StrRef` per embedded texture, into `TEXTURE_DATA`.
    pub const TEXTURE_TABLE: u32 = 0x80;
    /// Optional: PNG files, back to back.
    pub const TEXTURE_DATA: u32 = 0x81;
}

/// The name of a section ID in this version, for tools that print the table.
pub fn section_name(section: u32) -> Option<&'static str> {
    use id::*;
    Some(match section {
        META => "meta",
        STRINGS => "strings",
        PARAMETER_NAMES => "parameter_names",
        PARAMETERS => "parameters",
        BINDINGS => "bindings",
        AXES => "axes",
        AXIS_KEYS => "axis_keys",
        DRIVERS => "drivers",
        OPS => "ops",
        PART_PARENTS => "part_parents",
        PART_OPACITY => "part_opacity",
        MESH_NAMES => "mesh_names",
        MESHES => "meshes",
        MESH_BASE_POSITIONS => "mesh_base_positions",
        MESH_UVS => "mesh_uvs",
        MESH_INDICES => "mesh_indices",
        MESH_MASKS => "mesh_masks",
        MESH_KF_POSITIONS => "mesh_kf_positions",
        MESH_KF_ATTRS => "mesh_kf_attrs",
        WARPS => "warps",
        WARP_BASE_POINTS => "warp_base_points",
        WARP_KF_POINTS => "warp_kf_points",
        WARP_KF_OPACITY => "warp_kf_opacity",
        ROTATIONS => "rotations",
        ROTATION_KF => "rotation_kf",
        DEFORMER_ORDER => "deformer_order",
        BONE_NAMES => "bone_names",
        BONES => "bones",
        BONE_KF => "bone_kf",
        INFLUENCES => "influences",
        PHYSICS => "physics",
        PHYSICS_INPUTS => "physics_inputs",
        PHYSICS_OUTPUTS => "physics_outputs",
        PHYSICS_PARTICLES => "physics_particles",
        GLUES => "glues",
        GLUE_NODES => "glue_nodes",
        GLUE_KF_INTENSITY => "glue_kf_intensity",
        BLENDS => "blends",
        BLEND_KEYS => "blend_keys",
        BLEND_WEIGHTS => "blend_weights",
        BLEND_DELTAS => "blend_deltas",
        BLEND_REFS => "blend_refs",
        TEXTURE_TABLE => "texture_table",
        TEXTURE_DATA => "texture_data",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The buffer does not start on a 16-byte boundary; reading it would need a copy.
    Misaligned,
    /// The buffer ends before `what`.
    Truncated(&'static str),
    BadMagic,
    UnsupportedMajor {
        found: u16,
        supported: u16,
    },
    /// The header sets required-feature bits this reader does not know.
    UnknownRequiredFlags(u32),
    UnknownRequiredSection(u32),
    MissingSection(u32),
    DuplicateSection(u32),
    TooManySections(u32),
    /// The header length differs from the buffer length.
    LengthMismatch {
        header: u64,
        actual: u64,
    },
    /// Section `id` has a bad offset, length or count.
    Section {
        id: u32,
        reason: &'static str,
    },
    Texture(&'static str),
    Layout(LayoutError),
    /// A file could not be read.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Misaligned => write!(f, "the buffer is not 16-byte aligned"),
            Self::Truncated(what) => write!(f, "truncated before {what}"),
            Self::BadMagic => write!(f, "not a .tmod file"),
            Self::UnsupportedMajor { found, supported } => {
                write!(
                    f,
                    "major version {found} is not supported (only {supported})"
                )
            }
            Self::UnknownRequiredFlags(bits) => {
                write!(f, "required feature flags {bits:#x} are unknown")
            }
            Self::UnknownRequiredSection(id) => write!(f, "required section {id:#x} is unknown"),
            Self::MissingSection(id) => write!(f, "section {id:#x} is missing"),
            Self::DuplicateSection(id) => write!(f, "section {id:#x} appears twice"),
            Self::TooManySections(n) => write!(f, "{n} sections is over the limit"),
            Self::LengthMismatch { header, actual } => {
                write!(f, "the header says {header} bytes, the buffer has {actual}")
            }
            Self::Section { id, reason } => write!(f, "section {id:#x}: {reason}"),
            Self::Texture(reason) => write!(f, "texture: {reason}"),
            Self::Layout(e) => write!(f, "inconsistent model: {e}"),
            Self::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<LayoutError> for Error {
    fn from(e: LayoutError) -> Self {
        Self::Layout(e)
    }
}

/// A byte buffer whose first byte is 16-byte aligned, for reading a file in
/// place.
#[derive(Debug, Clone, Default)]
pub struct AlignedBuf {
    words: Vec<u128>,
    len: usize,
}

impl AlignedBuf {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut buf = Self::zeroed(bytes.len());
        buf.as_mut_bytes().copy_from_slice(bytes);
        buf
    }

    /// Reads a whole file straight into the aligned buffer.
    pub fn read_file(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        use std::io::Read;
        let io = |e: std::io::Error| Error::Io(format!("{}: {e}", path.as_ref().display()));
        let mut file = std::fs::File::open(path.as_ref()).map_err(io)?;
        let len = usize::try_from(file.metadata().map_err(io)?.len())
            .map_err(|_| Error::Io("file too large".into()))?;
        let mut buf = Self::zeroed(len);
        file.read_exact(buf.as_mut_bytes()).map_err(io)?;
        Ok(buf)
    }

    fn zeroed(len: usize) -> Self {
        Self {
            words: vec![0; len.div_ceil(ALIGN)],
            len,
        }
    }

    fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut bytemuck::cast_slice_mut::<u128, u8>(&mut self.words)[..self.len]
    }

    pub fn as_bytes(&self) -> &[u8] {
        &bytemuck::cast_slice::<u128, u8>(&self.words)[..self.len]
    }
}

impl std::ops::Deref for AlignedBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_bytes()
    }
}

fn align_up(n: usize) -> usize {
    n.next_multiple_of(ALIGN)
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

pub(crate) struct Section<'a> {
    pub(crate) id: u32,
    pub(crate) flags: u32,
    pub(crate) count: u32,
    pub(crate) data: Cow<'a, [u8]>,
}

fn put<'a, T: Pod>(out: &mut Vec<Section<'a>>, id: u32, items: &'a [T]) -> Result<(), Error> {
    out.push(Section {
        id,
        flags: SECTION_REQUIRED,
        count: u32::try_from(items.len()).map_err(|_| Error::Section {
            id,
            reason: "too many elements",
        })?,
        data: Cow::Borrowed(bytemuck::cast_slice(items)),
    });
    Ok(())
}

/// Writes `model` as a `.tmod` file, with optional embedded PNG textures.
/// The same input gives the same bytes.
pub fn write(model: &CompiledModel<'_>, textures: &[&[u8]]) -> Result<Vec<u8>, Error> {
    model.check()?;
    let mut s: Vec<Section> = Vec::new();
    s.push(Section {
        id: id::META,
        flags: SECTION_REQUIRED,
        count: 1,
        data: Cow::Owned(model.corner_capacity.to_le_bytes().to_vec()),
    });
    put(&mut s, id::STRINGS, &model.strings)?;
    put(&mut s, id::PARAMETER_NAMES, &model.parameter_names)?;
    put(&mut s, id::PARAMETERS, &model.parameters)?;
    put(&mut s, id::BINDINGS, &model.bindings)?;
    put(&mut s, id::AXES, &model.axes)?;
    put(&mut s, id::AXIS_KEYS, &model.axis_keys)?;
    put(&mut s, id::DRIVERS, &model.drivers)?;
    put(&mut s, id::OPS, &model.ops)?;
    put(&mut s, id::PART_PARENTS, &model.part_parents)?;
    put(&mut s, id::PART_OPACITY, &model.part_opacity)?;
    put(&mut s, id::MESH_NAMES, &model.mesh_names)?;
    put(&mut s, id::MESHES, &model.meshes)?;
    put(&mut s, id::MESH_BASE_POSITIONS, &model.mesh_base_positions)?;
    put(&mut s, id::MESH_UVS, &model.mesh_uvs)?;
    put(&mut s, id::MESH_INDICES, &model.mesh_indices)?;
    put(&mut s, id::MESH_MASKS, &model.mesh_masks)?;
    put(&mut s, id::MESH_KF_POSITIONS, &model.mesh_kf_positions)?;
    put(&mut s, id::MESH_KF_ATTRS, &model.mesh_kf_attrs)?;
    put(&mut s, id::WARPS, &model.warps)?;
    put(&mut s, id::WARP_BASE_POINTS, &model.warp_base_points)?;
    put(&mut s, id::WARP_KF_POINTS, &model.warp_kf_points)?;
    put(&mut s, id::WARP_KF_OPACITY, &model.warp_kf_opacity)?;
    put(&mut s, id::ROTATIONS, &model.rotations)?;
    put(&mut s, id::ROTATION_KF, &model.rotation_kf)?;
    put(&mut s, id::DEFORMER_ORDER, &model.deformer_order)?;
    put(&mut s, id::BONE_NAMES, &model.bone_names)?;
    put(&mut s, id::BONES, &model.bones)?;
    put(&mut s, id::BONE_KF, &model.bone_kf)?;
    put(&mut s, id::INFLUENCES, &model.influences)?;
    put(&mut s, id::PHYSICS, &model.physics)?;
    put(&mut s, id::PHYSICS_INPUTS, &model.physics_inputs)?;
    put(&mut s, id::PHYSICS_OUTPUTS, &model.physics_outputs)?;
    put(&mut s, id::PHYSICS_PARTICLES, &model.physics_particles)?;
    put(&mut s, id::GLUES, &model.glues)?;
    put(&mut s, id::GLUE_NODES, &model.glue_nodes)?;
    put(&mut s, id::GLUE_KF_INTENSITY, &model.glue_kf_intensity)?;
    put(&mut s, id::BLENDS, &model.blends)?;
    put(&mut s, id::BLEND_KEYS, &model.blend_keys)?;
    put(&mut s, id::BLEND_WEIGHTS, &model.blend_weights)?;
    put(&mut s, id::BLEND_DELTAS, &model.blend_deltas)?;
    put(&mut s, id::BLEND_REFS, &model.blend_refs)?;

    if !textures.is_empty() {
        let (mut table, mut data) = (Vec::new(), Vec::new());
        for t in textures {
            if !t.starts_with(&PNG_SIGNATURE) {
                return Err(Error::Texture("not a PNG file"));
            }
            table.push(StrRef {
                offset: u32::try_from(data.len()).map_err(|_| Error::Texture("too large"))?,
                len: u32::try_from(t.len()).map_err(|_| Error::Texture("too large"))?,
            });
            data.extend_from_slice(t);
        }
        u32::try_from(data.len()).map_err(|_| Error::Texture("too large"))?;
        s.push(Section {
            id: id::TEXTURE_TABLE,
            flags: 0,
            count: table.len() as u32,
            data: Cow::Owned(bytemuck::cast_slice(&table).to_vec()),
        });
        s.push(Section {
            id: id::TEXTURE_DATA,
            flags: 0,
            count: data.len() as u32,
            data: Cow::Owned(data),
        });
    }

    Ok(assemble(&s))
}

/// Lays the sections out: header, table, then each section on a 16-byte
/// boundary, zero padded.
pub(crate) fn assemble(s: &[Section<'_>]) -> Vec<u8> {
    let first = align_up(HEADER_LEN + ENTRY_LEN * s.len());
    let mut offsets = Vec::with_capacity(s.len());
    let mut end = first;
    for sec in s {
        offsets.push(end);
        end = align_up(end + sec.data.len());
    }
    let mut out = Vec::with_capacity(end);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&MAJOR.to_le_bytes());
    out.extend_from_slice(&MINOR.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // required flags
    out.extend_from_slice(&0u32.to_le_bytes()); // optional flags
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&(end as u64).to_le_bytes());
    for (sec, &offset) in s.iter().zip(&offsets) {
        out.extend_from_slice(&sec.id.to_le_bytes());
        out.extend_from_slice(&sec.flags.to_le_bytes());
        out.extend_from_slice(&(offset as u64).to_le_bytes());
        out.extend_from_slice(&(sec.data.len() as u64).to_le_bytes());
        out.extend_from_slice(&sec.count.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    }
    for (sec, &offset) in s.iter().zip(&offsets) {
        out.resize(offset, 0);
        out.extend_from_slice(&sec.data);
    }
    out.resize(end, 0);
    out
}

/// One row of the section table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionInfo {
    pub id: u32,
    pub required: bool,
    pub offset: u64,
    pub length: u64,
    pub count: u32,
}

/// A validated `.tmod` buffer. `model` borrows its arrays from the buffer.
#[derive(Debug, Clone)]
pub struct Tmod<'a> {
    pub major: u16,
    pub minor: u16,
    /// The section table, unknown sections included.
    pub sections: Vec<SectionInfo>,
    pub model: CompiledModel<'a>,
    texture_table: &'a [StrRef],
    texture_data: &'a [u8],
}

fn le_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn le_u64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

struct Entry {
    id: u32,
    flags: u32,
    offset: u64,
    length: u64,
    count: u32,
}

impl<'a> Tmod<'a> {
    /// Validates `bytes` and returns a view of it. The slice must be the
    /// whole file and start on a 16-byte boundary (see [`AlignedBuf`]);
    /// nothing is copied.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if !(bytes.as_ptr() as usize).is_multiple_of(ALIGN) {
            return Err(Error::Misaligned);
        }
        if bytes.len() < HEADER_LEN {
            return Err(Error::Truncated("the header"));
        }
        if bytes[..4] != MAGIC {
            return Err(Error::BadMagic);
        }
        let major = u16::from_le_bytes([bytes[4], bytes[5]]);
        let minor = u16::from_le_bytes([bytes[6], bytes[7]]);
        if major != MAJOR {
            return Err(Error::UnsupportedMajor {
                found: major,
                supported: MAJOR,
            });
        }
        let unknown = le_u32(bytes, 8) & !KNOWN_REQUIRED_FLAGS;
        if unknown != 0 {
            return Err(Error::UnknownRequiredFlags(unknown));
        }
        // Optional flags (offset 12) and the reserved word (offset 20) are ignored.
        let count = le_u32(bytes, 16);
        if count > MAX_SECTIONS {
            return Err(Error::TooManySections(count));
        }
        let total = le_u64(bytes, 24);
        if total != bytes.len() as u64 {
            return Err(Error::LengthMismatch {
                header: total,
                actual: bytes.len() as u64,
            });
        }
        let table_end = HEADER_LEN + ENTRY_LEN * count as usize;
        if bytes.len() < table_end {
            return Err(Error::Truncated("the section table"));
        }
        let data_start = align_up(table_end) as u64;

        let mut entries: Vec<Entry> = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let at = HEADER_LEN + i * ENTRY_LEN;
            let e = Entry {
                id: le_u32(bytes, at),
                flags: le_u32(bytes, at + 4),
                offset: le_u64(bytes, at + 8),
                length: le_u64(bytes, at + 16),
                count: le_u32(bytes, at + 24),
            };
            if entries.iter().any(|p| p.id == e.id) {
                return Err(Error::DuplicateSection(e.id));
            }
            entries.push(e);
        }

        // Unknown sections: rejected when required, skipped otherwise.
        // Known sections are range-checked when taken below.
        let known = |id: u32| {
            id == id::META
                || (id::STRINGS..=id::BLEND_REFS).contains(&id)
                || id == id::TEXTURE_TABLE
                || id == id::TEXTURE_DATA
        };
        for e in &entries {
            if !known(e.id) && e.flags & SECTION_REQUIRED != 0 {
                return Err(Error::UnknownRequiredSection(e.id));
            }
        }

        let find = |sec: u32| entries.iter().find(|e| e.id == sec);
        let raw = |sec: u32| -> Result<Option<&'a [u8]>, Error> {
            let Some(e) = find(sec) else { return Ok(None) };
            let bad = |reason| Error::Section { id: sec, reason };
            if !e.offset.is_multiple_of(ALIGN as u64) || e.offset < data_start {
                return Err(bad("offset is not aligned or lies in the header"));
            }
            let end = e.offset.checked_add(e.length).filter(|&n| n <= total);
            let Some(end) = end else {
                return Err(bad("extends past the end of the file"));
            };
            Ok(Some(&bytes[e.offset as usize..end as usize]))
        };
        fn take<'b, T: Pod>(
            sec: u32,
            e: Option<(&Entry, &'b [u8])>,
        ) -> Result<Cow<'b, [T]>, Error> {
            let bad = |reason| Error::Section { id: sec, reason };
            let Some((e, data)) = e else {
                return Err(Error::MissingSection(sec));
            };
            if e.length != u64::from(e.count) * std::mem::size_of::<T>() as u64 {
                return Err(bad("length does not match count times element size"));
            }
            bytemuck::try_cast_slice(data)
                .map(Cow::Borrowed)
                .map_err(|_| bad("misaligned or odd length"))
        }
        let get = |sec: u32| -> Result<(&Entry, &'a [u8]), Error> {
            match (find(sec), raw(sec)?) {
                (Some(e), Some(d)) => Ok((e, d)),
                _ => Err(Error::MissingSection(sec)),
            }
        };
        macro_rules! arr {
            ($sec:expr) => {
                take($sec, Some(get($sec)?))?
            };
        }

        let (meta_entry, meta) = get(id::META)?;
        if meta.len() != 4 || meta_entry.count != 1 {
            return Err(Error::Section {
                id: id::META,
                reason: "must hold one u32",
            });
        }
        let model = CompiledModel {
            corner_capacity: le_u32(meta, 0),
            strings: arr!(id::STRINGS),
            parameter_names: arr!(id::PARAMETER_NAMES),
            parameters: arr!(id::PARAMETERS),
            bindings: arr!(id::BINDINGS),
            axes: arr!(id::AXES),
            axis_keys: arr!(id::AXIS_KEYS),
            drivers: arr!(id::DRIVERS),
            ops: arr!(id::OPS),
            part_parents: arr!(id::PART_PARENTS),
            part_opacity: arr!(id::PART_OPACITY),
            mesh_names: arr!(id::MESH_NAMES),
            meshes: arr!(id::MESHES),
            mesh_base_positions: arr!(id::MESH_BASE_POSITIONS),
            mesh_uvs: arr!(id::MESH_UVS),
            mesh_indices: arr!(id::MESH_INDICES),
            mesh_masks: arr!(id::MESH_MASKS),
            mesh_kf_positions: arr!(id::MESH_KF_POSITIONS),
            mesh_kf_attrs: arr!(id::MESH_KF_ATTRS),
            warps: arr!(id::WARPS),
            warp_base_points: arr!(id::WARP_BASE_POINTS),
            warp_kf_points: arr!(id::WARP_KF_POINTS),
            warp_kf_opacity: arr!(id::WARP_KF_OPACITY),
            rotations: arr!(id::ROTATIONS),
            rotation_kf: arr!(id::ROTATION_KF),
            deformer_order: arr!(id::DEFORMER_ORDER),
            bone_names: arr!(id::BONE_NAMES),
            bones: arr!(id::BONES),
            bone_kf: arr!(id::BONE_KF),
            influences: arr!(id::INFLUENCES),
            physics: arr!(id::PHYSICS),
            physics_inputs: arr!(id::PHYSICS_INPUTS),
            physics_outputs: arr!(id::PHYSICS_OUTPUTS),
            physics_particles: arr!(id::PHYSICS_PARTICLES),
            glues: arr!(id::GLUES),
            glue_nodes: arr!(id::GLUE_NODES),
            glue_kf_intensity: arr!(id::GLUE_KF_INTENSITY),
            blends: arr!(id::BLENDS),
            blend_keys: arr!(id::BLEND_KEYS),
            blend_weights: arr!(id::BLEND_WEIGHTS),
            blend_deltas: arr!(id::BLEND_DELTAS),
            blend_refs: arr!(id::BLEND_REFS),
        };
        model.check()?;

        let (texture_table, texture_data): (&[StrRef], &[u8]) =
            match (find(id::TEXTURE_TABLE), find(id::TEXTURE_DATA)) {
                (None, None) => (&[], &[]),
                (Some(t), Some(d)) => {
                    let table =
                        take::<StrRef>(id::TEXTURE_TABLE, Some((t, get(id::TEXTURE_TABLE)?.1)))?;
                    let data = get(id::TEXTURE_DATA)?.1;
                    if d.length != u64::from(d.count) {
                        return Err(Error::Section {
                            id: id::TEXTURE_DATA,
                            reason: "length does not match count",
                        });
                    }
                    let Cow::Borrowed(table) = table else {
                        unreachable!("take borrows")
                    };
                    for r in table {
                        let span = (r.offset as usize)
                            .checked_add(r.len as usize)
                            .and_then(|end| data.get(r.offset as usize..end));
                        if !span.is_some_and(|p| p.starts_with(&PNG_SIGNATURE)) {
                            return Err(Error::Texture("outside the data or not a PNG file"));
                        }
                    }
                    (table, data)
                }
                (Some(_), None) => return Err(Error::MissingSection(id::TEXTURE_DATA)),
                (None, Some(_)) => return Err(Error::MissingSection(id::TEXTURE_TABLE)),
            };

        let sections = entries
            .iter()
            .map(|e| SectionInfo {
                id: e.id,
                required: e.flags & SECTION_REQUIRED != 0,
                offset: e.offset,
                length: e.length,
                count: e.count,
            })
            .collect();
        Ok(Self {
            major,
            minor,
            sections,
            model,
            texture_table,
            texture_data,
        })
    }

    pub fn texture_count(&self) -> usize {
        self.texture_table.len()
    }

    /// The PNG bytes of texture `i`, still inside the buffer.
    pub fn texture(&self, i: usize) -> Option<&'a [u8]> {
        let r = self.texture_table.get(i)?;
        let start = r.offset as usize;
        self.texture_data.get(start..start + r.len as usize)
    }
}

#[cfg(test)]
pub(crate) mod tests;
