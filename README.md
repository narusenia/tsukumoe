# Tsukumoe

> 日本語版: [`README_ja.md`](README_ja.md)

An open 2D rigging and animation platform: an alternative to Live2D Cubism, with a fast runtime SDK and an editor.

**Status: early development (M0, repository foundation). Nothing is usable yet.**

## Goals

- **Fast** — a GPU-independent, data-oriented evaluation pipeline. Target: at least 2x faster than the Cubism SDK on the same model.
- **AI native** — every editor operation is a serializable command, exposed to agents over MCP and CLI. Rigging assistance and lip sync are built in.
- **Compatible** — load existing Live2D models (`.moc3` and the JSON files around it) and Inochi2D models.
- **Open** — code under Apache-2.0 OR MIT. The runtime file format is publicly specified.

## Targets (v1)

| Area | Targets |
|---|---|
| Runtime | C ABI, WASM (WebGPU / WebGL2), Unity |
| VTuber | Tracking input (VMC / OSC, iFacialMocap, MediaPipe), a minimal streaming host |
| Editor | macOS and Windows (Linux best effort) |

## Documentation

Start with [`docs/requirements/overview.md`](docs/requirements/overview.md), then [`docs/specifications/architecture.md`](docs/specifications/architecture.md). The plan is in [`docs/implementation/roadmap.md`](docs/implementation/roadmap.md), and the reasons behind each decision are in [`docs/adr/`](docs/adr).

## License

Code is licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option. File format specifications, schemas and sample models are licensed under CC-BY 4.0.
