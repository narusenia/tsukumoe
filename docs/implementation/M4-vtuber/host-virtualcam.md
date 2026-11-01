# ホスト: 仮想カメラ（Windows）

- REQ: REQ-TRK-003（Windows は仮想カメラとして映像を出力できる）
- 単位: M4-03c-2（2a・2b に分割。下記「分割」）
- 状態: 計画中
- 依存: M4-03a（`FrameSink` は M4-03b / c-1 で入った `output` を使う）
- クレート: 新規 `tsukumoe-vcam-channel`（純 Rust・外部依存なし）、新規 `tsukumoe-vcam-source`（`cdylib`・Windows 限定）、`tsukumoe-host`（feature `virtualcam`・既定オフ）

## 目的

ホストの描画結果を Windows の仮想カメラとして公開し、Zoom・Teams・ブラウザ・OBS の「映像キャプチャデバイス」など、Spout を受けられないアプリでも選べるようにする。M4-03c-1（Spout2）の次の出力先。

## 分割

実装は大きい（規模 L 相当）。実機でしか確かめられない部分が多いので、2 つに割り、2a が動いてから 2b に進む。

| 単位 | 中身 | 規模 | 検証 |
|---|---|---|---|
| M4-03c-2a | メディアソース DLL（`tsukumoe-vcam-source`）・登録 / 解除・仮想カメラ作成の確認用ツール・テストパターン表示 | L | 実機のみ |
| M4-03c-2b | フレームチャネル（`tsukumoe-vcam-channel`）・ホストの `VirtualCamSink`・起動オプション・設定パネル | M | チャネルは macOS で単体テスト、結線は実機 |

2a のテストパターンが Windows のカメラアプリで映れば、Frame Server 越しの経路は成立している。2b はそこへホストの映像を流すだけ、という切り分け。

## 仕組み（調べた内容）

出典を付ける。「確認」は公式ドキュメントまたは windows 0.62.2 のソースで読んだ事実、「推論」は読んだ事実からの推測で実機未確認。

### 確認

- `MFCreateVirtualCamera(type, lifetime, access, friendlyName, sourceId, categories)`: Windows Build 22000（Windows 11）以降、`mfsensorgroup.dll`。`type` は `SoftwareCameraSource` のみ。`sourceId` は「カスタムメディアソースの CLSID（`{...}` 形式）」。表示名には「Windows Virtual Camera」が自動で付く（MS Learn `MFCreateVirtualCamera`）
- 返る `IMFVirtualCamera` に `Start` を呼ぶとカメラが列挙・起動可能になる。`lifetime = Session` は `IMFVirtualCamera` を破棄 / `Shutdown` するとカメラが消える。再起動後も残すには `System`。同じ引数で呼ぶと既存のカメラを開く。`Remove` で削除（同上）
- `access = AllUsers` は管理者権限が必要。`CurrentUser` はそのユーザーのみ。`categories` を管理者以外が指定するなら VIDEO_CAMERA / VIDEO / CAPTURE / SENSOR_CAMERA の部分集合（同上）
- カメラのプライバシー設定（カメラへのアクセス）が拒否だと `E_ACCESSDENIED`（同上）
- フレームの出どころはホストではなく、Frame Server が読み込むメディアソース。Frame Server のカスタムメディアソースは **LocalService アカウント・セッション 0** で動き、ユーザーのデスクトップとは対話できず、ファイルシステム・レジストリは基本的に読み取りだけ（MS Learn「Frame Server Custom Media Source」）
- 実装する COM インターフェース: `IMFMediaSourceEx`（`IMFMediaSource` → `IMFMediaEventGenerator`）、`IMFMediaStream2`、`IMFGetService`（未対応でも `MF_E_UNSUPPORTED_SERVICE` を返す実装は必須）、`IKsControl`（コントロールが無くても実装する）。ストリームの `MF_DEVICESTREAM_STREAM_CATEGORY` は `PINNAME_VIDEO_CAPTURE`、`MF_DEVICESTREAM_STREAM_ID` は 0 始まり。サンプルは `MFCreateSample` で作り、時刻・長さを付け、時刻は QPC 基準。映像で `MEEndOfStream` / `MEEndOfPresentation` は送らない（同上）
- CLSID の登録は `InprocServer32`（DLL のパス）と `ThreadingModel = Both`（同上）
- 参考実装 VCamSample（smourier、C++）: DLL はアプリ・Frame Server・カメラモニタ・利用側アプリの 4 者が読み込むので、登録は `HKLM`（`regsvr32`・管理者）。DLL は Local Service / Local System から読める場所に置く（ユーザープロファイル配下は `Start` が Access Denied になりうる）。ダウンロード物は「ブロック解除」が要りうる。ホスト側からソースへの映像の渡し方は README に書かれていない（`https://github.com/smourier/VCamSample`）
- windows 0.62.2 のバインディング（`~/.cargo/registry` のソースで確認）: `MFCreateVirtualCamera` / `IMFVirtualCamera`（`Start` `Stop` `Remove` `GetMediaSource` `Shutdown` など）/ `IMFMediaSourceEx` / `IMFMediaStream2` / `IMFActivate` / `IMFSampleAllocatorControl` / `IKsControl` / `IClassFactory` / `CoRegisterClassObject` が揃う。`IMFVirtualCamera` と `IMFMediaSourceEx` / `IMFMediaStream2` の `_Impl`（Rust で実装する側）は feature `Win32_System_Com_StructuredStorage` と `Win32_System_Variant`（`IMFVirtualCamera` はさらに `Win32_Devices_Properties`）が無いと出ない。型は `Win32_Media_MediaFoundation`、`IKsControl` は `Win32_Media_KernelStreaming`
- Rust 製の同種の実装例は検索で見つからなかった（C++ の VCamSample が唯一の手がかり）

