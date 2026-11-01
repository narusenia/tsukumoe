# ホスト: Spout2 出力（Windows）

- REQ: REQ-TRK-003（Windows は Spout2 で映像を出力できる）
- 単位: M4-03c-1
- 状態: 着手中
- 依存: M4-03a
- クレート: `tsukumoe-host`（Windows 限定・feature `spout`（既定オフ）で `spout-rs` を optional 依存に足す。ユーザー承認済みの依存追加はこれのみ）

## 目的

ホストの描画結果を Spout2 の送信側として公開し、OBS（Spout2 プラグイン）や Spout Receiver で受け取れるようにする。ウィンドウキャプチャより画質・透過で有利にする。M4-03b（Syphon）の Windows 版。

## 範囲

- 出力層 `output`: M4-03b の `FrameSink` に `SpoutSink` を足す（feature `spout` かつ Windows のときだけコンパイル）。`output` は `spout-rs` の型を公開しない
- `SpoutSink`: 送信側の名前（既定 `Tsukumoe`、起動オプション `--spout-name`）で公開。公開は毎フレーム 1 回、ウィンドウに描くのと同じ内容
- 公開の解像度: ウィンドウの描画サイズに従う。`--spout-size <w>x<h>` で固定サイズも選べる（仕組みは `--syphon-size` と同じ。`Output` の固定サイズ描画を使う）
- 起動オプション: `--spout`（有効化）、`--spout-name`、`--spout-size`。feature なし・Windows 以外で `--spout` を指定したら、理由を書いてエラー終了する（黙って無視しない）。`--spout-name` / `--spout-size` は `--spout` なしでは指定できない
- 設定パネル: Spout の有効・無効と、送信側の名前・状態の表示（feature 有効時のみ）
- 範囲外: Spout の受信（入力）、仮想カメラ（M4-03c-2 / c-3）、Spout2 SDK・`Spout.dll` の導入・同梱（利用者が用意する）、DX11 共有テクスチャの直接送信（下記「方式」）、macOS / Linux

## 方式

`spout-rs` 0.1.3 のソースを読んで決めた。

- 送信 API は 2 つだけ: `send_texture`（**OpenGL のテクスチャ ID** を取る）と `send_image`（CPU 上のピクセルを取る）。wgpu の既定の Windows バックエンド（DX12）は GL テクスチャを作れないので、前者は使えない
- 採用: **(b) CPU 読み戻し → `send_image_rgba`**。描画結果（`Rgba8Unorm`）をバッファへコピーして読み戻し、行のパディングを除いて詰めたピクセルを渡す。Spout 側は受け取ったピクセルを共有テクスチャへ書く
- 却下: (a) wgpu(DX12) → 共有 D3D11 テクスチャ → Spout の DX11 送信。`spout-rs` は DX11 の送信を公開しておらず、実現には Spout2 の `SpoutDX` への自前の cxx ブリッジか `windows` クレートでの共有ハンドル実装が要り、コードが大きい。必要になったら別単位で
- 費用と限界（近道）: 毎フレーム GPU→CPU の読み戻し（1080p で約 8 MB）と、同期待ち（`poll` で完了を待つ）が入る。実測はしておらず、配信用途で重ければ (a) に進む。受信側の数は取れないので、状態は「送信側を作れたか」までしか出せない
- Spout 内部の GL 経路（`SendImage` が OpenGL の相互運用を使う場合）が動くかどうかは Windows 上でしか確かめられない。動かないときは、エラーの文言を見て (a) へ進む判断材料にする

## 方針

- 既定のビルド・他 OS・CI に影響させない。feature `spout` は既定オフで、`target.'cfg(windows)'` の依存にする
- 依存は `[workspace.dependencies]` で管理（`spout-rs` 0.1.3）。`output`（trait）は外部クレートの型を公開しない。外すときは feature ごと消せる
- 読み戻しは UI にも Spout にも依存しない部品（`output/readback.rs`）に分け、macOS 上の単体テストで確かめる

## 利用者が用意するもの（ビルドと実行の前提）

- Spout2 の SDK（ヘッダ）: `SPOUT2_SDK_DIR` に `SpoutSender.h` があるディレクトリ（Spout2 リポジトリの `SPOUTSDK\SpoutGL`）を指す
- `Spout.lib`: Spout2 を CMake でビルドして得る。`SPOUT2_LIB_DIR` にそのディレクトリ（例 `cmake-build\lib\Release`）を指す
- `Spout.dll`: 実行時に `PATH` 上か `tsukumoe-host.exe` の隣に置く（例 `cmake-build\bin\Release`）。無いと起動時に Windows のローダがエラーにする
- C++ ビルドツール（MSVC）。`spout-rs` が C++ ブリッジをビルドする
- 本リポジトリはこれらを同梱・インストールしない

## 検査

- 読み戻しの単体テスト（既知の色を描いたテクスチャを読み戻すと、パディングなしの詰まったピクセルが得られる。行幅が 256 バイト境界に合わない大きさも）
- 起動オプションの解析（正常・不正なサイズ・feature なしで `--spout` はエラー・`--spout` なしの `--spout-name` はエラー）
- ビルド: 既定（feature なし）で全体が通る。依存ツリーに `spout-*` / `windows` / `cxx` が入らない。Windows 向けのコードは macOS ではコンパイラで確かめられない（Windows ターゲットが無い）ので、利用者の環境のビルドが最初の確認になる
- 手元の確認（Windows・Spout2 SDK・OBS の Spout2 プラグインか Spout Receiver が要る）: 送信側が一覧に出て、映像が届く。上下が逆でない。透過の縁が汚れない（premultiplied のまま渡している。汚れるなら読み戻しの時点で straight に直す）。これは利用者環境での確認で、できていなければ受入条件にチェックを付けない

## 受入条件

- [ ] 既定のビルド（feature なし）で依存・挙動が変わらない
- [ ] `--features spout`（Windows）でビルドでき、`--spout` で Spout 送信側として公開する
- [ ] OBS などの Spout 受信側で映像を受け取れる（利用者の環境で確認）
- [ ] 透過の縁が汚れない・上下が正しい（利用者の環境で確認）
- [ ] feature なし・Windows 以外で `--spout` を指定すると理由つきのエラーで終了する

## 決めたこと

- `spout-rs` を Windows 限定・optional・feature `spout`（既定オフ）で足す
- 送信は CPU 読み戻し（上記「方式」の (b)）。DX11 共有テクスチャは見送り
- `Spout2 SDK` と `Spout.dll` は利用者が用意する（`SPOUT2_SDK_DIR` / `SPOUT2_LIB_DIR`）
- `FrameSink::publish` に渡すテクスチャは `COPY_SRC` 用途も持つ（読み戻しのため）。ウィンドウの描画先と `Output` の固定サイズテクスチャに足す
- Syphon と Spout は OS が違うので同時には有効にならず、`Output` の sink は 1 つのまま。固定サイズは `--syphon-size` と `--spout-size` のどちらか指定された方を使う
