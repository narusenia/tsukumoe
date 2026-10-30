# Tsukumoe File Formats

Related: REQ-FMT-001 to REQ-FMT-006

License of this document: CC-BY 4.0

Status: the manifest, the containers, the model files and the published JSON Schema are defined (M1-08); the runtime format is M3.

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

- Elements refer to each other by **persistent name**, never by an internal index. The one exception is a keyform grid's `binding`: the position in `bindings`, because bindings are shared and unnamed.
- Files under `motions/` and `expressions/` are numbered in the order of the model's table so the order survives; the name part is the real name with every byte outside `A-Z a-z 0-9 - _` written as `%XX`. A reader takes the name from inside the file.
- Fields with a natural default (opacity 1, draw order 500, colors, `visible`, empty lists) may be left out when a file is edited by hand; a writer always writes them.
- Reading validates the content as a model: a name that is not defined, a duplicate name, or a geometry that does not fit is an error that names the element.
- Numbers must be finite; JSON has no NaN or infinity.

## 2. Runtime format (`.tmod`)

The distribution format. Designed for zero-copy loading: after validation, the evaluator reads directly from the loaded buffer.

- Byte order: little-endian.
- Alignment: every section starts on a 16-byte boundary.
- Layout: header, section table, sections (evaluation data in SoA form, string table, optional embedded textures).
- Untrusted input: a loader must validate all offsets, lengths and indices before use.

TODO (M3): header layout, section IDs, per-section layouts.

## 3. Versioning and feature flags

Both formats carry a major version and a set of feature flags.

- A reader rejects a file whose major version is newer than it supports.
- Each feature flag is either **optional** (a reader that does not know it may ignore it) or **required** (a reader that does not know it must reject the file).
