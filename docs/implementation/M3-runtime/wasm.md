# WASM と npm パッケージ（`tsukumoe-wasm`）

- REQ: REQ-RT-003（受入条件。PERF-003 の WASM 評価時間は M3-07）
- 単位: M3-04
- 状態: 実装済み（CI での実行確認が残る）
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

ブラウザ検証（Chrome、macOS arm64、`http://127.0.0.1` の静的サーバ、基準モデルは `generate` の出力、マスクは `examples/mask_model.rs` のモデル、描画は 512x512、画素は `read_pixels` で読み戻した premultiplied RGBA8 と、`native_render`（`Headless`）の出力を**数値で**比較。スクリーンショットも見た）:

| 項目 | WebGPU | WebGL2 強制 |
|---|---|---|
| 表示 | 表示された | 表示された |
| native との差（マスクモデル） | 最大 0/255、2 超の画素 0 | 最大 0/255、2 超の画素 0 |
| native との差（基準モデル） | 最大 0/255、2 超の画素 0 | 最大 0/255、2 超の画素 0 |
| 最初の描画の提出まで（ページ開始から、基準モデル） | 約 51〜98 ms | 約 77 ms |
| 同、マスクモデル | 約 56〜65 ms | 約 208 ms |

- 起動の内訳（ms、基準モデル）: WebGPU は instance 2.5、adapter 0.2、device 0.6、renderer（パイプライン）0.2 など。WebGL2 は adapter 7.7、renderer 4.2。マスクモデルの初回 WebGL2 だけ adapter 21.3、renderer 110.4（GL のシェーダ変換と推定。基準モデルの 2 回目以降は 4.2 で、キャッシュが効いたと見られる）。wasm の取得とコンパイルは数十 ms 以内
- 1 フレームの `update`（基準モデル、`opt-level=z`）: SIMD 有り 0.313 ms、SIMD 無し 0.317 ms（差なし。評価器が自動ベクトル化されていない）。`opt-level=3`（LTO、SIMD 有り）は 0.214 ms で約 32% 速いが、gzip は wasm-opt 後 1,070,327 バイトで予算を僅かに超える。`update` と描画の提出で 1.4〜2.6 ms
- 配布一式（JS グルー + wasm、`opt-level=z`、gzip -9）: SIMD 有り 821,257 バイト、SIMD 無し 823,080 バイト（グルーだけで gzip 20,142 バイト）
- 検証ページの `update` の 1 回の測定は 300 回の平均で、タイマ分解能は 0.1 ms。タブが背景のとき `requestAnimationFrame` が遅れる（最初の測定で 2.9 秒かかった）ため、「最初の描画の提出まで」を起動時間とした
- WebGPU の canvas は、透明部分が黒で合成された（`alpha_mode` が premultiplied にならなかったと見られる。画素の読み戻しは正しく、表示だけの問題）。WebGL2 は背景が透けて正しい。段階 2 で `CompositeAlphaMode` を確かめる

結論: 画像差・起動時間とも許容内。サイズは `opt-level=s` か `z` の profile で予算内（配布一式で約 0.82 MB）。段階 2 に進める。

### 段階 2: パッケージ化

- `tsukumoe-wasm`: ロード、パラメータ、モーション、表情、自動動作、`update`、描画、描画リストの型付き配列の参照（`memory.grow` で参照が無効になる点を API で扱う）
- `packages/tsukumoe-web/`: TypeScript の型、ブラウザ向けの入口（canvas を渡す）、ライセンス表記
- WASM SIMD を有効にしたビルド
- Node のテスト（描画なしで runtime の結果を検査）。CI に Node とビルド手順を足す

## 範囲外

- Unity（M3-05）、環境間の一致テスト（M3-06）、評価時間の測定と最適化（M3-07）、npm への公開

## 受入条件

- [x] WASM SIMD を有効にしたビルドを提供する
- [x] WebGPU 非対応のブラウザで WebGL2 経路で表示できる
- [x] 描画リストを JS 側の独自レンダラからも参照できる
- [x] 配布 wasm が gzip で 1MB 以内
- [x] TypeScript の型が付いている
- [ ] CI で、ビルドと Node のテストが回る（`ci.yml` に `web` ジョブを足した。実行は push 後に確認する）

## 段階 2 の結果

実装:

- `crates/tsukumoe-wasm`: `Model`（再生側。グラフィクス無しで Node でも動く）と `Screen`（canvas への描画）。`Model` が描画リストを `positions` などへのメモリ上の位置として渡し、メッシュごとの属性は 20 個の数値の表（`meshTable`）で渡す。JS の仕様は `docs/specifications/web-api.md`
- `packages/tsukumoe-web`（`@tsukumoe/web`、`private`）: `createPlayer`、`Model`、`Player`、`DrawList`（型付き配列。プロパティを読むたびに現在の `memory.buffer` から view を作るので、`memory.grow` 後も安全）。SIMD 有り・無しの 2 つの wasm と、`WebAssembly.validate` による実行時の選択。README（英語）に使い方と描画リスト参照の例
- ビルド: `mise run web:build`（`scripts/build-web.sh`: wasm 2 種のビルド、`wasm-bindgen`、`wasm-opt -Oz`、TypeScript、サイズ検査）。成果物は `.gitignore`。Cargo の `wasm` profile（`opt-level=z`、LTO fat、`codegen-units=1`、`panic=abort`）を使う
- テスト: `mise run web:test`（`tsc --noEmit` と `node:test` 9 件。パラメータ、モーション、表情、自動動作、描画リストの配列、`memory.grow` 後の view、SIMD 有り・無しの一致、壊れたファイル）。フィクスチャ `test/fixture.tmod` は `examples/web_fixture.rs` が作る
- WebGPU の canvas の透過: `CompositeAlphaMode::PreMultiplied` を明示して直した（wgpu の web 側は `Opaque` しか報告しないが、設定は受け付ける）
- adapter が取れない場合: WebGPU の adapter と device を canvas に触れる前に取るようにし、取れなければ同じ canvas で WebGL2 に進む（canvas の作り直しは不要）

測定（Chrome、パッケージの入口から。基準モデルとフィクスチャ（マスク付き）を `readPixels` で読み戻し、native の出力と数値比較）:

| 項目 | 結果 |
|---|---|
| WebGPU の表示（透過つき）、基準モデル・フィクスチャ | 表示された。native との差は最大 0/255 |
| WebGL2 強制の表示、基準モデル・フィクスチャ（SIMD 無しも） | 表示された。差は最大 0/255 |
| `navigator.gpu` を隠した `auto` | WebGL2 に落ちて表示された。差 0 |
| 配布 wasm の gzip -9 | SIMD 有り 809,466 バイト、SIMD 無し 811,011 バイト（上限 1,048,576） |
| 最初の描画まで（ページ内、基準モデル） | WebGPU 約 98 ms、WebGL2 約 116 ms |

残り: CI の `web` ジョブは未実行。PixiJS など実在の独自レンダラとの接続は試していない（描画リストの配列と属性の読み出しまで Node で検査した）。

