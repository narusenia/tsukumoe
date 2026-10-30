# Tsukumoe File Formats

Related: REQ-FMT-001 to REQ-FMT-006

License of this document: CC-BY 4.0

Status: the manifest, the containers, the model files and the published JSON Schema are defined (M1-08); the runtime format `.tmod` 1.0 is defined (M3-01); motions and expressions in it are M3-02.

## 1. Project format (`.tsukumoe`)

The editing format. Two physical layouts with identical content:

- **Archive**: a zip file with extension `.tsukumoe`
- **Directory**: an unpacked directory with the same tree, intended for git and for direct editing by agents

```
manifest.json        # format version, feature flags, generator
model.json           # parameters, parts, meshes, deformers, bones, drivers
physics.json
motions/*.json
expressions/*.json
textures/*.png
sources/             # optional references to original PSD files
```

- Encoding: UTF-8 JSON. Object keys are written in a stable order so diffs stay readable.
- Schema: JSON Schema generated from the Rust types, published under `schema/` (`manifest`, `model`, `physics`, `motion`, `expression`). `cargo test -p tsukumoe-format --test schema` fails when they drift; regenerate with `TSUKUMOE_UPDATE_SCHEMA=1`.

### 1.1 Manifest

`manifest.json` is the only file a reader needs before it can decide whether to read the rest.

```json
{
  "format": "tsukumoe-project",
  "version": { "major": 1, "minor": 0 },
  "generator": "tsukumoe-cli 0.1.0",
  "features": [{ "name": "example", "required": false }]
}
```

- `format` must be `tsukumoe-project`; anything else is not a project.
- `version.major` changes when an old reader cannot read a new file. A reader rejects a major version newer than it supports. `minor` only adds optional content.
- `features` lists capabilities the file uses. A reader that does not know a feature ignores it when `required` is false and rejects the file when it is true. Unknown optional features are kept when the file is written again.
- `generator` is informational.

### 1.2 Containers

