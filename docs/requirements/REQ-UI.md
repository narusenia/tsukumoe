# UI 要件定義（UI 基盤）

対象クレート: `tsukumoe-app` / `tsukumoe-host`

## REQ-UI-001: egui を UI の土台に採用

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-001 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

UI は egui（eframe + egui-wgpu）で作る。独自の fork は持たない。
上流に足りない機能は、まず上流への PR で解決する。
経緯は ADR-0027（ADR-0003・0025・0026 を置き換え）。

### 受入条件

- [ ] `Cargo.toml` が crates.io / 上流 git の egui 系だけを UI の土台として参照している
- [ ] fork が必要になった場合は、ADR で理由と解消条件を記録している

### 依存要件

- なし

## REQ-UI-002: egui 系と wgpu の同一版固定

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-002 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

`egui` / `eframe` を対応する版に固定し、1 回の変更で同時に更新する。
`wgpu` は egui-wgpu が依存する版と同じにし、エディタのビューポートを UI と同じ device で描く。

### 受入条件

- [ ] `cargo tree -d` で egui 系・`wgpu` の重複がないことを CI で検査する
- [ ] ビューポートが UI と同じ wgpu device を使って描画している
- [ ] 上流追従の手順が `docs/dev/` に書かれている

### 依存要件

- REQ-UI-001
- REQ-RT-005

## REQ-UI-003: ランタイム系クレートの UI 非依存

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-003 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

`tsukumoe-core` / `-format` / `-gpu` / `-compat` / `-runtime` / `-ffi` / `-wasm` / `-tracking` / `-ai` / `-cli` は egui 系に依存しない。UI の更新がランタイムに波及しないようにする。

### 受入条件

- [ ] 上記クレートの依存ツリーに egui 系が現れないことを CI で検査する

### 依存要件

- REQ-INFRA-001

## REQ-UI-004: UI 部品とテーマを自前で持つ

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-004 |
| 優先度 | Should |
| ステータス | Draft |

### 説明

ウィジェットは egui の上に `tsukumoe-ui` として自前で作り、見た目を Tsukumoe のテーマ（ライト / ダーク / システム、JSON で読み込み）で統一する（ADR-0027）。

### 受入条件

- [ ] テーマ（色・余白・フォント）がトークンとして一か所で定義されている
- [ ] 各部品の状態を並べる gallery がある
- [ ] テーマをライト / ダーク / システムで切り替えられ、JSON から読み込める

### 依存要件

- REQ-UI-001
