# トラッキング入力アダプタ（`tsukumoe-tracking`）

- REQ: REQ-TRK-001
- 単位: M4-01
- 状態: 完了
- 依存: なし
- クレート: `tsukumoe-tracking`（新設。`tsukumoe-core` / `tsukumoe-runtime` に依存しない。UI・GPU に依存しない。`wasm32-unknown-unknown` ではビルドできなくてよい（UDP ソケットを使うため）が、共通入力型・正規化・スムージングは I/O を持たない層に分ける）

## 目的

外部のトラッキング手段からの入力を、共通の入力型（ARKit 52 blendshape + 頭部姿勢）に正規化して渡す。パラメータへの対応付けは M4-02 で、ここでは入力を正規化するところまで。

## 範囲

- 共通入力型 `TrackingFrame`: ARKit 52 blendshape（0..1、名前は ARKit の識別子、添字は固定の列挙）、頭部姿勢（クォータニオン）、任意で頭部位置、タイムスタンプ
- アダプタ trait: 受信した入力を `TrackingFrame` にして取り出す。ソケットを持つ層（スレッド・非同期の選択はここ）とデータ変換の層を分け、後者はバイト列だけで単体テストできる
- VMC 受信（UDP、既定ポート 39539）: OSC の最小実装（自前、`float` / `int` / `string` / bundle のみ）。`/VMC/Ext/Blend/Val` + `/VMC/Ext/Blend/Apply`、`/VMC/Ext/Bone/Pos`（`Head`）、`/VMC/Ext/Root/Pos` を扱い、それ以外のアドレスは読み飛ばす
- VMC の blendshape 名の扱い: ARKit 名はそのまま採る。VRM 標準名（`A` / `I` / `U` / `E` / `O` / `Blink` 等）は ARKit 側の近い項目への対応表で変換し、対応のない名前は捨てる（`unmapped` の件数を数える）
- 出力の後処理（アダプタ共通）: スムージング（指数移動平均、項目ごとに強さ）、感度（項目ごとの倍率とオフセット、0..1 に切り詰め）、入力途切れの検出と既定姿勢への復帰
- 範囲外: iFacialMocap（M4-01 の後続）、MediaPipe（Web 版、TS 側。後続）、パラメータへの対応付け（M4-02）、ホストへの組み込み（M4-03）、VMC の送信

## 方針

- 受信と変換を分ける。受信スレッドは最新の `TrackingFrame` を共有に書くだけで、読む側は毎フレーム最新を 1 件取る（キューを持たない。遅れた入力は捨てる）
- 途切れ: 最後の受信から一定時間（設定、既定 0.5 秒）で「途切れ」とし、そこから設定の時間（既定 0.3 秒）で既定値（blendshape は 0、頭部姿勢は単位クォータニオン）へ線形に戻す。戻している途中に入力が再開したら、現在値から入力へ戻る
- スムージングの時定数は時刻で効かせる（フレームレートに依らない）
- 不正な入力は信頼境界で落とす: 長さ・型・件数の上限を超えるパケット、NaN / inf の値、正規化されていないクォータニオンは、そのメッセージだけ捨てて受信を続ける。パニックしない
- 依存は追加しない（OSC は自前、`std::net::UdpSocket`）。スレッドは `std::thread`

## 検査

- OSC: 手書きのバイト列（正常・bundle・パディング境界・型タグ違い・途中で切れたもの・巨大な件数）の構文解析。不正入力でパニックしない（変異テスト）
- VMC: 実際に近いメッセージ列（Blend/Val 複数 + Apply、Bone/Pos Head）から `TrackingFrame` が期待値になる。`Apply` が来るまで反映しない。未対応アドレスと未対応名を読み飛ばす
- 後処理: スムージングの階段応答が時定数どおり、`dt` の刻みを変えても同じ時刻の値が一致する。感度の倍率・オフセット・切り詰め。途切れ → 復帰 → 再開の遷移（途中再開を含む）
- 往復: ローカルの UDP で VMC のパケットを送り、アダプタが受けて `TrackingFrame` を返す（ポートは OS に選ばせる）
- 依存: `tsukumoe-core` / `tsukumoe-runtime` / `egui*` / `wgpu` に依存しないことをクレート依存のテストで検査

## 受入条件

- [x] 共通入力型（ARKit 52 + 頭部姿勢）が定義され、VMC アダプタがそれに正規化して出力する
- [x] 入力が途切れると既定姿勢へ滑らかに戻り、再開すると追従に戻る
- [x] スムージングと感度を項目ごとに設定できる
- [x] 不正なパケットで受信が止まらず、パニックしない
- [x] `tsukumoe-core` / `tsukumoe-runtime` に依存しない

## 決めたこと

- 初回は共通入力型 + VMC のみ。iFacialMocap・MediaPipe は後続
- OSC は自前実装（依存なし）
- ARKit 52 の添字順: 識別子（camelCase）のバイト順アルファベット順。`ARKIT_NAMES` の固定テーブルで、名前引きは ASCII 大文字小文字を区別しない（Perfect Sync の `JawOpen` も受ける）
- VRM 標準名 → ARKit 名: VMC アダプタ内の小さい固定表。`A`→`jawOpen`、`I`→`mouthStretchLeft`/`Right`、`U`→`mouthPucker`、`E`→`mouthSmileLeft`/`Right`、`O`→`mouthFunnel`、`Blink`→`eyeBlinkLeft`/`Right`、`BlinkL`→`eyeBlinkLeft`、`BlinkR`→`eyeBlinkRight`。それ以外は捨てて `unmapped` に数える。M4-02 の既定マッピングは別途設計する
- 公開 API: ポーリングのみ（`TrackingAdapter::latest()`、取り出すと空になる）。コールバックは持たない
- 仕様書: `docs/specifications/` には足さず、この計画書にとどめる
- `Root/Pos` の位置を `head_position` とする。`Bone/Pos` の `Head` は回転のみ採る（位置は無視）。`Blend/Val` は `Blend/Apply` まで保留、骨・ルートは受信時に即反映
- 不正入力の上限: パケット 65536 バイト、1 メッセージ 64 引数、1 パケット 1024 メッセージ、bundle の入れ子 4 段。クォータニオンはノルムが 1 から 0.05 を超えて外れたら捨てる
- 後処理は時刻（秒）を引数に取る純粋な層（`Postprocess::push` / `sample`）。受信スレッドは後処理を持たず、呼び出し側が `latest()` の結果を `push` する
