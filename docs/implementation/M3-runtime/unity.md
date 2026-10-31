# Unity パッケージ（`tsukumoe-unity`）

- REQ: REQ-RT-004（受入条件。出力一致の判定は M3-06）
- 単位: M3-05
- 状態: 段階 1 完了、段階 2 実施中
- 依存: M3-03（`tsukumoe-ffi`）
- リポジトリ: 別 repo `tsukumoe-unity`（`~/ghq/local/tsukumoe-unity`。GitHub での公開は後で決める）。UPM パッケージ名 `com.tsukumoe.unity`

## 目的

Unity で `.tmod` を再生し、描画する。評価は `tsukumoe-ffi` のネイティブプラグイン、描画は Unity のシェーダ。wgpu は持ち込まない。描画は `rendering.md` に従い、wgpu 参照レンダラと同じ見た目を出す。

## 決めたこと

- **Unity 6000.3 LTS**（手元は 6000.3.27f2）。**URP のみ**を初版の対象にする。Built-in と HDRP は対象外（受入条件の「仕様で決める」は、この対象外の明記で満たす）
- **評価は C ABI**（`docs/specifications/c-abi.md`）を P/Invoke で呼ぶ。C# のラッパは ABI の 1 対 1 の写しにし、ABI の major を起動時に検査する
- **描画リストは Unity の `Mesh` に写す**。`positions`・`uvs` は `geometry_changed` のメッシュだけ、不透明度・色は `style_changed` のメッシュだけ更新する。描画順は `order` に従う
- テクスチャは `.tmod` に埋め込まれた PNG を C 側から取り、読み込み時に premultiply する（`rendering.md` §4）
- マスクは `rendering.md` §3 の式（マスクメッシュのアルファの最大値）をそのまま実装する。方式（オフスクリーンの RT に最大値ブレンドで描いて参照する案が本命）は段階 1 で実測して決める
- 対象 OS は Windows x64 と macOS（arm64 / x64）のエディタとスタンドアロン。ネイティブライブラリは seimei 側のビルドで作り、パッケージの `Runtime/Plugins/` へ置く（バイナリを git に入れるかは段階 2 で決める）

## 進め方（2 段階）

### 段階 1: 実現性の関門

最小の Unity プロジェクトで次を実測して報告する。

- `tsukumoe_ffi` の dylib をエディタが読み込み、基準モデルの `update` と描画リストの取得ができる
- URP で基準モデルとマスク付きモデルが表示される
- **色空間**: `rendering.md` は sRGB の変換をしない前提。Unity の Linear 色空間では、フレームバッファの書き込みと混合が線形値で行われ、一致しない。Gamma 専用にするか、専用 RT を非 sRGB で作って最後に合成するかを決める
- native の画像（`native_render`）との画素差が 2/255 以内（同じビュー・同じ解像度。読み戻しは `ReadPixels`）
- Unity を batchmode で動かせる（ライセンス認証、`-runTests`）。動かない場合は CI での自動検証を諦める条件を報告する
- 通れば段階 2。色空間かマスクで許容内に収まらなければ止めて報告

### 段階 2: パッケージ化

- `Runtime/`: ABI ラッパ、`TsukumoeModel`（`.tmod` のロード・パラメータ・モーション・表情・自動動作・`update`）、`TsukumoeRenderer`（MonoBehaviour。メッシュ生成・描画・マスク）、URP シェーダ（3 つのブレンドモード、乗算色、スクリーン色）
- `Editor/`: `.tmod` の ScriptedImporter、インスペクタ
- `Tests/`: EditMode（ABI の呼び出しと描画リストの数値）、PlayMode（描画の画素）
- `Samples~/`: 基準モデルを置いたシーン
- 描画リストの数値とスクリーンショットを書き出す口（M3-06 が native・WASM と比較する）
- `package.json`、README（英語）、ライセンス表記（Apache-2.0 OR MIT）

## 段階 1 の結果

実装: 別 repo `tsukumoe-unity`（`package/` が UPM パッケージ、`project/` が検証用の Unity プロジェクト）。ABI ラッパ `Native.cs`・`TsukumoeModel.cs`、描画 `OffscreenRenderer.cs` とシェーダ `Drawable.shader`、batchmode の入口 `Editor/PoC.cs`、比較は `scripts/compare.py`。

測定（Unity 6000.3.27f2、Metal、macOS arm64。同じ `.tmod`・同じビュー・512x512 で `native_render` と画素比較）:

| モデル | 色空間 | 最大差 | 2/255 超の画素 |
|---|---|---|---|
| fixture（`web_fixture`: 通常・反転マスク、アルファ傾斜、opacity 0.8） | Linear | 0/255 | 0 |
| fixture | Gamma | 0/255 | 0 |
| 基準モデル（150 メッシュ、変形・物理） | Linear | 1/255 | 0 |

決めたこと:
- **色空間**: 非 sRGB の RenderTexture に描き、テクスチャは linear で読み込んで premultiply する。Linear・Gamma の両方で一致するので、Gamma 専用にはしない
- **マスク**: 専用 RT に最大値ブレンド（`BlendOp Max`）で描き、`Texture2D.Load` で参照する。Render Graph に依存しない
- **カリング**: RT への投影は Y が反転するため `Cull Front` を使う（`rendering.md` の反時計回りが表になる）
- batchmode はライセンス認証つきで動作する（`-executeMethod`、`-nographics` なしで描画できる）

未確認: 時計回りのメッシュの背面カリング（fixture に無い）、URP のカメラ経路、Windows、スタンドアロン、フレーム時間。

## 範囲外

- Built-in / HDRP、iOS / Android / WebGL / Linux、環境間の一致の判定（M3-06）、Asset Store・OpenUPM への公開、Unity 上での編集

## 受入条件

- [ ] 対応 Unity バージョンと URP のみを対象とすることを README と仕様に書いた
- [ ] 基準モデルとマスク付きモデルが Unity 上で wgpu 参照レンダラと 2/255 以内で一致する（段階 1 で測定、M3-06 で CI 化）
- [ ] Windows / macOS のエディタとスタンドアロンで動く（Windows の実機確認は手元に環境がなければ未確認と明記）
- [ ] ABI の major 不一致をロード時に検出する
- [ ] EditMode / PlayMode のテストが batchmode で通る

## 未確定

- Unity 6 の Render Graph への組み込み方（`ScriptableRendererFeature` か、メッシュごとの `Graphics.DrawMesh`）
- ネイティブライブラリの配布（git 管理、Release 添付、ビルドスクリプトのみ）
