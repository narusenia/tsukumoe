# M0: リポジトリ基盤

完了条件: 3 OS の CI が通る。

計画ファイルは着手時にこのディレクトリへ作る（1 本 = 1 ファイル、冒頭に REQ-ID・状態・依存）。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M0-01 | `AGENTS.md` / `CLAUDE.md` / README（英日）/ CONTRIBUTING（DCO）/ ライセンスファイル | INFRA-003, INFRA-006 | S | — | 完了 |
| M0-02 | Cargo workspace と `tsukumoe-core` の空クレート、`[workspace.dependencies]` | INFRA-001 | S | — | 完了 |
| M0-03 | mise（Rust 1.99）と lefthook（fmt / clippy / コミットメッセージ） | INFRA-007 | S | M0-02 | 完了（`mise run check` 通過。hooks は未インストール） |
| M0-04 | GitHub Actions: 3 OS の build / test / clippy / fmt、wasm32 ビルド | INFRA-002 | M | M0-02 | 作成済み・未検証（リモートがなく CI を回していない） |
| M0-05 | `cargo deny`（ライセンス）と依存規則検査（GPUI 非依存・依存の向き） | INFRA-003, UI-003 | S | M0-04 | 完了（`deps` 検査はローカルで通過。`cargo deny` は未実行） |
| M0-06 | 基準モデルの仕様決め（パーツ構成・パラメータ一覧）と制作開始 | PERF-001 | L | — | 仕様のみ完了（[reference-model.md](reference-model.md)）。制作は未着手 |
