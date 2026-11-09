# 編集処理を UI 非依存のクレートへ移す（`tsukumoe-edit`）

- REQ: REQ-AI-001
- 単位: M5-01e
- 状態: 計画中
- 依存: M5-01
- クレート: `tsukumoe-edit`（新設）、`tsukumoe-app`（移した処理を使う側に変える）、`tsukumoe-cli`（M5-01f 以降で使う）

## 目的

キー位置での形の編集・デフォーマで包む・左右反転コピーなどの高水準の編集は、`tsukumoe-app` にしかない。CLI は UI 境界の下にあり `tsukumoe-app` に依存できないため、エージェントからは低水準のコマンドを組み立てるしかない。これらを UI に依存しないクレートへ移し、エディタと MCP が同じ処理を使うようにする。

## 範囲

- 新しいクレート `tsukumoe-edit`（`tsukumoe-core` にだけ依存する。egui・wgpu に依存しない）
- 移すもの（どれも egui を使っていない。ADR-0019 によりモデルを読んで `Command` を返す純粋な関数）
  - `keyed`（キー位置の判定と、編集をどのキーフォームに入れるか）
  - `deformer_ops`（デフォーマの作成・移し替えの座標変換、自動格子）
  - `mirror`（左右反転コピー、名前の対応）
  - `param_ops`（パラメータの作成・削除・バインド・キーの追加と削除）
  - `tree_ops`（名前変更・子ごとの削除・複製・パーツ移動）
  - `glue`（頂点の対からグルーを作る）
  - `proportional`（エディタ側の重み付き移動の状態のうち、計算の部分）
  - `bone_tool` / `mesh_tool` / `deformer_tool` のうち、純粋な計算（当たり判定・ジオメトリの取得・ウェイトの計算）。ドラッグ中の状態は `tsukumoe-app` に残す
  - `physics_preset`、`motion_import`
  - `heat_color`（ウェイトの配色。M5-01c が使う）
- 依存を断つもの
  - `crate::view`（選択・現在のパラメータ値）: 関数の引数で値を受け取る形にする。`View` 型は `tsukumoe-app` に残す
  - `crate::scene`（コンパイル済みモデルと評価済みの姿勢）: 必要な部分だけを持つ小さな型を `tsukumoe-edit` に置き、`Scene` がそれを持つ
  - `crate::options`（ツールのしきい値）: 値を引数で受け取る
  - `crate::dispatcher`: 移す関数はコマンドを返すだけにし、送るのは呼び出し側
- `Command` の JSON Schema の読み取り（`tsukumoe-app/src/commands.rs` と `tsukumoe-cli/src/commands.rs` の重複）を `tsukumoe-edit` にまとめる
- `docs/specifications/architecture.md` のクレート表と依存図を更新し、ADR を書く
- 範囲外: 移した処理の機能追加、MCP のツール化（M5-01f）

## 方針

- 振る舞いを変えない移動にする。テストも一緒に移し、移動前後でテストの中身を変えない
- 公開 API は「モデル（と姿勢・しきい値）を受け取り、`Command`（多くは `Batch`）か計算結果を返す」に揃える
- 段階的に移す。依存の少ないもの（`physics_preset`・`motion_import`・`glue`・`proportional`）→ `keyed` → `mirror`・`param_ops`・`deformer_ops` → `tree_ops` → ツールの計算部分の順。各段でエディタのテストとスナップショットが通ることを確かめる
- `tsukumoe-edit` は `wasm32-unknown-unknown` でもビルドできる状態を保つ（ブラウザ版のエディタに備える。費用は小さい）

## 検査

- 移動前後で `tsukumoe-app` の全テスト・スナップショットが同じ結果
- `tsukumoe-edit` が egui・eframe・wgpu に依存しない（`cargo tree` で確認するテスト）
- `tsukumoe-edit` が `wasm32-unknown-unknown` でビルドできる
- スキーマの読み取りが 1 か所になり、パレットと MCP のツール一覧が同じ元から作られる

## 受入条件

- [ ] 高水準の編集処理が UI 非依存のクレートにあり、エディタがそれを使っている
- [ ] `tsukumoe-cli` から同じ処理を呼べる
- [ ] アーキテクチャ文書と ADR が更新されている
