# ADR-0015: クレート構成

- 状態: 採用
- 日付: 2026-10-30

## 背景

ランタイムを先に作り、GPUI の変化をランタイムに波及させないための分割が要る。

## 決定

`tsukumoe-core` / `-format` / `-gpu` / `-compat` / `-runtime` / `-ffi` / `-wasm` / `-tracking` / `-ai` / `-cli`（GPUI 非依存）と、`tsukumoe-host` / `tsukumoe-app`（GPUI 依存）。詳細は `docs/specifications/architecture.md`。

- UI 部品は Ravel のように最初から `-ui` / `-widgets` / `-dock` に分けず、`tsukumoe-app` から始めて必要になったら分ける
- Unity パッケージは別リポジトリ
- 各クレートは必要になったマイルストーンで追加する
- 追記（M5-03）: `tsukumoe-psd`（PSD リーダと取り込み計画、GPUI / egui 非依存）を M5 で追加した。構成の方針（UI 境界より下は UI 非依存、必要になった M で追加）の範囲内なので、新しい ADR は起こさず、クレート表（architecture.md）への追記で記録する

## 結果

- 依存の向きと GPUI 非依存を CI で検査する
