# モデル設定（`tsukumoe-core` / `tsukumoe-format` / `tsukumoe-app`）

- REQ: REQ-EDIT-004（パラメータとキーフォームの編集。表示名・グループ・標準パラメータ）、REQ-TRK-002（標準パラメータマッピング）
- 単位: M5-10
- 状態: 実装済み（実機の GUI 操作は未確認）
- 依存: M5-06a、M5-06e（表示名・グループは `Parameter` にある）
- クレート: `tsukumoe-core`（データ、コマンド、`.tmod` の読み書き）、`tsukumoe-format`（`.tproj` の DTO）、`tsukumoe-runtime`（読み出しと当たり判定）、`tsukumoe-app`（パネル）。新しい依存クレートは足さない

## 目的

モデルの「設定」を持たせ、エディタで編集し、`.tproj` と `.tmod` の両方に保存する。ランタイムが、標準パラメータの割り当てと当たり判定を読める。

- **標準パラメータの割り当て**: 役割（頭の角度 X/Y/Z、体の角度 X/Y/Z、目の開閉（左右）、目玉の向き X/Y、口の開き、口の形、眉）に、どのパラメータを使うか。自動まばたき・リップシンク・トラッキングが名前に頼らず使う
- **当たり判定**: 名前付きの領域。メッシュを指す。クリックがどの領域に当たったかを返すためのもの
- **モデル情報**: タイトル・作者・ライセンス・メモ

範囲外: 表示名とグループ（`Parameter.display_name` / `group` として既にある）、Live2D の `model3.json` / `cdi3.json` の読み書き（COMPAT 側）。

## 前提（現状）

- `Model` に「設定」は無い（`parameters` `parts` `meshes` `physics` など）。物理と同じく `Table` ではなく素の構造体として持つのが合う（M5-07 の `Change::Physics` と同じ全体の前後の記録）
- `.tmod` は、任意セクション（`0x80` 以上）を足しても、マイナーバージョンを上げずに済む（`docs/specifications/file-formats.md`。`MESH_HIDDEN` `WARP_BEZIER` の前例）。読めない読み手は無視する
- `.tproj` の `ModelFile`（`tsukumoe-format/src/dto.rs`）は、`#[serde(default)]` の欄を足せば、古いファイルも読める

## 設計

- **データ**（`tsukumoe-core/src/settings.rs`）
  - `ModelSettings { info: ModelInfo, hit_areas: Vec<HitArea>, standard: Vec<(StandardRole, ParameterId)> }`
  - `ModelInfo { title, author, license, notes }`（文字列）
  - `HitArea { name, mesh: MeshId }`（名前は一意）
  - `StandardRole`: `AngleX` `AngleY` `AngleZ` `BodyAngleX` `BodyAngleY` `BodyAngleZ` `EyeLOpen` `EyeROpen` `EyeBallX` `EyeBallY` `MouthOpen` `MouthForm` `BrowLY` `BrowRY`（役割ごとに高々 1 パラメータ）
- **コマンド**（すべて `Command`。Undo 1 回。パラメータとメッシュは名前で指す。変更の記録は設定全体の前後）
  - `SetModelInfo { title, author, license, notes }`
  - `AddHitArea { name, mesh }`、`RemoveHitArea`、`RenameHitArea`、`SetHitAreaMesh`
  - `SetStandardParameter { role, parameter: Option<String> }`（`None` で外す）
  - 検査: 名前の一意、メッシュ・パラメータの存在。パラメータ・メッシュの削除は、設定が指していれば失敗する（`validate`）
- **`.tproj`**: `ModelFile` に `settings` を足す（`default`、空なら書かない）。パラメータ・メッシュは名前で保存する
- **`.tmod`**: 任意セクションで保存する。文字列用に `SETTINGS_STRINGS`（`0x9C`）、本体に `MODEL_INFO`（`0x9D`）、`HIT_AREAS`（`0x9E`）、`STANDARD_PARAMS`（`0x9F`）。設定が空なら 1 つも書かず、他のファイルは変わらない。4 つは一緒に書く。`CompiledModel` に写し、`layout` の検査（範囲外の番号）を足す
- **ランタイム**: モデルから情報・割り当て・当たり判定の一覧を読める。当たり判定の検査（`hit_test(x, y)`）は、領域のメッシュの現在の描画位置の外接矩形で行い、当たった領域の名前を返す
- **パネル `model_settings`**: 3 つの節。標準パラメータは、役割ごとにパラメータのコンボボックス。「名前から割り当てる」ボタンで、`ParamAngleX` などの標準名のパラメータを一括で入れる。当たり判定は一覧と、名前・メッシュの編集。情報は 4 つの欄。説明文は画面に出さない（ラベルと操作名だけ）

## 決めること（実装時）

- `.tmod` の文字列（情報・当たり判定の名前）を、既存の `STRINGS`（`StrRef`）に入れるか、`SETTINGS_STRINGS` に分けるか。`tmod/playback.rs` が `PLAYBACK_STRINGS` を分けた前例に合わせるなら後者
- 役割の一覧に、他に要るもの（`BreathParam`、`HairFront` など）があるか。実装時は上の一覧で始める

## 検証（変更したクレートだけ）

- `tsukumoe-core`: 各コマンドの適用・Undo・不正値の拒否、指されているパラメータ・メッシュは消せないこと、`.tmod` の往復（空・全部入り）、古い `.tmod`（セクション無し）が読めること
- `tsukumoe-format`: `.tproj` の往復、設定の無い古いファイルの読み込み
- `tsukumoe-runtime`: 当たり判定が、変形したメッシュの位置で当たる / 外れること
- `tsukumoe-app`: パネルの kittest
- `docs/specifications/file-formats.md` に新しいセクションを書く。`docs/reference/commands.md` は `tsukumoe docs commands` で再生成
- 実機の GUI 操作は `gui-checklist.md` に追記

## 受け入れ基準

- [ ] 標準パラメータ・当たり判定・モデル情報を編集でき、Undo / Redo で戻る
- [ ] 保存して開き直すと同じ内容。`.tmod` に書き出して `tsukumoe inspect` で見ても同じ
- [ ] 設定の無い古い `.tproj` / `.tmod` が、そのまま読める
- [ ] ランタイムが当たり判定の名前を返す
- [ ] 指されているパラメータ・メッシュは削除できない
