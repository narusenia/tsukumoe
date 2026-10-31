# WASM と npm パッケージ（`tsukumoe-wasm`）

- REQ: REQ-RT-003（受入条件。PERF-003 の WASM 評価時間は M3-07）
- 単位: M3-04
- 状態: 段階 1 実装済み・ブラウザ検証は未実施（拡張が未接続だった）
- 依存: M3-02
- クレート: `tsukumoe-wasm`（新設。`wasm-bindgen`）、npm パッケージ `packages/tsukumoe-web/`

## 目的

ランタイムと描画を WASM にし、TypeScript の型つき npm パッケージとして配布する。描画は WebGPU、非対応環境は WebGL2。描画リストは JS 側の独自レンダラ（PixiJS など）からも読める。

## 決めたこと

- **描画は Rust の `wgpu` を WASM に入れる**（`tsukumoe-gpu` を再利用。WebGPU と WebGL2 は wgpu が処理する）。調査の根拠: WebGL2 の制約に当たるのは `draw_indexed` の `base_vertex` の 1 箇所だけ（頂点バッファのオフセットに替えて直す）。描画規則の実装が 1 本で済み、native と web の一致が構造的に保てる
- **1 モジュール**に runtime と gpu を入れる（別モジュールだと描画リストが memory をまたいでコピーになる）
- **配布サイズの予算は gzip で 1MB 以内**（WebGPU + WebGL2、グルーと PNG デコードを含む）。超えるなら C（描画は WebGL2 だけ TypeScript で書く）へ切り替えるか、予算を相談する
- 追加してよいツール: `wasm-bindgen`（crate と固定バージョンの CLI）、npm 側の TypeScript と Node のテスト実行

## 進め方（2 段階）

### 段階 1: ブラウザ検証（実装に進む前の関門）

最小の `tsukumoe-wasm` と検証ページで、次を実測して報告する。

- WebGPU と WebGL2 の両経路でモデルが表示される
- マスク（`textureLoad` と `gl_FragCoord` の Y 向き）が正しい。native の画像比較との差が 2/255 以内
- gzip 後のサイズ、起動時間（wgpu の初期化とシェーダ変換）
- 通れば段階 2、サイズか起動時間が許容外なら止めて報告（C への切り替えを相談）

### 段階 1 の結果

実装: `crates/tsukumoe-wasm`（`Player`: ロード、パラメータ、`update`、描画、画素の読み戻し）、検証ページと補助物は `crates/tsukumoe-wasm/examples/`（`build.sh`、`index.html`、`native_render.rs`、`mask_model.rs`）。`wasm-bindgen-cli` 0.2.130 と binaryen（wasm-opt）を `mise.toml` に固定した。`renderer.rs` の `base_vertex` は頂点バッファのオフセットに替え、native の既存テストは通る。

測定済み（gzip -9 後の wasm。`opt-level=z`、`lto=fat`、`codegen-units=1`、`panic=abort`）:

| 構成 | wasm-opt 前 | wasm-opt -Oz 後 |
|---|---|---|
| SIMD 有り | 933,096 | 801,238 |
| SIMD 無し | 932,676 | 803,065 |

- 既定の release（`opt-level=3`、LTO なし）は wasm-opt 後で 1,161,811 バイトとなり、予算（1MB）を超える。段階 2 でサイズ向けの profile を決める必要がある（`opt-level=s` は SIMD 有りで 881,920）
- 基準モデル（`assets/reference-model`）にマスクは無い（`mesh_masks` が 0 件）。マスクの確認用に `examples/mask_model.rs` で通常・反転・アルファ傾斜つきのモデルを作る

未実施（ブラウザ拡張が接続できず、ブラウザ操作が 3 回失敗した）:

- WebGPU / WebGL2 での表示、マスクの native との差（2/255 以内）、起動時間、1 フレームの評価時間
- 手順: `build.sh` でビルド → 静的サーバ → `index.html?model=mask&compare=1`（`&webgl=1` で WebGL2 強制、`&simd=0` で SIMD 無し）

### 段階 2: パッケージ化

- `tsukumoe-wasm`: ロード、パラメータ、モーション、表情、自動動作、`update`、描画、描画リストの型付き配列の参照（`memory.grow` で参照が無効になる点を API で扱う）
- `packages/tsukumoe-web/`: TypeScript の型、ブラウザ向けの入口（canvas を渡す）、ライセンス表記
- WASM SIMD を有効にしたビルド
- Node のテスト（描画なしで runtime の結果を検査）。CI に Node とビルド手順を足す

## 範囲外

- Unity（M3-05）、環境間の一致テスト（M3-06）、評価時間の測定と最適化（M3-07）、npm への公開

## 受入条件

- [ ] WASM SIMD を有効にしたビルドを提供する
- [ ] WebGPU 非対応のブラウザで WebGL2 経路で表示できる
- [ ] 描画リストを JS 側の独自レンダラからも参照できる
- [ ] 配布 wasm が gzip で 1MB 以内
- [ ] TypeScript の型が付いている
- [ ] CI で、ビルドと Node のテストが回る