### 推論（実機未確認）

- フレームチャネルの置き場: ホストは通常ユーザー、受け側は別アカウントのセッション 0。`Local\` 名前空間のオブジェクトは見えない。`Global\` のファイルマッピングは `SeCreateGlobalPrivilege` が無い通常ユーザーのホストからは作れない見込み。**ファイルを共有メモリの代わりに使う**: 管理者のインストーラが `C:\ProgramData\Tsukumoe\vcam\` を作り、ユーザーに書き込み・LocalService に読み取りを許す ACL を付ける。ホストがその中のファイルを作ってマップして書き、ソース側が同じファイルを読み取りでマップする（名前なしマッピングでも同じファイルなら同じページを見る）。名前空間を越えない。通知はイベントを使わず、ソース側が 30 fps の自前タイマで世代番号を見る（イベントを使うなら ACL 付きの `Global\` イベントを別途）
- 名前付きパイプ（DACL で LocalService を許可）は代案。1080p BGRA で毎秒約 250 MB が流れる。ソース側が NV12 で受ければ約 90 MB/s。ファイルマッピングの方が単純なので第一候補にする
- ソースが出す形式: ホスト側で BGRA→NV12 に変換して渡す（変換は CPU・`vcam-channel` 側の関数）。NV12 は仮想カメラの利用側が確実に扱える形式として VCamSample も出す。RGB32 も併記して出せば最悪でも映る見込みだが、透過は仮想カメラに載らない（アルファは捨てる。背景色は決める。下記）
- Rust の `#[implement]`（`windows-core` のマクロ）で、複数インターフェース持ちの COM オブジェクトを作れる。ただしメディアソースは `IMFMediaEventGenerator` のイベントキュー（`MFCreateEventQueue`）、`IMFPresentationDescriptor`、ストリーム選択、`Start` / `Stop` / `Pause` の状態遷移、`MENewStream` / `MESourceStarted` の送出を手書きする必要がある。VCamSample は 1 クラス約 1,000 行規模の C++

## 範囲

### M4-03c-2a: メディアソース DLL と登録

- 新規クレート `tsukumoe-vcam-source`（`crate-type = ["cdylib"]`）。`#![cfg(windows)]` 相当で、Windows 以外では空のクレートとしてビルドが通る
- COM: `DllGetClassObject`（`IClassFactory`）、`DllCanUnloadNow`、`DllRegisterServer` / `DllUnregisterServer`（`HKLM\Software\Classes\CLSID\{ID}\InprocServer32` に自分のパスと `ThreadingModel = Both`）。CLSID は固定の新規 GUID
- メディアソース: `IMFMediaSourceEx` + `IMFGetService` + `IKsControl`（空実装）、1 ストリーム（`IMFMediaStream2`・`PINNAME_VIDEO_CAPTURE`・`MFFrameSourceTypes_Color`）、NV12・1280x720・30 fps 固定から開始（まず 1 形式に絞る）
- 2a の映像はテストパターン（色帯と動く印）。CPU で生成して `MFCreate2DMediaBuffer` に詰める。D3D マネージャ（`SetD3DManager`）は受けて無視する（GPU 経路は範囲外）
- 確認用ツール（`tsukumoe-vcam-source` の例か小さな bin）: `MFCreateVirtualCamera` → `Start` → Enter で `Shutdown`。`lifetime = Session`、`access = CurrentUser`。管理者は要らない（登録が管理者）
- 範囲外: ホストの映像、複数解像度、音声、カメラコントロール（露出など）、D3D 経由、システム常駐（`lifetime = System`）、インストーラ

