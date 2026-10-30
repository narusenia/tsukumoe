# ロードマップ

順序は ADR-0011。エディタが途中で要るとわかったら入れ替える。
状態は各マイルストーンの `README.md` だけで管理する（ADR-0014）。

| M | 内容 | 完了条件 |
|---|---|---|
| [M0](M0-foundation/README.md) | リポジトリ基盤・基準モデルの制作開始 | 3 OS の CI が通る |
| [M1](M1-core/README.md) | core 評価パイプライン・JSON プロジェクト形式・wgpu 参照レンダラ・最小ビューア・CLI | 基準モデル（Seimei 形式）が表示され、パラメータで動く |
| [M2](M2-compat/README.md) | Live2D JSON 群と moc3 の読み込み（moc3 は法務ゲート後） | 自作の Live2D モデルが画像差分テストを通る |
| [M3](M3-runtime/README.md) | ランタイム形式・C ABI・WASM / Web・Unity・性能目標 | 3 環境で出力が一致し、PERF 目標を満たす |
| [M4](M4-vtuber/README.md) | トラッキング入力・標準マッピング・ホスト・リップシンク | 実際の配信で使える |
| [M5](M5-editor/README.md) | エディタ・MCP | 新しいモデルを Seimei だけで作れる |
| [M6](M6-release/README.md) | AI リグ支援・Inochi2D・v1 リリース | v1 リリース |
