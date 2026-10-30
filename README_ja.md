# Tsukumoe

> English: [`README.md`](README.md)

Live2D Cubism の代替を目指す、オープンな 2D リグ・アニメーション基盤。高速なランタイム SDK とエディタを作る。

**状態: 開発初期（M1）。core（評価・物理・描画リスト）とプロジェクト形式（JSON Schema 付き）まで実装済み。wgpu 参照レンダラ（ヘッドレス）と CLI（`tsukumoe convert | validate | render | diff | generate`）も実装済み。アプリとしてはまだ使えない。**

## 目標

- **高速** — GPU に依存しないデータ指向の評価パイプライン。同じモデルで Cubism SDK の 2 倍以上の速度を目指す
- **AI ネイティブ** — エディタの全操作をシリアライズ可能なコマンドにし、MCP と CLI からエージェントに公開する。リグ支援とリップシンクを内蔵する
- **互換性** — 既存の Live2D モデル（`.moc3` とその周辺の JSON）と Inochi2D モデルを読み込める
- **オープン** — コードは Apache-2.0 OR MIT。ランタイム形式の仕様を公開する

## 対象（v1）

| 領域 | 対象 |
|---|---|
| ランタイム | C ABI、WASM（WebGPU / WebGL2）、Unity |
| VTuber | トラッキング入力（VMC / OSC、iFacialMocap、MediaPipe）、最小の配信ホスト |
| エディタ | macOS と Windows（Linux はベストエフォート） |

## ドキュメント

[`docs/requirements/overview.md`](docs/requirements/overview.md) から読み、次に [`docs/specifications/architecture.md`](docs/specifications/architecture.md) を読む。計画は [`docs/implementation/roadmap.md`](docs/implementation/roadmap.md)、各判断の理由は [`docs/adr/`](docs/adr) にある。

## ライセンス

コードは [Apache License 2.0](LICENSE-APACHE) または [MIT license](LICENSE-MIT) のどちらかを選べる。ファイル形式の仕様・スキーマ・サンプルモデルは CC-BY 4.0。
