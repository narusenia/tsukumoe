# ADR-0004: v1 ランタイムの対象は C ABI / WASM / Unity

- 状態: 採用
- 日付: 2026-10-30

## 背景

Live2D の主な利用先は Unity（ゲーム・VTuber アプリ）と Web である。対象環境が core の設計（GPU 依存・ABI・メモリ）を決める。

## 決定

- v1: C ABI、WASM（WebGPU、WebGL2 フォールバック）、Unity
- Godot / Unreal / モバイル専用対応は v1.x
- core は GPU 非依存で、描画リストまでを出す。レンダラは差し替え式
- `tsukumoe-gpu`（wgpu）は参照実装。Unity は Unity のシェーダで描き、wgpu を持ち込まない
- Unity パッケージは別リポジトリ（UPM 配布）

## 結果

- 描画規則を仕様（`docs/specifications/rendering.md`）として定め、レンダラ間の一致を検証する必要がある
