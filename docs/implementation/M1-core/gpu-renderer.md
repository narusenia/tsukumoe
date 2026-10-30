# 描画規則と wgpu 参照レンダラ

- REQ: REQ-RT-005, REQ-RT-006
- 単位: M1-09
- 状態: 完了
- 依存: M1-07
- 設計判断: [ADR-0024](../../adr/0024-rendering-rules.md)
- 仕様: [rendering.md](../../specifications/rendering.md)
- クレート: `tsukumoe-gpu`（新規。`tsukumoe-core` に依存し、`tsukumoe-core` は wgpu に依存しない）

## 範囲

| 含める | 含めない |
|---|---|
| 描画規則の仕様（rendering.md）と ADR | Cubism との一致の検証（M2） |
| `Renderer`: 外部の `wgpu::Device` / `Queue` を受け取って描く | GPUI との結合（M1-11） |
| マスク（最大値合成・反転）、3 種のブレンド、乗算色・スクリーン色 | 入れ子のマスク、WASM（WebGPU）ビルド（M3） |
| ヘッドレス出力（ソフトウェアアダプタ優先）と PNG 書き出し | PNG テクスチャの読み込み（M1-10） |
| 規則ごとのテスト（全面を覆う矩形の画素値を式で検算） | 画像差分ツール（M1-10） |

## 実装ステップ

1. ADR-0024、rendering.md
2. `tsukumoe-gpu` クレート（依存: `wgpu` `bytemuck` `pollster` `png`、承認済み）
3. `Renderer`: テクスチャ登録（premultiplied 化）、描画パイプライン、マスクパス
4. ヘッドレス: アダプタ取得、`render_rgba8`、PNG
5. テスト。アダプタがない環境ではスキップする

## 受入条件

- [x] 乗算色・スクリーン色・不透明度・3 種のブレンドが rendering.md の式どおりの画素値になる
- [x] マスク（通常・反転・複数）が rendering.md どおりに効く
- [x] 描画順と両面・背面カリングが効く
- [x] 外部の device / queue を受け取って描ける
- [x] ヘッドレスで画像（PNG）を出力できる
- [x] `mise run check` が通る（`deps`: `tsukumoe-core` が wgpu に依存しない）
