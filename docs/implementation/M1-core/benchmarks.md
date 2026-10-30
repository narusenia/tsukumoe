# ベンチマークと回帰検知

- REQ: REQ-PERF-002, REQ-PERF-009
- 単位: M1-12
- 状態: 完了
- 依存: M1-03
- クレート: `tsukumoe-core`（`benches/evaluate.rs`、`criterion` は dev-dependency）

## 方針

- 基準モデル（M1-13）はまだない。代わりに基準モデル規模の合成モデルを測る: 100 メッシュ × 196 頂点（約 2 万頂点）、ワープ 1 つの子、各メッシュは 2 パラメータの 2×2 キーフォーム
- 計測: `compile`、`evaluate`（毎フレーム両パラメータを動かす）、`evaluate_draw_list`、`advance_16ms`
- 回帰検知: 中央値の上限（ns）を `benches/budget.json` に置き、`scripts/check-bench.sh` が比較する。CI は `bench` ジョブ（ubuntu）で `mise run bench`
- 上限は、CI ランナー（ubuntu）の実測（evaluate 約 0.38 ms、compile 約 0.17 ms）の約 2 倍。手元（Apple Silicon）は約 5 倍速い。検知できるのは 2 倍近い劣化から

## 受入条件

- [x] `mise run bench` が計測し、上限超過で失敗する
- [x] 合成モデルでパラメータが頂点を動かすことをベンチの開始時に検算する
- [x] CI の `bench` ジョブが走る
- [ ] 基準モデルでの 0.5 ms 以下の確認（REQ-PERF-002 の受入条件）は M1-13 の後

## 残した制約

- 合成モデルの評価は 約 0.07 ms（手元）。物理・ボーン・ドライバ・グルーは含まない
- ウォームアップ後の中央値のみ。ばらつきの大きい CI では、上限を超えたら再実行して確かめる
