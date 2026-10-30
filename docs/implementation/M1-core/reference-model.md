# 基準モデルの生成

- REQ: REQ-PERF-001（Tsukumoe 形式版）
- 単位: M1-13
- 状態: 完了
- 依存: M1-08, M1-10
- 設計判断: [ADR-0017](../../adr/0017-reference-model-generator.md)
- クレート: `tsukumoe-cli`（`tsukumoe generate`、生成器は `generate.rs`）

## 入力: パーツ表

`assets/reference-model/parts.json`（git 管理、差分が読める形）。グループごとの内訳:

| 項目 | 意味 |
|---|---|
| `meshes` | ArtMesh 数（1〜15）。`grid` × `grid` 頂点 |
| `warps` | ワープデフォーマ数（2 以上）。先頭が回転の子、残りは先頭の子。メッシュは先頭以外へ振り分け |
| `parameters` | パラメータ数（5 以上）。`0` がワープ、`1` が回転、`2` `3` が物理出力、残りがメッシュの 2 軸目 |
| `chains` | 物理チェーン数（2 以下）。入力は `0` |

- グループごとに回転デフォーマ 1 つ、パーツ 1 つ、グラデーションのテクスチャ 1 枚（`textures/NN.png`）
- 既定姿勢（全パラメータ 0）は恒等の形

## コマンド

`tsukumoe generate <parts.json> <out>`: 出力先は zip（`.zip` / `.tsukumoe`）かディレクトリ。規模を JSON で標準出力へ。出力は決定的

## 規模（現在の表）

ArtMesh 150 / 頂点 33,750 / パラメータ 100 / デフォーマ 80（回転 10 + ワープ 70）/ 物理 20 系統 — REQ-PERF-001 の規模を満たす

## 受入条件

- [x] 規模が REQ-PERF-001 を満たす（テストで検算）
- [x] 生成物が `validate` を通り、`render` で描ける
- [x] 生成が決定的（2 回の出力が一致）
- [x] 不正な表（範囲外・JSON 不正）はエラーになる
- [x] 基準モデルの評価ベンチマーク（`reference_evaluate`、`reference_advance_16ms`）と CI 上限

## 残した制約

- 生成物（約 18 MB の展開形）はリポジトリに入れない。表から再生成する。CC-BY 4.0 での同梱は、生成物を配布する段階（M2 以降）で決める
- ボーン・グルー・ブレンドシェイプ・ドライバ・モーションは含まない。機能網羅は必要になった時点で表に足す
- Live2D 形式版は M2（ADR-0016、自作モデルの書き出し）
- 評価時間 0.5 ms 以下の判定（REQ-PERF-002）は手元で 約 0.14 ms。CI 上限は別（`scripts/bench-budget.json`）
