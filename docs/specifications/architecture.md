# アーキテクチャ

関連: REQ-INFRA-001, REQ-UI-001〜003, REQ-CORE-001, REQ-RT-005〜006

## 概要

Seimei は「GPU も UI も知らない評価コア」を中心に置き、その出力（描画リスト）を各環境のレンダラが描く。
エディタとホストだけが GPUI に依存する。

```
             seimei-app (editor)     seimei-host (streaming)        [GPUI]
                    │                       │
   ─ ─ ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  GPUI 境界
                    ▼                       ▼
seimei-cli ──► seimei-ai     seimei-tracking
    │              │               │
    ▼              ▼               ▼
seimei-compat ──► seimei-format ──► seimei-core ◄── seimei-runtime ◄── seimei-ffi ──► (Unity package, 別 repo)
                                       ▲                 ▲
                                       │                 └── seimei-wasm ──► npm
                                   seimei-gpu (wgpu)
```

## クレート

| クレート | 役割 | GPUI | 追加する M |
|---|---|---|---|
| `seimei-core` | データモデル・評価パイプライン・物理・パラメータ式・編集コマンド / undo | なし | M1 |
| `seimei-format` | JSON プロジェクト形式・ランタイムバイナリ形式・JSON Schema 生成 | なし | M1 / M3 |
| `seimei-gpu` | wgpu 参照レンダラ（描画リスト → 画像） | なし | M1 |
| `seimei-compat` | moc3（`moc3` feature）・Live2D JSON 群・Inochi2D の変換 | なし | M2 / M6 |
| `seimei-runtime` | 再生 API（ロード・パラメータ・モーション・表情・物理・自動動作） | なし | M3 |
| `seimei-ffi` | C ABI（cbindgen） | なし | M3 |
| `seimei-wasm` | wasm-bindgen + TS 型 | なし | M3 |
| `seimei-tracking` | 入力アダプタ・標準パラメータマッピング | なし | M4 |
| `seimei-ai` | 推論プロバイダ抽象・リップシンク・リグ支援 | なし | M4 / M6 |
| `seimei-cli` | 変換・レンダ・検証・MCP サーバ | なし | M1〜 |
| `seimei-host` | 透過ウィンドウ・Spout2 / Syphon・仮想カメラ | あり | M1（ビューア）→ M4 |
| `seimei-app` | エディタ。UI 部品も最初はここに置く | あり | M5 |

## 依存規則

- 下位のクレートは上位のクレートに依存しない。`seimei-core` は workspace 内のどのクレートにも依存しない
- GPUI 境界より下のクレートは `gpui` 系に依存しない（CI で `cargo tree` 検査）
- `seimei-core` は `wgpu` にも依存しない。`wasm32-unknown-unknown` でビルドできる状態を保つ
- `seimei-runtime` は編集機能（コマンド・undo）を含まない。core の評価部分を feature で切り出して使う
- moc3 関連コードは `seimei-compat` の `moc3` feature に閉じる（REQ-COMPAT-003）

## 評価パイプライン

```
params ─► drivers ─► keyform interp ─► skinning ─► deformer hierarchy ─► physics ─► draw list
         (式)        (多重線形 /         (bones)     (warp / rotation /    (pendulum /
                      blend shape)                    glue)                 chain)
```

- 編集用モデル（木構造・名前付き）と評価用モデル（SoA・インデックス参照・事前計算済み）を分ける。編集用 → 評価用は「コンパイル」で、ランタイム形式は評価用モデルをそのまま直列化したもの
- 定常状態のフレーム評価でヒープ確保をしない（REQ-CORE-008）
- 物理は固定タイムステップで、残り時間を次フレームに持ち越す
- 物理の出力パラメータは次段の入力に戻すため、物理段のあとに「物理が書いたパラメータに依存する段」を再評価する。範囲はコンパイル時に求める
  - TODO: Cubism の評価順と一致させる方法を M1 で確定する

## GPU と UI

- UI: `gpui-pre` + `gpui-kit`（fork なし、ADR-0003）
- `gpui-pre` / 兄弟クレート / `gpui-kit` は同じ Zed リビジョンに対応する版に固定し、同時に更新する
- `wgpu` は `gpui-pre-wgpu` と同じ版。ビューポートは GPUI から device を受け取り `seimei-gpu` で描く
- Unity は `seimei-ffi` で評価し、Unity のシェーダで描く。描画規則は `rendering.md`

## 拡張点

| 拡張点 | 方式 |
|---|---|
| レンダラ | 描画リストを入力とする外部実装（`rendering.md` 準拠） |
| トラッキング入力 | `seimei-tracking` のアダプタ trait |
| 推論 | `seimei-ai` のプロバイダ trait（ローカル ONNX が既定） |
| 互換形式 | `seimei-compat` の feature ごとの importer / exporter |

## 制約・前提

- Rust edition 2024、`[workspace.dependencies]` で依存を一元管理
- 対応 OS は REQ-INFRA-004
