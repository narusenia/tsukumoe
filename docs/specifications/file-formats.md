# Tsukumoe File Formats

Related: REQ-FMT-001 to REQ-FMT-006

License of this document: CC-BY 4.0

Status: outline. Field-level definitions are filled in during M1 (project format) and M3 (runtime format).

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
- Schema: JSON Schema generated from the Rust types and published under `schema/`.

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
