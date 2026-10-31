# 環境間の出力一致テスト

- REQ: REQ-RT-007
- 単位: M3-06
- 状態: 実装済み（CI 待ち。Unity の 1 ケースに端の画素差が残る）
- 依存: M3-04（WASM）、M3-05（Unity）

## 目的

同じ `.tmod`・同じ入力から、native・WASM・Unity が同じ描画リストと同じ画像を出すことをテストで保証する。基準は native（`tsukumoe-runtime` と `tsukumoe-gpu`）。許容誤差は `rendering.md` §5（画素は各チャンネル 2/255、描画リストの数値は絶対誤差 1e-4）。

## 決めたこと

- **比較は基準 1 つ対他の環境**（native 対 WASM、native 対 Unity）。環境どうしの総当たりはしない
- **各環境の出力を同じ形式のファイルにそろえる**: 画像は premultiplied RGBA8（ヘッダなし、上の行から）、描画リストは `TsukumoeCapture` と同じ JSON（`drawables` を奥から手前の順に並べ、メッシュごとの全数値を持つ）。形式は `docs/specifications/draw-list.md` に節を足して固定する
- **比較器は Rust の example 1 本**（`crates/tsukumoe-cli` の `parity <a> <b>`、または独立の example）。画像の最大差と 2/255 超の画素数、描画リストの最大差と、順序・マスク・ブレンド・フラグの完全一致を報告し、許容を超えたら非 0 で終了する
- **ケース**: fixture（マスク通常・反転、アルファ傾斜）、基準モデル、各 1 つ以上の時刻・パラメータ値（`P` を動かす、モーションの途中、表情）。入力は「`update` の呼び方」（dt の列）で表し、全環境で同じ列を使う
- **実行場所**: native 対 WASM の描画リストは CI（Node）。画像は Chrome が要るためローカル。Unity は CI に載せない（ライセンスと macOS・Windows の実機が要る）ので、**ローカルで走らせて golden を更新する**。CI は golden（native が出した画像と JSON）との比較だけを行い、Unity 側の tsukumoe-unity は同じ golden を `Tests/Fixtures/` に持って PlayMode で比較する（M3-05 で実装済みの方式を全ケースに広げる）

## 進め方

1. （完了）native の書き出し（画像と描画リスト JSON）を example で作り、ケース定義（`.tmod` と `update` の dt 列）を `tests/parity/` に置く。実装: `crates/tsukumoe-cli/examples/parity_export.rs`、モデルは `parity_fixture.rs`（パラメータ・モーション・表情・物理が動く fixture）と基準モデル、一括実行は `tests/parity/export.sh <out-dir>`
2. （完了）比較器を作る。実装: `tsukumoe parity <a> <b>`（`crates/tsukumoe-cli/src/parity.rs`）
3. （完了）WASM: Node で描画リスト JSON を出し、比較する。実装: `packages/tsukumoe-web/test/parity-export.ts`、実行は `mise run web:parity`（`tests/parity/native-vs-wasm.sh`）。native 対 WASM の描画リストは全 11 ケースで最大差 1.8e-7（許容 1e-4）。native の書き出しは、WASM の `Model` が読み込み時に `update(0)` を 1 回呼ぶのに合わせ、最初の step の前に `update(0)` を呼ぶ（物理はこの 1 回の有無で結果が変わる）。画像は Chrome の検証ページ（`tests/parity/parity.html`）から出し、ローカルサーバ（`tests/parity/web-server.mjs`、Node 標準のみ、127.0.0.1）が `<case>.rgba` に書く。`PARITY_IMAGES=1 tests/parity/native-vs-wasm.sh <out-dir>`（`mise run web:parity-images`）がサーバを起動し、Chrome でページを開くと比較まで進む。測定（Chrome、macOS）: 全 11 ケースで native 対 WASM の画素差は最大 0、2/255 超 0 画素。WebGPU（`auto` で選ばれた）と WebGL2（`backend=webgl2`）の両方で同じ
4. （完了）Unity: `TsukumoeCapture.RunCase` がケース JSON を再生し（読み込み時の `update(0)` は `TsukumoeModel` のコンストラクタが行う）、`scripts/parity.sh <out-dir>` が全ケースを Unity エディタ（batch、グラフィックス有り）で書き出す。golden は `scripts/import-golden.sh` で `tsukumoe-unity` の `Tests/Fixtures/parity/` に取り込み、PlayMode の `ParityTests`（11 ケース）が比較する。測定（Unity 6000.3.27f2、Metal、macOS arm64、Linear）: 描画リストの最大差は全ケース 0、順序・マスク・ブレンド・フラグは一致。画素差は最大 1/255 が 5 ケース（reference-*）、fixture 6 ケースは 0。例外は reference-physics-settle で、三角形の辺上にある 2 画素が Unity だけ塗られ（native は透明）、2/255 を超える。`ParityTests` は、このケースに限り 2 画素までを許す（`KnownEdgePixels`）
5. （実装済み、CI 待ち）CI に `parity` ジョブを足した（`web:build` → `mise run web:parity` → `mise run parity:golden`）。golden は `tests/parity/golden/`（22 ファイル、14 MB、git の pack では数 MB）。更新は `bash tests/parity/golden.sh target/parity-golden --update`

## 範囲外

- Windows と Linux の実機での一致、PixiJS など他レンダラ、Cubism との一致（M2）、Unity の CI 化

## 受入条件

- [x] 描画リストの数値差が 1e-4 以内（native 対 WASM、native 対 Unity）
- [ ] 最終画像の画素差が 2/255 以内（同上、全ケース）（native 対 WASM は全ケース 0、native 対 Unity は 10 ケースで通り、reference-physics-settle は辺上の 2 画素が 2/255 を超える）
- [x] 許容を超えたとき、比較器が非 0 で終了し、差の大きい画素・数値を報告する
- [ ] CI で native 対 WASM の描画リストと golden が比較される

## 確定したこと（旧「未確定」）

- 描画リスト JSON の形式: `TsukumoeCapture` の形をそのまま使う。`docs/specifications/draw-list.md` の「Capture format」で固定した
- ケースの数: 11（fixture 6、基準モデル 5。`tests/parity/cases/`）。fixture は `web_fixture` ではなく parity 専用の `parity_fixture`（binding 付き）を使う
- 物理: 基準モデルの物理出力はどのメッシュにも結び付いておらず、重み（`weight / 100`）も小さいので、描画リストに出ない。物理は `parity_fixture` の `fixture-physics` で確かめる。基準モデルの `reference-physics-settle` は dt 列の例として残す（`generate` の変更は M3-06 の範囲外）
