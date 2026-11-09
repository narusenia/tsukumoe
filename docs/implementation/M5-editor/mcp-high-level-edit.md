# MCP の高水準編集ツール

- REQ: REQ-AI-001
- 単位: M5-01f
- 状態: 計画中
- 依存: M5-01e, M5-01a
- クレート: `tsukumoe-cli`（MCP のツール・CLI）、`tsukumoe-edit`（処理本体）

## 目的

エディタで 1 操作でできる編集を、エージェントも 1 回の呼び出しでできるようにする。低水準のコマンドを数十回組み立てる手間と、頂点配列を丸ごと送るトークンを減らす。REQ-AI-001 の「UI からできる編集はすべて MCP / CLI からもできる」を満たす中心の単位。

## 範囲

- ツール（どれも `tsukumoe-edit` の関数が返す `Batch` を 1 つの undo 段として適用する）
  - `set_shape_at_pose { element, params, positions | transform }`: 姿勢を指定し、その位置のキーフォームの形を変える。キー上にないときはエラー（キーを足すかは `add_key: true` で選ぶ）。`keyed` の判定をそのまま使う
  - `transform { element, vertices?, translate?, rotate?, scale?, pivot?, params?, proportional? }`: 選んだ点（省略で全部）を移動・回転・拡縮する。`params` を付けるとその姿勢のキーフォームに入る。`proportional { radius, falloff }` で周りの点を重み付きで動かす
  - `wrap_in_deformer { elements, kind: warp|rotation, name?, grid? }`: 要素を新しいデフォーマで包む。見た目を変えない
  - `move_to_deformer { elements, deformer }`: 見た目を保って親を付け替える
  - `mirror_keyform { element, from_params, to_params, center? }` と `mirror_element { element, to }`: 左右反転コピー
  - `bind_and_key { elements, parameter, keys }`: 要素をパラメータに紐づけ、キーを打つ（現在の形で）
  - `duplicate { element, name? }` / `delete_with_children { part }` / `move_to_part { elements, part }`
  - `auto_mesh { mesh, method: contour|grid, ... }`: テクスチャのアルファから輪郭メッシュを作る・格子で作り直す（`core/contour`）
  - `edit_mesh { mesh, op, ... }`: 頂点の追加・削除・辺の分割と反転・結合・外周の延長（`core/mesh_edit`）
  - `create_glue { a: {mesh, vertices}, b: {mesh, vertices}, intensity? }`: 近い順に対にしてグルーを作る
  - `auto_weights { mesh, bones? }` / `paint_weights { mesh, bone, vertices, value }`: ボーンのウェイト
  - `set_physics_feel { group, sway?, stickiness?, length? }`: 揺れ物の 3 つのつまみ
- 各ツールに `dry_run`（M5-01h）と、適用結果の差分（M5-01h）を付ける
- CLI: `tsukumoe edit` の入力に、同じ名前・引数の高水準操作を書けるようにする（`{"op": "mirror_keyform", ...}`）
- 範囲外: 新しい編集機能の追加（エディタにない操作は作らない）

## 方針

- MCP 層では計算しない。`tsukumoe-edit` の関数を呼んで返った `Batch` を `Model::apply` に渡すだけにする。エディタとエージェントで結果が必ず一致する
- 入力スキーマは `schemars` で引数の型から作る（`Command` と同じやり方）。リファレンス（`docs/reference/commands.md`）に高水準ツールの節を足す
- エディタの「選択」に当たるものは、要素名と頂点番号の配列で受け取る
- 名前の自動決定（`unique_name`）はエディタと同じ規則

## 検査

- 各ツールの結果が、エディタの同じ操作（`tsukumoe-app` のテストの手順）の結果と一致する
- 適用後に `validate` が通り、1 回の `undo` で元に戻る
- 失敗（存在しない要素・キー上にない姿勢など）でモデルが変わらない
- 全ツールのサンプル JSON がスキーマで検証でき、リファレンスに載る（M5-01 と同じく網羅を `match` で強制）

## 受入条件

- [ ] エディタの高水準操作がすべて MCP / CLI から 1 回の呼び出しでできる
- [ ] 結果がエディタの同じ操作と一致する