- Paths are relative and `/`-separated. A path that is empty, absolute, or contains `\`, `:`, a control character, or a `.` / `..` / empty segment is rejected.
- Archive entries are written in path order with a fixed timestamp, so the same project gives the same bytes.
- Readers bound the entry count, the size of each file and the total size. The declared size in an archive is not trusted.
- A directory is read without following links; a link or another non-regular file is an error.

### 1.3 Files of the model

All are UTF-8 JSON in the order of the field definitions in the Rust types (`tsukumoe-format::dto`).

| File | Content |
|---|---|
| `model.json` | `parameters`, `parts`, `bindings`, `bones`, `warps`, `rotations`, `meshes`, `glues`, `blend_shapes`, `drivers`, optional `pose` |
| `physics.json` | `pendulums`, `chains` |
| `motions/NNNNNN-name.json` | one motion; the real name is the `name` field inside |
| `expressions/NNNNNN-name.json` | one expression, same naming |
| `textures/*.png` | texture `i` of a mesh is the `i`-th PNG under `textures/` in path order; straight alpha |

- Elements refer to each other by **persistent name**, never by an internal index. The one exception is a keyform grid's `binding`: the position in `bindings`, because bindings are shared and unnamed.
- Files under `motions/` and `expressions/` are numbered in the order of the model's table so the order survives; the name part is the real name with every byte outside `A-Z a-z 0-9 - _` written as `%XX`. A reader takes the name from inside the file.
- Fields with a natural default (opacity 1, draw order 500, colors, `visible`, empty lists) may be left out when a file is edited by hand; a writer always writes them.
- Reading validates the content as a model: a name that is not defined, a duplicate name, or a geometry that does not fit is an error that names the element.
- Numbers must be finite; JSON has no NaN or infinity.

## 2. Runtime format (`.tmod`)

The distribution format. Designed for zero-copy loading: after validation, the evaluator reads directly from the loaded buffer. The reference implementation is `tsukumoe-core::tmod` (write, load, `AlignedBuf`); the byte layout below is the contract. Semantics of the data (what the evaluator does with it) are in `evaluation.md`, `draw-list.md` and `physics.md`.

- Byte order: little-endian. Floats are IEEE 754 binary32. Every integer is unsigned unless stated.
- Alignment: every section starts on a 16-byte boundary, so an element of any section can be read in place when the file itself is loaded at a 16-byte boundary. A loader that cannot guarantee this must copy the file into an aligned buffer first.
- Layout: header, section table, sections (evaluation data in SoA form, string table, optional embedded textures). Padding is zero.
- Untrusted input: a loader must validate all offsets, lengths and indices before use (section 2.5). A file that passes the checks can be evaluated without any further bounds checking by the loader's own logic.
- Writers are deterministic: the same model gives the same bytes. Sections are written in ascending ID order, in the order of the table in section 2.3.

### 2.1 Header (32 bytes at offset 0)

| Offset | Type | Field |
|---|---|---|
| 0 | `u8[4]` | magic, ASCII `TMOD` |
| 4 | `u16` | major version (this document: 1) |
| 6 | `u16` | minor version (this document: 0) |
| 8 | `u32` | required feature flags |
| 12 | `u32` | optional feature flags |
| 16 | `u32` | section count N (at most 256) |
| 20 | `u32` | reserved; writers write 0, readers ignore it |
| 24 | `u64` | total file length in bytes; must equal the length of the buffer |

- A reader rejects a file whose major version differs from the one it supports, and accepts any minor version of that major. A new minor version only adds optional content.
- Feature flags (section 3): a reader rejects the file when `required & !known_required != 0`, and ignores optional bits it does not know. Version 1.0 defines no flag, so a 1.0 reader rejects any set required bit.

### 2.2 Section table (N entries of 32 bytes at offset 32)

| Offset | Type | Field |
|---|---|---|
| 0 | `u32` | section ID |
| 4 | `u32` | flags; bit 0 = required, other bits are reserved and ignored |
| 8 | `u64` | offset of the section from the start of the file; a multiple of 16 |
| 16 | `u64` | length in bytes (excluding padding) |
| 24 | `u32` | element count |
| 28 | `u32` | reserved; writers write 0, readers ignore it |

- The first section starts at the first multiple of 16 at or after `32 + 32 * N`. Writers place the sections one after another, each starting on the next multiple of 16 after the previous one ends, and the file ends at a multiple of 16.
- A section ID appears at most once. A reader rejects the file when an ID is repeated.
- A section whose ID the reader does not know is skipped when its required bit is clear, and the file is rejected when the bit is set. Offsets and lengths of skipped sections are not interpreted.
- For a known section, `length == count * element size` (the sizes are in section 2.3), `offset + length` does not pass the end of the file, and `offset` is at least the first multiple of 16 at or after the end of the table. Overlaps are harmless to a reader and are not checked.
- All sections of IDs `0x01` to `0x2A` are required to be present, and a writer marks them required. A section of length 0 is still present, with its offset on a 16-byte boundary inside the file.

### 2.3 Sections

"Index" fields are element positions in another section; a `start` / `count` pair addresses `count` consecutive elements from position `start` of the section named. `NONE` is `0xFFFFFFFF` and marks an absent reference. Counts and indices are `u32`, so a section holds fewer than 2^32 - 1 elements.

| ID | Name | Element | Size | Content |
|---|---|---|---|---|
| `0x01` | `meta` | `u32` | 4 | one element: `corner_capacity`, the total corner scratch a frame needs |
| `0x02` | `strings` | `u8` | 1 | UTF-8 text that names point into; not NUL-terminated |
| `0x03` | `parameter_names` | `StrRef` | 8 | one per parameter |
| `0x04` | `parameters` | `ParamDef` | 16 | |
| `0x05` | `bindings` | `BindingDef` | 20 | |
| `0x06` | `axes` | `AxisDef` | 12 | |
| `0x07` | `axis_keys` | `f32` | 4 | keys of all axes |
| `0x08` | `drivers` | `DriverDef` | 12 | in running order: a driver comes after the drivers whose outputs it reads |
| `0x09` | `ops` | `Op` | 8 | instructions of all drivers |
| `0x0A` | `part_parents` | `u32` | 4 | parent part index or `NONE`; parents come first |
| `0x0B` | `part_opacity` | `f32` | 4 | one per part |
| `0x0C` | `mesh_names` | `StrRef` | 8 | one per mesh |
| `0x0D` | `meshes` | `MeshDef` | 108 | |
| `0x0E` | `mesh_base_positions` | `f32` | 4 | `x, y` of every vertex of every mesh |
| `0x0F` | `mesh_uvs` | `f32` | 4 | `u, v` per vertex; same length as `mesh_base_positions` |
| `0x10` | `mesh_indices` | `u32` | 4 | triangle lists, local to each mesh |
| `0x11` | `mesh_masks` | `u32` | 4 | mesh indices of the clipping meshes |
| `0x12` | `mesh_kf_positions` | `f32` | 4 | keyform vertex positions |
| `0x13` | `mesh_kf_attrs` | `f32[8]` | 32 | keyform attributes (below) |
| `0x14` | `warps` | `WarpDef` | 48 | |
| `0x15` | `warp_base_points` | `f32` | 4 | `x, y` of the control points |
| `0x16` | `warp_kf_points` | `f32` | 4 | keyform control points |
| `0x17` | `warp_kf_opacity` | `f32` | 4 | one per warp keyform |
| `0x18` | `rotations` | `RotationDef` | 44 | |
| `0x19` | `rotation_kf` | `RotationKf` | 28 | |
| `0x1A` | `deformer_order` | `ParentRef` | 8 | every warp and rotation once, parents first |
| `0x1B` | `bone_names` | `StrRef` | 8 | one per bone |
| `0x1C` | `bones` | `BoneDef` | 56 | parents come first |
| `0x1D` | `bone_kf` | `BoneTransform` | 20 | |
| `0x1E` | `influences` | `Influence` | 32 | one per vertex of each skinned mesh |
| `0x1F` | `physics` | `PhysicsDef` | 60 | |
| `0x20` | `physics_inputs` | `PhysicsInput` | 16 | |
| `0x21` | `physics_outputs` | `PhysicsOutput` | 24 | |
| `0x22` | `physics_particles` | `PhysicsParticle` | 32 | |
| `0x23` | `glues` | `GlueDef` | 28 | |
| `0x24` | `glue_nodes` | `GlueNode` | 12 | |
| `0x25` | `glue_kf_intensity` | `f32` | 4 | |
| `0x26` | `blends` | `BlendDef` | 24 | |
| `0x27` | `blend_keys` | `f32` | 4 | |
| `0x28` | `blend_weights` | `f32` | 4 | same length as `blend_keys` |
| `0x29` | `blend_deltas` | `f32` | 4 | `x, y` offsets of all blend shapes |
| `0x2A` | `blend_refs` | `u32` | 4 | blend indices, grouped per mesh and per warp |
| `0x80` | `texture_table` | `StrRef` | 8 | optional: one per embedded texture, into `texture_data` |
| `0x81` | `texture_data` | `u8` | 1 | optional: the PNG files back to back (straight alpha) |

IDs from `0x2B` to `0x7F` are reserved for later minor versions (M3-02 adds motions and expressions as optional sections); `0x80` and up are optional content. The texture sections are written together or not at all; texture `i` of a mesh (`texture` field) is entry `i` of the table, in the order of `textures/*.png` in the project.

### 2.4 Elements

Offsets are in bytes from the start of the element. Fields are `u32` unless a type is given. Flags are `0` or `1`.

**`StrRef`** (8): `offset` (0), `len` (4). A byte range of `strings` (or of `texture_data`); the range must lie inside it. Names must be valid UTF-8.

**`ParamDef`** (16): `min` (0, f32), `max` (4, f32), `default` (8, f32), `repeat` (12, flag). A repeating parameter wraps into `[min, max)`; otherwise values clamp to `[min, max]`. A value that is not finite becomes `default`.

**`BindingDef`** (20): `axis_start` (0), `axis_count` (4), `keyform_count` (8), `corner_start` (12), `max_corners` (16). Axes: `axes[axis_start .. axis_start + axis_count]`. `keyform_count` is the product of the axes' key counts; `max_corners` is the product of `min(key_count, 2)`. `corner_start` is the sum of the `max_corners` of the bindings before this one, so `corner_capacity` (section `meta`) is the sum over all bindings. Keyforms sit on the dense grid of key combinations, the first axis varying slowest.

**`AxisDef`** (12): `parameter` (0, index), `key_start` (4), `key_count` (8). Keys: `axis_keys[key_start .. key_start + key_count]`, strictly ascending and finite, at least one.

**`KeyformRef`** (8): `binding` (0, index or `NONE`), `first` (4). `NONE` means the element has no keyforms and uses its base values. Otherwise the element has `keyform_count` keyforms of the binding, stored from index `first` in the keyform array of its kind (`mesh_kf_attrs`, `warp_kf_opacity`, `rotation_kf`, `bone_kf`, `glue_kf_intensity`); array-valued keyforms (mesh positions, warp points) have a separate float offset field, and keyform `k` starts at `offset + k * length` where `length` is the element's float count.

**`DriverDef`** (12): `output` (0, parameter index), `start` (4), `len` (8). Instructions: `ops[start .. start + len]`.

**`Op`** (8): `kind` (0), `value` (4). Kinds: 0 `Const` (`value` holds the f32 bits), 1 `Param` (`value` is a parameter index), 2 `Add`, 3 `Sub`, 4 `Mul`, 5 `Div`, 6 `Neg`, 7 `Abs`, 8 `Min`, 9 `Max`, 10 `Clamp`, 11 `Lerp`, 12 `Sin`, 13 `Cos`; `value` is 0 for kinds 2 to 13. Postfix stack machine as in `driver-expressions.md`. A driver's instructions must never pop from an empty stack, hold more than 16 values, and must leave exactly one value.

**`ParentRef`** (8): `kind` (0), `index` (4). Kind 0 none (`index` 0), 1 warp deformer, 2 rotation deformer.

**`MeshDef`** (108):

| Offset | Field |
|---|---|
| 0 | `vertex_start`: first vertex in `mesh_base_positions` / `mesh_uvs` (a float offset is twice this) |
| 4 | `vertex_count` |
| 8 | `parent` (`ParentRef`, 8 bytes) |
| 16 | `part` (index) |
| 20 | `base_attrs` (`f32[8]`) |
| 52 | `keyforms` (`KeyformRef`, 8 bytes) |
| 60 | `kf_positions`: float offset of keyform 0 in `mesh_kf_positions`; each keyform is `2 * vertex_count` floats |
| 64 | `blend_start`: first entry in `blend_refs` |
| 68 | `blend_count` |
| 72 | `index_start`: first entry in `mesh_indices` |
| 76 | `index_count` |
| 80 | `texture` |
| 84 | `blend_mode`: 0 normal, 1 additive, 2 multiply |
| 88 | `mask_start`: first entry in `mesh_masks` |
| 92 | `mask_count` |
| 96 | `mask_inverted` (flag) |
| 100 | `double_sided` (flag) |
| 104 | `influence_start`: first entry in `influences`, or `NONE` |

Attributes (`base_attrs`, `mesh_kf_attrs`): opacity, draw order, multiply color r, g, b, screen color r, g, b.

**`WarpDef`** (48): `rows` (0), `columns` (4), `parent` (8, `ParentRef`), `point_start` (16, float offset in `warp_base_points`), `point_count` (20, points), `base_opacity` (24, f32), `keyforms` (28, `KeyformRef`), `kf_points` (36, float offset of keyform 0 in `warp_kf_points`; each keyform is `2 * point_count` floats), `blend_start` (40), `blend_count` (44). `point_count == (rows + 1) * (columns + 1)`, rows and columns at least 1. `warp_kf_opacity[first + k]` is the opacity of keyform `k`.

**`RotationKf`** (28): `origin_x` (0, f32), `origin_y` (4, f32), `angle_degrees` (8, f32), `scale` (12, f32), `reflect_x` (16, flag), `reflect_y` (20, flag), `opacity` (24, f32). **`RotationDef`** (44): `parent` (0, `ParentRef`), `base` (8, `RotationKf`), `keyforms` (36, `KeyformRef`).

**`BoneTransform`** (20): `translation_x` (0), `translation_y` (4), `rotation_degrees` (8), `scale_x` (12), `scale_y` (16), all f32. **`BoneDef`** (56): `parent` (0, bone index or `NONE`, less than its own index), `base` (4, `BoneTransform`), `keyforms` (24, `KeyformRef`), `rest_inverse` (32, six f32: `m0 m1 m2 m3 t0 t1`, mapping a point to `[m0 x + m2 y + t0, m1 x + m3 y + t1]`).

**`Influence`** (32): `bones` (0, four indices, `NONE` for an unused slot), `weights` (16, four f32).

**`PhysicsDef`** (60): `name` (0, `StrRef`), `kind` (8), `values` (12, six f32), `input_start` (36), `input_count` (40), `output_start` (44), `output_count` (48), `particle_start` (52), `particle_count` (56). Kind 0 is a pendulum group: `values` are the position range `min, max, default` then the angle range `min, max, default`. Kind 1 is a chain: `gravity_x, gravity_y, wind_x, wind_y, 0, 0`.

**`PhysicsInput`** (16): `parameter` (0), `channel` (4: 0 X, 1 Y, 2 angle), `weight` (8, f32, percent), `reflect` (12, flag). **`PhysicsOutput`** (24): `parameter` (0), `channel` (4), `particle` (8, index within the group), `scale` (12, f32), `weight` (16, f32, percent), `reflect` (20, flag). **`PhysicsParticle`** (32, all f32): `rest_x`, `rest_y`, `length`, `mobility`, `delay`, `acceleration`, `stiffness`, `damping`; fields a kind does not use are 0.

**`GlueDef`** (28): `mesh_a` (0), `mesh_b` (4), `node_start` (8), `node_count` (12), `intensity` (16, f32), `keyforms` (20, `KeyformRef`; keyform `k` is `glue_kf_intensity[first + k]`). **`GlueNode`** (12): `index_a` (0, vertex of mesh a), `index_b` (4, vertex of mesh b), `weight` (8, f32).

**`BlendDef`** (24): `target_kind` (0: 0 mesh, 1 warp), `target` (4, index), `parameter` (8), `key_start` (12), `key_count` (16), `delta_start` (20, float offset in `blend_deltas`; `2 * vertex_count` or `2 * point_count` floats). Keys and weights are `blend_keys` / `blend_weights` from `key_start`, `key_count` entries each; keys are strictly ascending and finite. The blend shapes of a mesh or warp are `blend_refs[blend_start .. blend_start + blend_count]`; each entry is a blend index whose target is that mesh or warp.

### 2.5 What a loader must check

A loader checks the following before it exposes the data, and rejects the file with an error otherwise. It never trusts a length, offset or index it has not checked, and a failed check never panics or reads outside the buffer.

1. Header and table: magic, major version, required flags, `section count <= 256`, total length equal to the buffer, table inside the file, no repeated ID, no unknown required section, every required known section present.
2. Each known section: aligned offset not inside the header or table, range inside the file, `length == count * element size`.
3. Strings: every `StrRef` lies inside its byte array and names are UTF-8; one name per parameter, mesh and bone.
4. Numbers: every `f32` is finite; flags are 0 or 1; enumerations (`kind`, `channel`, `blend_mode`, op kind) have known values; part, bone and deformer parents come before their children.
5. Ranges and indices: every `start + count` lies inside the array it addresses; every index (parameter, binding, part, mesh, warp, rotation, bone, blend, vertex in `mesh_indices` and glue nodes) is inside its array; keyform ranges cover `keyform_count` keyforms of the element's size; `deformer_order` lists each deformer once with parents first.
6. Bindings: key lists strictly ascending; `keyform_count`, `max_corners` and `corner_start` agree with the axes; `corner_capacity` equals their sum.
7. Drivers: the instruction rules of `Op` above.
8. Textures (when present): both sections present, every entry inside `texture_data` and starting with the PNG signature.

Limits: at most 256 sections; at most 16 axes per binding; `corner_capacity` at most 2^20; arrays of fewer than 2^32 - 1 elements; instruction stack of 16.

### 2.6 Sample

`samples/quad.tmod` is a complete 1.0 file: one parameter `P` (0 to 1), one mesh `Quad` with four vertices and two triangles, two keyforms on `P` that move the quad one unit to the right, and one embedded 1x1 PNG. `samples/quad.expected.json` lists what a loader must read from it (parameters, mesh, texture count) and the vertex positions after evaluating at `P` = 0, 0.5 and 1. Another implementation is conforming for evaluation when its results equal these values. The test `the_specification_sample_is_current` in `tsukumoe-cli` regenerates both files and fails when they drift (`TSUKUMOE_UPDATE_SAMPLES=1` rewrites them). The same file is produced by `tsukumoe convert <project> quad.tmod`, and `tsukumoe inspect quad.tmod` prints its summary and section table.

## 3. Versioning and feature flags

Both formats carry a major version and a set of feature flags.

- A reader rejects a file whose major version is newer than it supports.
- Each feature flag is either **optional** (a reader that does not know it may ignore it) or **required** (a reader that does not know it must reject the file).
- `.tsukumoe`: the manifest lists named features, each with a `required` boolean (section 1.1).
- `.tmod`: the header has two bit sets, required and optional (section 2.1), and every section table entry has its own required bit (section 2.2). A reader rejects the file for an unknown required header bit or an unknown section marked required; it ignores unknown optional header bits and skips unknown sections that are not marked required. A major version newer than the reader's is rejected before anything else is interpreted.
