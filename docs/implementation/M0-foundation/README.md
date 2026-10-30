# M0: リポジトリ基盤

完了条件: 3 OS の CI が通る。

計画ファイルは着手時にこのディレクトリへ作る（1 本 = 1 ファイル、冒頭に REQ-ID・状態・依存）。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M0-01 | `AGENTS.md` / `CLAUDE.md` / README（英日）/ CONTRIBUTING（DCO）/ ライセンスファイル | INFRA-003, INFRA-006 | S | — | 未着手 |
| M0-02 | Cargo workspace と `seimei-core` の空クレート、`[workspace.dependencies]` | INFRA-001 | S | — | 未着手 |
| M0-03 | mise（Rust 1.99）と lefthook（fmt / clippy / コミットメッセージ） | INFRA-007 | S | M0-02 | 未着手 |
| M0-04 | GitHub Actions: 3 OS の build / test / clippy / fmt、wasm32 ビルド | INFRA-002 | M | M0-02 | 未着手 |
| M0-05 | `cargo deny`（ライセンス）と依存規則検査（GPUI 非依存・依存の向き） | INFRA-003, UI-003 | S | M0-04 | 未着手 |
| M0-06 | 基準モデルの仕様決め（パーツ構成・パラメータ一覧）と制作開始 | PERF-001 | L | — | 未着手 |
