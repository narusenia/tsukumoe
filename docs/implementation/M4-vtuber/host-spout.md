# ホスト: Spout2 出力（Windows）

- REQ: REQ-TRK-003（Windows は Spout2 で映像を出力できる）
- 単位: M4-03c-1
- 状態: 着手中
- 依存: M4-03a
- クレート: `tsukumoe-host`（Windows 限定・feature `spout`（既定オフ）で `spout-rs` 0.2.0（`vendored`）と `windows` 0.62（D3D11 / DXGI の最小 feature）を optional 依存に足す。ユーザー承認済みの依存追加はこの 2 つのみ）

## 目的

ホストの描画結果を Spout2 の送信側として公開し、OBS（Spout2 プラグイン）や Spout Receiver で受け取れるようにする。ウィンドウキャプチャより画質・透過で有利にする。M4-03b（Syphon）の Windows 版。

## 範囲

- 出力層 `output`: M4-03b の `FrameSink` に `SpoutSink` を足す（feature `spout` かつ Windows のときだけコンパイル）。`output` は `spout-rs` の型を公開しない
- `SpoutSink`: 送信側の名前（既定 `Tsukumoe`、起動オプション `--spout-name`）で公開。公開は毎フレーム 1 回、ウィンドウに描くのと同じ内容
- 公開の解像度: ウィンドウの描画サイズに従う。`--spout-size <w>x<h>` で固定サイズも選べる（仕組みは `--syphon-size` と同じ。`Output` の固定サイズ描画を使う）
- 起動オプション: `--spout`（有効化）、`--spout-name`、`--spout-size`。feature なし・Windows 以外で `--spout` を指定したら、理由を書いてエラー終了する（黙って無視しない）。`--spout-name` / `--spout-size` は `--spout` なしでは指定できない
- 設定パネル: Spout の有効・無効と、送信側の名前・状態の表示（feature 有効時のみ）
- 範囲外: Spout の受信（入力）、仮想カメラ（M4-03c-2 / c-3）、DX12 共有テクスチャのゼロコピー送信（下記「方式」の B）、macOS / Linux

## 方式

`spout-rs` のソースを読んで決めた。Windows 実機での確認は、実機で動いた箇所だけ「実機確認」と書き、それ以外は推論。

### 経緯

- 初版は `spout-rs` 0.1.3 の `SpoutSender::send_image_rgba`（CPU ピクセルを渡す）を使った。実機（Windows）で毎フレーム `Spout: could not send a 920x800 frame` で失敗した（実機確認）
- 原因: この API は Spout の GL 経路（`SpoutSender::SendImage`）を通り、OpenGL コンテキストを要る。ホストの wgpu の既定バックエンド（DX12）には GL コンテキストが無い
- 0.2.0 の `SpoutDirectSender`（呼び出し側が作った共有ハンドルを Spout に登録する）に移る

### 却下: B（wgpu(DX12) の共有テクスチャを NT ハンドルのまま登録するゼロコピー）

- 推論（ソース読み。実機未確認）: `SpoutDirectSender` はハンドルの生の値を Spout のレジストリ（DWORD）へ載せる。受信側は自プロセスで `OpenSharedResource` を呼ぶ（NT ハンドル用の `OpenSharedResource1` ではない）。D3D12 の `CreateSharedHandle` が作るのは NT ハンドルで、その値はプロセスごとのハンドルテーブルの添字なので、別プロセスの受信側では開けない
- D3D12 はレガシー（KMT）共有ハンドルを作れないので、DX12 の共有リソースを直接 Spout に渡す形は成立しない見込み
- 読み戻しのストールが配信で問題になったら、B の変形（wgpu → NT 共有テクスチャ A → D3D11 が `OpenSharedResource1` で A を開き `CopyResource` でレガシー共有テクスチャへ → そのハンドルを登録）を別単位で行う。同期は D3D12 フェンス共有か CPU 待ち

### 採用: A（CPU 読み戻し → D3D11 レガシー共有テクスチャ → `SpoutDirectSender`）

