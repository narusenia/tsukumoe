# 性能の測り方

M3-07 の測定手順。目標と合否は `docs/implementation/M3-runtime/performance.md`、結果は同ファイルの「結果」節に日付つきで記録する。

## 基準環境

- 合否の判定は開発者の手元（Apple M5、macOS、Chrome 安定版）。他の環境の数値は参考値
- CI（ubuntu）のベンチは絶対値ではなく、main との比較による回帰検知（PERF-009）に使う
- 基準モデルは `assets/reference-model/parts.json` から生成する（出力は決定的）。ベンチは自分で生成するので、手で作る必要があるのは PERF-003 のブラウザ計測だけ

```sh
cargo run --release -p tsukumoe-cli -- generate assets/reference-model/parts.json <出力ディレクトリ>
cargo run --release -p tsukumoe-cli -- convert <出力ディレクトリ> <出力>.tmod
```

## 一括実行

```sh
mise run bench
```

`cargo bench --workspace` のあと `scripts/check-bench.sh` が、各ベンチの中央値を `scripts/bench-budget.json` の天井と比べる。天井を超えると失敗する。結果は `target/bench-results.json` にも出る。

- GPU のベンチ（`gpu_*`、`frame_*`）はハードウェアの adapter が要る。無ければ結果なしで飛ばされる。並列版（`*_parallel*`）はコア数に依存するので、天井は基準環境（10 コア）の値。コアの少ない機械で超えるときは、`BENCH_CEILING_SCALE` を使う。CI の ubuntu ランナーは GPU が無く、これらの結果が出ないので、並列版は CI の天井と回帰比較の対象にならない
- `BENCH_BASELINE=<結果ファイル>` を付けると、そのファイルの中央値より 10% を超えて遅い項目で失敗する（`BENCH_THRESHOLD_PERCENT` で変更）
- `BENCH_CEILING_SCALE=<倍率>` は天井を倍率で緩める。天井は基準環境の値なので、CI は 4 倍にして、劣化は main との比較で見る

## 項目ごと

| REQ | 実行 | ベンチ名 / 見るもの |
|---|---|---|
| PERF-002 | `cargo bench -p tsukumoe-cli --bench reference` | `reference_evaluate`（目標 0.5 ms） |
| PERF-004 | `cargo bench -p tsukumoe-cli --bench gpu` | `PERF-004` の行、`gpu_reference_render_1080p`（目標 1 ms） |
| PERF-005 | `cargo bench -p tsukumoe-cli --bench load` | `reference_load`（ファイル読み込み + 検証。目標 20 ms）。`reference_parse` は検証だけ |
| PERF-006 | `cargo bench -p tsukumoe-cli --bench gpu` | `PERF-006` の行、`frame_reference_x50_1080p` と `_p99`（目標 16.6 ms）。評価だけの `reference_advance_x50` も `reference` にある |
| PERF-003 | 下記 | 検証ページの `bench` の `update_ms_median`（目標 2 ms） |

### PERF-004 の読み方

- Metal のタイムスタンプクエリは使えない。wgpu の `TIMESTAMP_QUERY` は有効になるが、同じパスの中でも終了の値が開始より前になる（200 回中 189 回）。代わりに、submit の後に GPU の完了を待った時間（`Headless::wait`）を測る
- この時間は提出の遅れを含むので、GPU 時間の上限。上限が目標以下なら合格とする
- 他の環境（Vulkan、D3D12）ではタイムスタンプが使える可能性がある。使う場合は `DrawOptions` に書き込み先を足す

### PERF-006 の読み方

- 1 フレーム = 50 体それぞれの `advance`（ドライバ・物理を含む）→ 描画の提出を体ごとに続け、最後に GPU の完了を待つまでの壁時計。300 フレームの中央値と 99 パーセンタイル（最初の 30 は捨てる）
- 50 体は 10x5 のセルに並べて 1920x1080 に描く。体ごとに位相を変え、全パラメータを毎フレーム動かす
- 内訳（評価、最後の GPU 待ち）も行に出る
- ベンチは 2 通りを測る。**合否は並列版**: 体ごとの評価を `min(コア数, 8)` スレッドに分け（`std::thread::scope`）、描画と提出は 1 スレッドのまま、`frame_reference_x50_1080p_parallel`（と `_p99`）。単一スレッド版（`frame_reference_x50_1080p`。体ごとに評価して、すぐ提出する）は参考値
- 合格は「体ごとの評価を並列に行う前提（スレッド数・コア数を結果に書く）で、中央値と p99 が 16.6 ms 以下」。結果にスレッド数とコア数（`PERF-006` の行に出る）を必ず添える
- ブラウザ（WASM）は並列にできないので、この判定の対象外。ネイティブの 1 スレッドが苦しい構成は、将来の改善候補（複数の描画リストを 1 パス・1 回の提出にまとめる API、評価の SIMD 化）で扱う

### PERF-003（ブラウザ）

1. `mise run web:build`（または `scripts/build-web.sh`）で `packages/tsukumoe-web/dist` を作る
2. 作業用ディレクトリに、`dist`（`packages/tsukumoe-web/dist` へのリンクで可）、`packages/tsukumoe-web/examples/index.html`、基準モデルの `ref.tmod` を置き、静的サーバで配る（`python3 -m http.server --bind 127.0.0.1`）
3. Chrome で `http://127.0.0.1:<port>/index.html?model=ref&bench=1` を開く。ページが 15 回 × 300 回の `update`（全パラメータを毎回動かす）を測り、`window.__result.bench` に出す
4. `update_ms_median` が結果。`move_only_ms_median`（パラメータの設定だけ）は引いてある。`&simd=0` で SIMD 無し、`&backend=webgl2` で描画経路を選べる
5. 終わったらサーバを止め、タブを閉じる

タイマ分解能は 0.1 ms なので、1 回ごとではなく 300 回の平均を取る。タブが背景だと遅れるので、前面で測る。

## 回帰検知（PERF-009）

- main への push で、CI が `target/bench-results.json` を Actions のキャッシュ（キー `bench-baseline-<sha>`）に保存する
- pull request では、最新の `bench-baseline-*` を復元して `BENCH_BASELINE` に渡し、10% を超える劣化で失敗させる
- キャッシュが無い（初回、7 日アクセス無しで消えた）ときは比較を飛ばして天井だけを見る
- 共有ランナーのゆらぎで誤検知する場合は、`BENCH_THRESHOLD_PERCENT` を見直すか、criterion の標本数を増やす
- 手元で確かめるには、結果を別名で保存し、ベンチに遅延を入れて `BENCH_BASELINE` 付きで `scripts/check-bench.sh` を走らせる