### M4-03c-2b: フレームチャネルとホストの出力

- 新規クレート `tsukumoe-vcam-channel`: チャネルのレイアウト（ヘッダ: マジック・版・幅・高さ・形式・世代番号・フレームサイズ、続いてピクセル）、書き手 / 読み手（バイト列に対する純粋なコード）、BGRA→NV12 変換。外部依存なし。macOS で単体テスト
- 世代番号による整合: 書き手は偶数→奇数→偶数の 2 回更新（シーケンスロック）。読み手は前後の世代が同じで偶数のときだけ採用し、そうでなければ前のフレームをもう一度出す
- ホスト: `VirtualCamSink`（`FrameSink`）。CPU 読み戻し（`output/readback.rs` を Spout と共用）→ NV12 → チャネルへ書く。解像度はソースの固定（2a の値）に合わせる。ホストは `--vcam-size` を持たず、`Output` の固定サイズを 1280x720 に強制する。ソース側が読み取り中かどうかは世代番号の確認応答（ソースが `ack` 欄を更新）で `SinkStatus` に出す
- 起動オプション: `--vcam`（有効化）。feature なし・Windows 以外・チャネルのファイルが作れない（インストール前）ときは、理由を書いてエラー終了する（黙って無視しない）。Spout・Syphon と同時には使えない（sink は 1 つ）
- 仮想カメラの作成 / `Start` はホストが行う（2a の確認用ツールの処理を移す）。`lifetime = Session`・`access = CurrentUser`。ホストが終了すればカメラは消える
- 設定パネル: 仮想カメラの有効・無効と状態（feature 有効時のみ）
- 透過: 仮想カメラはアルファを持てない。premultiplied の RGB をそのまま背景色（既定は黒。`--vcam-background` は持たない）に重ねてから NV12 にする（premultiplied なので黒は単に捨てるだけ）。緑背景などはクロマキー用途として後で足す
- 範囲外: ホスト内での複数カメラ、Windows 10、マイク、`lifetime = System`、macOS（M4-03c-3）

## 依存と新規ファイル

| 項目 | 内容 |
|---|---|
| `windows` features（workspace 共通へ追加） | `Win32_Media_MediaFoundation`、`Win32_Media_KernelStreaming`、`Win32_System_Com`、`Win32_System_Com_StructuredStorage`、`Win32_System_Variant`、`Win32_Devices_Properties`、`Win32_System_Registry`、`Win32_System_LibraryLoader`、`Win32_System_Memory`、`Win32_Storage_FileSystem`、`Win32_Security`。ホストは `Win32_Media_MediaFoundation` 周りと `Win32_System_Memory` / `Win32_Storage_FileSystem` のみ必要 |
| 他クレート | なし（`windows` の `implement`（`std` 既定）だけ。新規クレート追加はこの設計では不要） |
| 新規ワークスペースクレート | `tsukumoe-vcam-channel`（lib）、`tsukumoe-vcam-source`（cdylib） |
| `tsukumoe-host` | feature `virtualcam`（既定オフ）= `dep:windows` + `dep:tsukumoe-vcam-channel` |
| 新規 ADR | 要。Frame Server 経由・ファイルによるチャネル・`HKLM` 登録を決定として残す（実装前に `docs/adr/` へ） |

## 方針

- 既定のビルド・他 OS・CI に影響させない。`tsukumoe-vcam-source` は Windows 以外で空、ホストの機能は feature `virtualcam`（既定オフ）
- `FrameSink` の形（`publish`）は変えない。`output` は新しい外部型を公開しない
- チャネルのコード（`vcam-channel`）は OS 非依存の純粋なコードにして macOS でテストする。COM と登録は `vcam-source`、`CreateFileMapping` などは各自の側に閉じる
- ソース DLL はファイルシステム・レジストリに書かない（LocalService の制約）。何かあれば ETW ではなく `OutputDebugString` で出す
- 失敗を握りつぶさない。ソースはチャネルが無い・古いときは最後のフレームを出し続け、完全に無いときは「待機中」の画面（単色）を出す

## 検査

