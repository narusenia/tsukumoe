# ADR-0025: UI に wgpui を使う（ADR-0003 を置き換える）

- 状態: 採用
- 日付: 2026-10-30
- 置き換え: [ADR-0003](0003-ui-gpui-pre-and-gpui-kit.md)

## 背景

ADR-0003 は、ビューポートを UI と同じ wgpu device で描くために `gpui-pre-wgpu` と同じ wgpu 版を使う前提だった。M1-11 の調査で次が分かった。

- `gpui-pre-macos` は wgpu に依存せず、描画は Metal 直（`gpui_apple`）。macOS では GPUI から wgpu の device を取り出す口がない
- `gpui-pre-wgpu` は Linux / Windows 向けの描画層で、macOS では使われない
- 一方 `wgpui`（GPUI-CE 0.3.3 のコミュニティ fork）は wgpu + winit の共通経路で、`WgpuSurface`（UI ツリーに置ける二重バッファの wgpu テクスチャ）により全 OS で device を共有できる
- macOS 上で、`wgpui-kit` 0.6.1 + wgpu 30 の窓・スライダ・`WgpuSurface` 描画とポインタ入力を確認した

## 決定

- UI の土台は `wgpui` とその上の `wgpui-base`。`wgpui-kit` / `wgpui-component` は使わない（UI 部品は [ADR-0026](0026-ui-crate.md)）
- `wgpu` は `wgpui` と同じ版（現在 30）に固定し、`tsukumoe-gpu` も同じ版を使う
- ビューポートは `WgpuSurface` の device / queue で `tsukumoe-gpu` の `Renderer` により描く
- `wgpui` / `wgpui-base` は同時に更新する
- ランタイム系クレートは `wgpui` 系にも `gpui` 系にも依存しない

## 結果

- macOS を含む全 OS で、ビューポートが UI と同じ device で描ける
- 代わりに、`wgpui` は個人 fork（0.x）で Zed 本流に追随しない。保守が止まった場合は fork を持つか `gpui-pre` へ戻る。戻す条件は、`gpui-pre` の macOS 側が wgpu の device を公開したとき
- Linux / Windows での窓の動作は未検証（CI はビルドのみ）
- ADR-0003 の「fork を持たない」は、wgpui を上流とみなす限り保たれる
