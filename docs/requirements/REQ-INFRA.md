# INFRA 要件定義（基盤・開発体制）

## REQ-INFRA-001: Cargo workspace とクレート構成

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-001 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

Cargo workspace で、クレートを `docs/specifications/architecture.md` の構成で作る。各クレートは必要になったマイルストーンで追加する。Unity パッケージは別リポジトリにする。

### 受入条件

- [ ] 依存の向きが architecture.md の図と一致する（下位から上位への依存がない）
- [ ] 外部依存のバージョンは `[workspace.dependencies]` で一元管理する
- [ ] edition 2024

### 依存要件

- なし

## REQ-INFRA-002: 3 OS の CI

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-002 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

macOS / Windows / Linux でビルドとテストを行う。画像差分テストはヘッドレス（ソフトウェアアダプタ / lavapipe）で実行する。

### 受入条件

- [ ] 3 OS で `cargo build` / `cargo test` / `clippy -D warnings` / `fmt --check` が通る
- [ ] `wasm32-unknown-unknown` のビルドが通る
- [ ] 画像差分テストが Linux CI で実行される

### 依存要件

- REQ-INFRA-001

## REQ-INFRA-003: ライセンスと DCO

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-003 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

コードは Apache-2.0 OR MIT。形式仕様・JSON Schema・基準モデル・サンプルは CC-BY 4.0。AI モデルの重みはそれぞれのライセンスを明記し、同梱しない。貢献は DCO（Signed-off-by）で受け、CLA は求めない。
OSS 本体に有料前提の機能制限を入れない。

### 受入条件

- [ ] `LICENSE-APACHE` / `LICENSE-MIT` がある
- [ ] 依存クレートのライセンスを `cargo deny` で検査する（GPL 系を拒否）
- [ ] CONTRIBUTING に DCO の説明がある

### 依存要件

- なし

## REQ-INFRA-004: 対応プラットフォームの優先度

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-004 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

| OS | エディタ | ホスト |
|----|---------|-------|
| macOS（Metal） | 主対象 | 主対象（Syphon） |
| Windows（D3D12 / Vulkan） | 主対象 | 主対象（Spout2・仮想カメラ） |
| Linux（Vulkan） | ベストエフォート | ベストエフォート |
| Web | 対象外 | ビューアのみ |

Linux は CI でのビルドとテストだけを保証する。

### 受入条件

- [ ] 主対象 OS でのリリース前手動確認の手順がある

### 依存要件

- REQ-INFRA-002

## REQ-INFRA-005: ドキュメント構成と状態管理

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-005 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

ドキュメントを次の構成で管理する（経緯は ADR-0014）。

- `docs/requirements/` — REQ-{SCOPE}-{SEQ}
- `docs/specifications/` — アーキテクチャ・形式仕様
- `docs/implementation/roadmap.md` — マイルストーンと完了条件だけ
- `docs/implementation/M{n}-{name}/` — マイルストーンごとの計画。状態はその `README.md` の表だけで管理する
- `docs/implementation/backlog.md` — 予定の決まっていない項目だけ
- `docs/adr/NNNN-*.md` — 設計判断の記録
- `docs/handoff/` — セッション引き継ぎ（日付ファイル）
- `docs/dev/` — 開発者向け手順

### 受入条件

- [ ] 状態をファイルの移動（`done/` 等）で表していない
- [ ] 計画ファイル名は kebab-case で、`-plan` 等の接尾辞・日付を含まない
- [ ] リポジトリ直下に handoff ファイルがない
- [ ] backlog.md が 200 行程度に収まっている

### 依存要件

- なし

## REQ-INFRA-006: エージェント向け規約と言語の使い分け

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-006 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

主開発者と AI エージェント（Claude Code / Codex）での開発を前提とする。`AGENTS.md` を正本とし、`CLAUDE.md` は `@AGENTS.md` だけを書く。
言語: 要件・計画・ADR は日本語。コード・コメント・コミット・形式仕様・README は英語（README_ja を併設）。

### 受入条件

- [ ] AGENTS.md に moc3 のクリーンルーム規則（REQ-COMPAT-002）が書かれている
- [ ] AGENTS.md にクレート間の依存規則が書かれている

### 依存要件

- REQ-COMPAT-002

## REQ-INFRA-007: 開発ツールチェーン

| 項目 | 内容 |
|------|------|
| ID | REQ-INFRA-007 |
| 優先度 | Should |
| ステータス | Draft |

### 説明

ツールのバージョンを mise で固定し、lefthook でコミット前検査（fmt / clippy / コミットメッセージ形式）を行う。
Rust は blaze を使うため 1.99 に固定する（`mise.toml`）。

### 受入条件

- [ ] `mise install` だけで開発環境が揃う
- [ ] Rust のバージョンが `mise.toml` の 1 か所でだけ指定されている
- [ ] lefthook の pre-commit で fmt と clippy が走る

### 依存要件

- なし
