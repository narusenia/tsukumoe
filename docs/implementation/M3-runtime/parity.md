# 環境間の出力一致テスト

- REQ: REQ-RT-007
- 単位: M3-06
- 状態: 計画中
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

1. native の書き出し（画像と描画リスト JSON）を example で作り、ケース定義（`.tmod` と `update` の dt 列）を `tests/parity/` に置く
2. 比較器を作る
3. WASM: Node で描画リスト JSON を出し、比較する。画像は Chrome の検証ページから出して比較する（手順を `mise run web:parity` にまとめる）
4. Unity: `TsukumoeCapture` に dt 列を渡せるようにし、全ケースの出力を比較する。golden を `tsukumoe-unity` に取り込む手順を `scripts/` に書く
5. CI に native 対 WASM（描画リスト）と golden 一致のジョブを足す

## 範囲外

- Windows と Linux の実機での一致、PixiJS など他レンダラ、Cubism との一致（M2）、Unity の CI 化

## 受入条件

- [ ] 描画リストの数値差が 1e-4 以内（native 対 WASM、native 対 Unity）
- [ ] 最終画像の画素差が 2/255 以内（同上、全ケース）
- [ ] 許容を超えたとき、比較器が非 0 で終了し、差の大きい画素・数値を報告する
- [ ] CI で native 対 WASM の描画リストと golden が比較される

## 未確定

- 描画リスト JSON の形式（`TsukumoeCapture` の形をそのまま仕様にするか）
- ケースの数（時刻・パラメータの組）
