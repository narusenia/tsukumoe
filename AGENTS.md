# Seimei — agent guide

Seimei is an open 2D rigging and animation platform, an alternative to Live2D Cubism: a runtime SDK (built first) and an editor.

## Where things are

- Requirements: `docs/requirements/` (`REQ-{SCOPE}-{SEQ}`; start at `overview.md`)
- Specifications: `docs/specifications/` (`architecture.md` first)
- Plans: `docs/implementation/roadmap.md`, then `docs/implementation/M{n}-*/README.md`
- Decisions: `docs/adr/`
- Session handoffs: `docs/handoff/` (never at the repository root)

## Rules

- **Crate dependencies** follow `docs/specifications/architecture.md`. Crates below the GPUI boundary must not depend on `gpui*`; `seimei-core` must not depend on `wgpu` and must build for `wasm32-unknown-unknown`.
- **moc3 clean room** (ADR-0016): do not read, search for, fetch or quote any existing third-party moc3 parser, its code or its documentation, and do not analyze Cubism SDK / Core binaries or other people's models. Only models the maintainer exported from Cubism Editor may be analyzed. Record every format finding with its source.
- **License header**: every `.rs` file starts with these two lines, followed by a blank line:
  ```rust
  // Copyright 2026 Seimei Contributors
  // SPDX-License-Identifier: Apache-2.0 OR MIT
  ```
- **Plan status** lives only in the table of each milestone `README.md`. Do not move files to express status.
- **Language**: requirements, plans and ADRs in Japanese; code, comments, commit messages, format specifications and README in English.
- **Docs consistency**: when finishing a piece of work, check whether it affects the requirements, specifications or plans under `docs/`, and if it does, ask the user whether to update them.
