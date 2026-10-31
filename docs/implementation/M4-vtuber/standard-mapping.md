# 標準パラメータマッピング（`tsukumoe-tracking`）

- REQ: REQ-TRK-002
- 単位: M4-02
- 状態: 未着手
- 依存: M4-01, M3-02
- クレート: `tsukumoe-tracking`（M4-01 で新設済み。`tsukumoe-core` / `tsukumoe-runtime` には依存しない。モデルのパラメータは名前の列で受け取り、出力は「何番目のパラメータにいくつを入れるか」の列にする）

## 目的

`TrackingFrame`（ARKit 52 + 頭部姿勢）を、モデルのパラメータ値へ対応付ける。Live2D の慣習 ID（`ParamAngleX` 等）を持つモデルは、設定なしで頭・目・口が追跡で動く。モデルごとに JSON で上書きできる。

## 範囲

- マッピング規則 `Rule`: 出力パラメータ名、入力の線形結合（ARKit 名 or 頭部角度 `head_yaw` / `head_pitch` / `head_roll`（度）に重みをかけて足す）、オフセット、出力の切り詰め範囲（任意）
- 既定マッピング（Live2D 標準 ID）: `ParamAngleX`（yaw）/ `ParamAngleY`（pitch）/ `ParamAngleZ`（roll）、`ParamEyeLOpen` / `ParamEyeROpen`（`1 - eyeBlink*`）、`ParamEyeBallX` / `ParamEyeBallY`（`eyeLook*` の左右・上下の差）、`ParamBrowLY` / `ParamBrowRY`（`browDown*` と `browOuterUp*` の差）、`ParamMouthOpenY`（`jawOpen`）、`ParamMouthForm`（`mouthSmile*` の平均 − `mouthFrown*` の平均）。ARKit 名そのままのパラメータ（`eyeBlinkLeft` 等、ARKit 名の ID を持つモデル）は同名の blendshape を直接入れる既定規則を足す（Perfect Sync 向け）
- 頭部姿勢の角度変換: クォータニオン → オイラー角（度）。順序と軸の向きを関数 1 つに閉じ込め、符号は規則の重みで反転できる
- JSON 上書き: `{ "rules": [...], "mode": "merge" | "replace" }`。`merge`（既定）は同じ出力パラメータの規則を置き換え、無いものは足す。`replace` は既定を使わない。`schemars` で JSON Schema を生成できる
- `Mapper::bind(&rules, param_names)`: モデルのパラメータ名の列に束縛し、存在するパラメータにだけ規則を解決する（存在しない出力は読み飛ばし、数を返す）。`Mapper::apply(&TrackingFrame, &mut Vec<(usize, f32)>)`: 添字と値を書き出す（確保なしで毎フレーム呼べる）
- パラメータ式（REQ-CORE-004）との組み合わせ: マッピングの出力は、ランタイムの `set_parameter` に入れる値になる。ドライバ式は評価器がその後に評価するので、追跡で動いたパラメータを式の入力にできる（検査で確かめる）
- 範囲外: ランタイム・ホストへの配線（M4-03）、パラメータの範囲（min / max）への正規化（`set_parameter` 側の切り詰めに任せる）、視線追従の高度な較正、較正 UI

## 方針

- 規則は値の式を持たない（線形結合 + 切り詰めだけ）。複雑な加工はパラメータ式側に任せる。JSON を小さく保つ
- 入力の外部由来の JSON は信頼境界。規則数・項数の上限、非有限の重み・オフセット、未知の ARKit 名、空の出力名は読み込み時に拒否する。パニックしない
- 毎フレームの `apply` は割り当てをしない（出力先の `Vec` は呼び出し側が再利用）
- 既定マッピングの式と符号はこの計画書の範囲で決め、実機のトラッキングで較正する余地を残す（符号は JSON で反転できる）

## 検査

- 角度変換: 単位クォータニオンで 0、各軸 ±30° の回転で期待の角度、境界（pitch ±90°）で NaN を出さない
- 既定規則: 標準 ID を持つ小さなモデルのパラメータ名の列に束縛し、代表的な入力（`jawOpen = 1`、`eyeBlinkLeft = 1`、yaw 20°）で期待の値になる。標準 ID を持たない名前は読み飛ばし、その数を返す
- ARKit 名のパラメータへの直接規則（Perfect Sync）
- JSON: 正常、`merge` と `replace` の違い、上限超過・非有限・未知の ARKit 名・空の名前の拒否、JSON Schema の生成
- `apply`: 繰り返し呼んでも割り当てが増えない（容量が変わらない）
- 組み合わせ: 基準モデル（`tsukumoe-cli` の fixture）を `tsukumoe-runtime` で読み、マッピング出力を `set_parameter` で入れて `update` すると、パラメータ式（ドライバ）を通した結果が期待値になる。この統合テストは `tsukumoe-tracking` に `tsukumoe-runtime` を引き込まない場所（`tsukumoe-cli` のテスト等）に置く
- 依存: M4-01 の依存テストが引き続き通る

## 受入条件

- [ ] Live2D 標準パラメータ ID を持つモデルが、設定なしで頭・目・口を追跡する
- [ ] マッピングをモデルごとに JSON で上書きできる
- [ ] パラメータ式と組み合わせられる
- [ ] `tsukumoe-tracking` が `tsukumoe-core` / `tsukumoe-runtime` に依存しない

## 決めたこと

- 規則は線形結合 + 切り詰めのみ。式の評価はしない
- モデルへの束縛は名前の列で受け、出力は添字と値の列にする（ランタイムに依存しない）
- 上書きは `merge`（既定）と `replace`
- 統合テストは `tsukumoe-cli` 側に置く

## 決めること（着手時）

- 頭部角度の軸の向き（カメラ映像の鏡像か否か）の既定符号。VMC（Unity 座標）の向きに合わせる想定で、実装時に決めて計画書に書く
