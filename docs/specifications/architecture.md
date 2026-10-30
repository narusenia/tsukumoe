# アーキテクチャ

関連: REQ-INFRA-001, REQ-UI-001〜003, REQ-CORE-001, REQ-RT-005〜006

## 概要

Tsukumoe は「GPU も UI も知らない評価コア」を中心に置き、その出力（描画リスト）を各環境のレンダラが描く。
エディタとホストだけが UI フレームワーク（egui）に依存する。

```
             tsukumoe-app (editor)     tsukumoe-host (streaming)        [UI]
                    │                       │
                    └───── tsukumoe-ui ─────┘
                    │                       │
   ─ ─ ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  UI 境界
                    ▼                       ▼
tsukumoe-cli ──► tsukumoe-ai     tsukumoe-tracking
    │              │               │
    ▼              ▼               ▼
tsukumoe-compat ──► tsukumoe-format ──► tsukumoe-core ◄── tsukumoe-runtime ◄── tsukumoe-ffi ──► (Unity package, 別 repo)
                                       ▲                 ▲
                                       │                 └── tsukumoe-wasm ──► npm
                                   tsukumoe-gpu (wgpu)
```

## クレート

| クレート | 役割 | UI | 追加する M |
|---|---|---|---|
| `tsukumoe-core` | データモデル・評価パイプライン・物理・パラメータ式・編集コマンド / undo | なし | M1 |
| `tsukumoe-format` | JSON プロジェクト形式・ランタイムバイナリ形式・JSON Schema 生成 | なし | M1 / M3 |
| `tsukumoe-gpu` | wgpu 参照レンダラ（描画リスト → 画像） | なし | M1 |
| `tsukumoe-compat` | moc3（`moc3` feature）・Live2D JSON 群・Inochi2D の変換 | なし | M2 / M6 |
| `tsukumoe-runtime` | 再生 API（ロード・パラメータ・モーション・表情・物理・自動動作） | なし | M3 |
| `tsukumoe-ffi` | C ABI（cbindgen） | なし | M3 |
| `tsukumoe-wasm` | wasm-bindgen + TS 型 | なし | M3 |
| `tsukumoe-tracking` | 入力アダプタ・標準パラメータマッピング | なし | M4 |
| `tsukumoe-ai` | 推論プロバイダ抽象・リップシンク・リグ支援 | なし | M4 / M6 |
| `tsukumoe-cli` | 変換・レンダ・検証・MCP サーバ | なし | M1〜 |
| `tsukumoe-ui` | デザイントークン・テーマ・UI 部品（egui の上）、gallery | あり | M1 |
| `tsukumoe-host` | 透過ウィンドウ・Spout2 / Syphon・仮想カメラ | あり | M1（ビューア）→ M4 |
| `tsukumoe-app` | エディタ | あり | M5 |

## 依存規則

- 下位のクレートは上位のクレートに依存しない。`tsukumoe-core` は workspace 内のどのクレートにも依存しない
- UI 境界より下のクレートは egui 系（`egui` / `eframe` / `egui-wgpu` / `epaint` など）に依存しない（CI で `cargo tree` 検査）
- `tsukumoe-core` は `wgpu` にも依存しない。`wasm32-unknown-unknown` でビルドできる状態を保つ
- `tsukumoe-runtime` は編集機能（コマンド・undo）を含まない。core の評価部分を feature で切り出して使う
- moc3 関連コードは `tsukumoe-compat` の `moc3` feature に閉じる（REQ-COMPAT-003）

## 評価パイプライン

```
params ─► drivers ─► physics ─► keyform interp ─► skinning ─► deformer hierarchy ─► draw list
         (式)        (pendulum /  (多重線形 /       (bones)     (warp / rotation /
                      chain)       blend shape)                  glue)
```

- 編集用モデル（木構造・名前付き）と評価用モデル（SoA・インデックス参照・事前計算済み）を分ける。編集用 → 評価用は「コンパイル」で、ランタイム形式は評価用モデルをそのまま直列化したもの
- 定常状態のフレーム評価でヒープ確保をしない（REQ-CORE-008）
- 物理は固定タイムステップで、残り時間を次フレームに持ち越す
- 物理の入出力はパラメータだけなので、物理はドライバの直後に評価し、出力は同じフレームのキーフォーム補間に届く（再評価は要らない、[ADR-0022](../adr/0022-physics-stage.md)）

## GPU と UI

- UI: egui（eframe + egui-wgpu、[ADR-0027](../adr/0027-ui-egui.md)）。デザイントークン・テーマ・部品は `tsukumoe-ui` に置く
- `wgpu` は egui-wgpu と同じ版（30）。ビューポートは egui-wgpu の device で `tsukumoe-gpu` が描く（`Renderer::render_to_view`）
- Unity は `tsukumoe-ffi` で評価し、Unity のシェーダで描く。描画規則は `rendering.md`

## 拡張点

| 拡張点 | 方式 |
|---|---|
| レンダラ | 描画リストを入力とする外部実装（`rendering.md` 準拠） |
| トラッキング入力 | `tsukumoe-tracking` のアダプタ trait |
| 推論 | `tsukumoe-ai` のプロバイダ trait（ローカル ONNX が既定） |
| 互換形式 | `tsukumoe-compat` の feature ごとの importer / exporter |

## 制約・前提

- Rust edition 2024、`[workspace.dependencies]` で依存を一元管理
- 対応 OS は REQ-INFRA-004
