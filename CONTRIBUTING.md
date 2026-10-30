# Contributing to Tsukumoe

> 日本語版: [`CONTRIBUTING_ja.md`](CONTRIBUTING_ja.md)

This page is a map. The rules live next to the things they govern.

## Where the rules are

| Read this | For |
|---|---|
| [`AGENTS.md`](AGENTS.md) | The canonical repository guide: layout, dependency rules, the moc3 clean-room rule |
| [`docs/specifications/architecture.md`](docs/specifications/architecture.md) | Crate layout and dependency direction |
| [`docs/implementation/roadmap.md`](docs/implementation/roadmap.md) | What is planned, milestone by milestone |
| [`docs/adr/`](docs/adr) | Why each decision was made |

`CLAUDE.md` is a one-line import of `AGENTS.md`.

## Getting set up

Tools are pinned with [mise](https://mise.jdx.dev/):

```bash
mise trust               # once per clone
mise install             # Rust, blaze, lefthook, jq
mise run hooks:install   # optional: pre-commit fmt / clippy, commit message check
mise run check           # fmt + clippy -D warnings + dependency rules + tests
```

`mise run check` is what CI runs.

## Making a change

- **Branch names**: a semantic prefix plus a concrete kebab-case description (`feat/dag-topological-sort`, not `feat/step2`).
- **Commits**: one logical concept each, one English line, Conventional Commit prefix (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `perf:`, `ci:`), lowercase after the prefix, no issue numbers.
- **Tests**: a bug fix carries a regression test that fails when the fix is reverted.
- **Dependencies**: ask before adding a production dependency. Dependency licenses are checked with `cargo deny`; GPL-family licenses are rejected.
- **moc3**: read the clean-room rule in `AGENTS.md` before touching anything related to `.moc3`.

## Sign-off (DCO)

Contributions are accepted under the [Developer Certificate of Origin](https://developercertificate.org/). Sign each commit with `git commit -s`, which adds a `Signed-off-by:` trailer. There is no CLA.

## License

By contributing you agree that your contribution is licensed under Apache-2.0 OR MIT, the same as the project.
