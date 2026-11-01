# ホスト: Syphon 出力（macOS）

- REQ: REQ-TRK-003（macOS は Syphon で映像を出力できる）
- 単位: M4-03b
- 状態: 完了
- 依存: M4-03a
- クレート: `tsukumoe-host`（macOS 限定・feature `syphon`（既定オフ）で `syphon-wgpu` を optional 依存に足す。ユーザー承認済みの依存追加はこれのみ）

## 目的

ホストの描画結果を Syphon サーバとして公開し、OBS（Syphon プラグイン）などが GPU 上で受け取れるようにする。ウィンドウキャプチャより画質・遅延・透過で有利にする。

## 範囲

- 出力層 `output`（UI に依存しない）: 「描画結果のテクスチャを受けて公開する」trait `FrameSink`。実装は `SyphonSink`（feature `syphon` かつ macOS のときだけコンパイル）。他の構成では何もしない実装（またはモジュールごと除外）。b の後の c（仮想カメラ・Spout2）が同じ trait に差し込める形にする
- `SyphonSink`: サーバ名（既定 `Tsukumoe`、起動オプション `--syphon-name`）で公開。レンダラの出力は premultiplied のアルファつきなので、`Bgra8Unorm` のテクスチャに描いて渡す（Syphon の既定のアルファの扱いに合わせ、OBS 側で縁が汚れないかを確かめる点を「検査」に書く）。公開は毎フレーム 1 回、ウィンドウに描くのと同じ内容。クライアントがいないときは公開の負荷を避ける（クライアント数が取れるなら 0 のとき飛ばす）
- 公開の解像度: ウィンドウの描画サイズ（M4-03a の透過時のビュー）に従う。`--syphon-size <w>x<h>` で固定サイズも選べる（配信の解像度を固定したい場合）。固定サイズ時は、固定サイズのテクスチャに別途描いて公開する（ウィンドウの描画は変えない）
- 起動オプション: `--syphon`（有効化）、`--syphon-name`、`--syphon-size`。feature なし・macOS 以外で `--syphon` を指定したら、理由を書いてエラー終了する（黙って無視しない）。`Syphon.framework` が見つからないときの実行時エラーは、導入方法（公式の配布先）を含めて表示する
- 設定パネル: Syphon の有効・無効と、公開中のサーバ名・状態の表示（feature 有効時のみ）
- 範囲外: Syphon の受信（入力）、Spout2・仮想カメラ（M4-03c）、`Syphon.framework` の導入・同梱（システム領域へのインストールになるため、利用者が公式の配布物を導入する）、Windows / Linux

## 方針

- 既定のビルド・他 OS・CI に影響させない。feature `syphon` は既定オフで、`target.'cfg(target_os = "macos")'` の依存にする
- 依存は `[workspace.dependencies]` で管理（`syphon-wgpu` は wgpu 30 に合う 0.4 系。README は wgpu 29 対象と書いており、依存の `^30.0` と食い違うので、実際にビルドして確かめ、結果をここに書く）。メンテが若い依存なので、`output` の trait の裏に閉じ込め、外すときは feature ごと消せる
- `output`（trait）は外部クレートの型を公開しない

## 検査

- `FrameSink` の trait 越しの単体テスト（偽の sink に毎フレーム 1 回渡される、無効時は呼ばれない、サイズ固定時に固定サイズで描かれる）
- 起動オプションの解析（正常・不正なサイズ・feature なしで `--syphon` はエラー）
- ビルド: 既定（feature なし）で全体が通る。macOS で `--features syphon` の `cargo check` が通る（リンクには `Syphon.framework` が要る。無い環境ではリンクの確認ができないので、その旨を結果に書く）
- 手元の確認（`Syphon.framework` と OBS の Syphon プラグインが要る）: サーバが OBS などの一覧に出て、映像が届く。透過の縁が汚れない。これはユーザー環境での確認で、できていなければ受入条件にチェックを付けない
- 依存: 既定のビルドの依存ツリーに `syphon-*` / `objc2` が入らない

## 受入条件

- [x] 既定のビルド（feature なし）で依存・挙動が変わらない
- [x] `--features syphon`（macOS）でビルドでき、`--syphon` で Syphon サーバとして公開する
- [x] OBS などの Syphon クライアントで映像を受け取れる（利用者の環境で確認）
- [x] 透過の縁が汚れない（利用者の環境で確認）
- [x] feature なし・macOS 以外で `--syphon` を指定すると理由つきのエラーで終了する

## 決めたこと

- `syphon-wgpu` を macOS 限定・optional・feature `syphon`（既定オフ）で足す
- `Syphon.framework` は利用者が導入する（本リポジトリは同梱・インストールしない）
- 出力は `FrameSink` trait の裏に置き、c（仮想カメラ・Spout2）が同じ形で足せるようにする
- `syphon-wgpu` は 0.4.3（`syphon-core` 0.4.0・`syphon-metal` 0.4.2 を連れてくる）。README は「wgpu 29 対象」と書くが、Cargo.toml の依存は `wgpu` / `wgpu-hal` `^30.0` で、ワークスペースの wgpu 30.0.1 にそのまま解決し `cargo check -p tsukumoe-host --features syphon` が通った（README の記述が古い）
- 公開用テクスチャ: レンダラの出力は `Rgba8Unorm` で、wgpu のテクスチャコピーは RGBA と BGRA を変換できないため、ウィンドウの描画先は直接渡さない。`SyphonSink` が `Bgra8Unorm` のテクスチャを持ち、全画面 1 パス（`textureLoad` で読んで書く。値は変えず、チャンネルの並びだけフォーマットが入れ替える。premultiplied のまま）で描き直してから `publish` する。クライアントがいない間はこのパスも公開も飛ばす。サイズが変わると Syphon サーバとテクスチャを作り直す（ウィンドウ端のドラッグ中は毎フレーム作り直しになる。問題になったら見直す）
- `--syphon-size` 指定時は、`Output` が固定サイズの `Rgba8Unorm` テクスチャを別に持ち、`Viewer::draw`（モデルを進めずに現在のポーズを描く）で描いて渡す。ウィンドウの描画は変えない
- `--syphon-name` / `--syphon-size` は `--syphon` なしでは指定できない（黙って無視せずエラー）
- 出力を有効にしている間はトラッキングと同じく毎フレーム描画する（受信側に一定の流れが要るため）
- `syphon-core` は `Syphon.framework` を同梱の中身から組み直してリンクする（リンクは通る）が、ランタイムの `@rpath` はそのクレート自身のテスト用にしか足さない。そのためホストの `build.rs` が feature 有効時に `-rpath /Library/Frameworks` を足す。フレームワークが無いと、起動時に dyld が探した場所つきでエラーにして終了する（`SyphonSink::new` の導入案内のエラーは、読み込めたが使えないときの保険）