- 描画結果（`Rgba8Unorm`）を読み戻し（`output/readback.rs`）、CPU で BGRA に並べ替える（premultiplied の値はそのまま）
- 自前の D3D11 デバイスで `D3D11_RESOURCE_MISC_SHARED` の `B8G8R8A8_UNORM` テクスチャを作り、`UpdateSubresource` でピクセルを書いて `Flush`。`IDXGIResource::GetSharedHandle` のレガシー共有ハンドルを `SpoutDirectSender` に登録する。BGRA は Spout の既定形式で、受信側（OBS の Spout2 プラグインなど）が確実に扱える
- 解像度が変わったら新しいテクスチャを作って `SpoutDirectSender::update`。拒否されたら送信側を作り直す
- `DirectSharedTexture::from_owned_handle` は `OwnedHandle` しか受け取らず、他の構築手段が無い。レガシーハンドルは実カーネルハンドルではないので、drop 時の `CloseHandle` は失敗する（無害。デバッガか Application Verifier 下でのみ例外になる）。この形を受け入れる。他にハンドルを複製する案は、レガシーハンドルが複製できないので成立しない
- 費用と限界（近道）: 毎フレーム GPU→CPU の読み戻し（1080p で約 8 MB）と同期待ち（`poll`）、CPU の BGRA 変換、CPU→GPU のアップロードが入る。実測はしていない。受信側の数は取れないので、状態は常に「公開中」
- 送信側の名前が既に使われていると Spout が `名前_1` のように変えて登録する。パネルには要求した名前を出す

## 方針

- 既定のビルド・他 OS・CI に影響させない。feature `spout` は既定オフで、`target.'cfg(windows)'` の依存にする
- 依存は `[workspace.dependencies]` で管理（`spout-rs` 0.2.0 `vendored`、`windows` 0.62）。`output`（trait）は外部クレートの型を公開しない。外すときは feature ごと消せる
- 読み戻しは UI にも Spout にも依存しない部品（`output/readback.rs`）に分け、macOS 上の単体テストで確かめる

## ビルドと実行の前提

- ビルドは `cargo build -p tsukumoe-host --features spout` のみ。Spout2 SDK・`Spout.lib`・`Spout.dll`・`SPOUT2_*` 環境変数は要らない（`spout-rs` の `vendored` が同梱ソースから Spout2 を組み込む）
- MSVC の C++ ビルドツールは必要（`vendored` が C++ をコンパイルする）
- 本リポジトリは Spout2 を同梱・インストールしない

## 検査

- 読み戻しの単体テスト（既知の色を描いたテクスチャを読み戻すと、パディングなしの詰まったピクセルが得られる。行幅が 256 バイト境界に合わない大きさも）
- 起動オプションの解析（正常・不正なサイズ・feature なしで `--spout` はエラー・`--spout` なしの `--spout-name` はエラー）
- ビルド: 既定（feature なし）で全体が通る。依存ツリーに `spout-*` / `windows` / `cxx` が入らない。Windows 向けのコードは macOS ではコンパイラで確かめられない（Windows ターゲットが無い）ので、利用者の環境のビルドが最初の確認になる
- 手元の確認（Windows・OBS の Spout2 プラグインか Spout Receiver が要る）: 送信側が一覧に出て、映像が届く。**未確認事項**: レガシーハンドルの登録が受信側で開けること、色が正しい（赤と青が逆でない）、上下が逆でない、透過の縁が汚れない（premultiplied のまま渡している。汚れるなら読み戻しの時点で straight に直す）、`CloseHandle` の失敗が実害を出さないこと。これは利用者環境での確認で、できていなければ受入条件にチェックを付けない

## 受入条件

- [ ] 既定のビルド（feature なし）で依存・挙動が変わらない
- [ ] `--features spout`（Windows）でビルドでき、`--spout` で Spout 送信側として公開する
- [ ] OBS などの Spout 受信側で映像を受け取れる（利用者の環境で確認）
- [ ] 透過の縁が汚れない・上下が正しい（利用者の環境で確認）
- [ ] feature なし・Windows 以外で `--spout` を指定すると理由つきのエラーで終了する

## 決めたこと

- `spout-rs` を Windows 限定・optional・feature `spout`（既定オフ）で足す
- 送信は CPU 読み戻し → D3D11 レガシー共有テクスチャ → `SpoutDirectSender`（上記「方式」の A）。NT ハンドルのゼロコピー（B）は見送り
- Spout2 は `spout-rs` の `vendored` で組み込む（SDK・DLL・環境変数は不要）
- `FrameSink::publish` に渡すテクスチャは `COPY_SRC` 用途も持つ（読み戻しのため）。ウィンドウの描画先と `Output` の固定サイズテクスチャに足す
- Syphon と Spout は OS が違うので同時には有効にならず、`Output` の sink は 1 つのまま。固定サイズは `--syphon-size` と `--spout-size` のどちらか指定された方を使う