- 単体テスト（macOS）: チャネルの書き読み、シーケンスロックの破れ検出（途中で書き手が更新された読み取りを捨てる）、ヘッダ検証（マジック・版・幅・高さの不一致）、BGRA→NV12（既知の色が期待値になる）、`--vcam` の解析
- ビルド: 既定で全体が通る。依存ツリーに `windows` / `tsukumoe-vcam-*` が入らない。Windows 向けコード（`vcam-source`・`VirtualCamSink`）は macOS ではコンパイラで確かめられない（Windows ターゲットが無い）ので、利用者環境のビルドが最初の確認
- 実機（Windows 11・Camera アプリ・OBS）のみで確認できること:
  - 2a: 管理者の `regsvr32 tsukumoe_vcam_source.dll` が通る / 確認用ツールが `Start` に成功する（`E_ACCESSDENIED` が出ないか。出るなら DLL の置き場所を `C:\Program Files\...` へ）/ Camera アプリ・ブラウザの一覧に出て、テストパターンが映る / `Shutdown` で一覧から消える
  - 2b: ホストの映像が Camera アプリに映る / 色・上下・向き / 透過の合成 / ホスト終了でカメラが消える / ProgramData の ACL でホストが書け LocalService が読める / 利用側が複数（Camera アプリと OBS）でも映る / 30 fps が出るか
- 未確認事項はチェックを付けない（実機で確認できていなければ受入条件を空欄のままにする）

## 受入条件

- [ ] 既定のビルド（feature なし）で依存・挙動が変わらない
- [ ] 2a: 登録したメディアソースが仮想カメラとして列挙され、テストパターンが Camera アプリに映る（利用者の環境で確認）
- [ ] 2b: `--vcam` でホストの映像が Camera アプリ・OBS の映像キャプチャに映る（利用者の環境で確認）
- [ ] 色・上下・向きが正しい（利用者の環境で確認）
- [ ] feature なし・Windows 以外・未登録の状態で `--vcam` を指定すると理由つきのエラーで終了する
- [ ] 利用者が行うシステム変更（登録・解除の手順）が README に書かれている

## 利用者が行うシステム変更（承認が要る）

- 管理者権限で `regsvr32`（HKLM の COM 登録。解除は `regsvr32 /u`）。2a の確認時と 2b の使用時の両方で必要
- DLL を LocalService が読める場所（例 `C:\Program Files\Tsukumoe\`）へ置く。ユーザープロファイル配下・`target\` のままだと Frame Server が読めない見込み
- 管理者権限で `C:\ProgramData\Tsukumoe\vcam\` を作り、ACL（ユーザー書き込み・LocalService 読み取り）を付ける
- Windows のプライバシー設定「カメラへのアクセス」（アプリのカメラ利用許可）がオンであること。拒否だと `E_ACCESSDENIED`
- 本リポジトリは DLL をインストールもレジストリ変更もしない。上記は利用者が手順を見て行う

## 主なリスク

- メディアソースの手書き（Rust には前例が見つからない）。イベントキュー・状態遷移・属性の取りこぼしで、Frame Server が黙ってソースを落とす。実機でのデバッグ（`OutputDebugString`・Event Viewer）が要り、macOS から検証できない。**最大のリスク**
- LocalService / セッション 0 の権限境界。ProgramData の ACL とファイルマップの組み合わせは推論で、実機で通らなければ名前付きパイプへ切り替える
- VCamSample の README に「Teams でプレビューは映るが相手には映らない」とある（原因不明）。同種の問題が出ると、会議アプリでの実用が成立しない恐れ
- 透過が使えない。VTuber 用途の主な利用側（OBS）は Spout / Syphon のほうが有利で、仮想カメラは会議アプリ向けの補助になる
- CPU 読み戻し + NV12 変換 + ファイル書き込みの費用。1280x720 では小さいが、実測はしていない
- DLL のロード中は上書き・削除できない（再ビルドのたびにカメラを止めて Frame Server の解放を待つ必要がある）

## 決めたこと

- 仮想カメラは `MFCreateVirtualCamera`（Windows 11 のみ）。Windows 10 は対象外
- メディアソースは新規 `cdylib` クレート `tsukumoe-vcam-source` に Rust で実装する。C++ は使わない
- ホストからソースへのチャネルはファイル（ProgramData）のメモリマップ + シーケンスロック。名前付きパイプは代案
- 形式は NV12・1280x720・30 fps 固定から始める。可変解像度は範囲外
- カメラは `Session` / `CurrentUser`。システム常駐は範囲外
- 2a が実機で動くまで 2b に着手しない
- 実装前に ADR を書く
