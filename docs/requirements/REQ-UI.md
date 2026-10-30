# UI 要件定義（UI 基盤）

対象クレート: `tsukumoe-app` / `tsukumoe-host`

## REQ-UI-001: gpui-pre と gpui-kit を fork せず採用

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-001 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

UI は Zed の gpui のスナップショット公開である `gpui-pre` と、その上の `gpui-kit`（`gpui-base` / `gpui-component`）で作る。fork は持たない。
上流に足りない機能は、まず上流への PR で解決する。
経緯は ADR-0003。

### 受入条件

- [ ] `Cargo.toml` が crates.io / 上流 git の `gpui-pre` 系と `gpui-kit` 系だけを参照している
- [ ] fork が必要になった場合は、ADR で理由と解消条件を記録している

### 依存要件

- なし

## REQ-UI-002: GPUI 系と wgpu の同一リビジョン固定

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-002 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

`gpui-pre` と兄弟クレート（`-platform` / `-wgpu` 等）と `gpui-kit` を、同じ Zed リビジョンに対応する版に固定し、1 回の変更で同時に更新する。
`wgpu` は `gpui-pre-wgpu` が依存する版と同じにし、エディタのビューポートを UI と同じ device で描く。

### 受入条件

- [ ] `cargo tree -d` で `gpui` 系・`wgpu` の重複がないことを CI で検査する
- [ ] ビューポートが UI と同じ wgpu device を使って描画している
- [ ] 上流追従の手順が `docs/dev/` に書かれている

### 依存要件

- REQ-UI-001
- REQ-RT-005

## REQ-UI-003: ランタイム系クレートの GPUI 非依存

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-003 |
| 優先度 | Must |
| ステータス | Draft |

### 説明

`tsukumoe-core` / `-format` / `-gpu` / `-compat` / `-runtime` / `-ffi` / `-wasm` / `-tracking` / `-ai` / `-cli` は GPUI に依存しない。GPUI の更新がランタイムに波及しないようにする。

### 受入条件

- [ ] 上記クレートの依存ツリーに `gpui` 系が現れないことを CI で検査する

### 依存要件

- REQ-INFRA-001

## REQ-UI-004: UI 部品は gpui-base の上に自前実装

| 項目 | 内容 |
|------|------|
| ID | REQ-UI-004 |
| 優先度 | Should |
| ステータス | Draft |

### 説明

ウィジェットは `gpui-base`（スタイルを持たない振る舞い層）の上に自前で作り、見た目を Tsukumoe のテーマで統一する。初めは `tsukumoe-app` の中に置き、必要になったら `tsukumoe-ui` / `-widgets` / `-dock` に分ける。

### 受入条件

- [ ] テーマ（色・余白・フォント）がトークンとして一か所で定義されている

### 依存要件

- REQ-UI-001
