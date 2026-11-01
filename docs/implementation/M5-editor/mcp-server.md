# MCP サーバとエージェント向け API リファレンス（`tsukumoe-cli` / `tsukumoe-core`）

- REQ: REQ-AI-001
- 単位: M5-01
- 状態: 計画中
- 依存: M1-02
- クレート: `tsukumoe-core`（コマンドのスキーマ生成）、`tsukumoe-cli`（MCP サーバ・`commands` / `edit` サブコマンド・リファレンス生成）。`tsukumoe-format`（プロジェクトの読み書き）を使う

## 目的

編集コマンド（`tsukumoe-core` の `Command`）を、エージェントが MCP と CLI から実行できるようにする。コマンドの一覧・引数スキーマ・API リファレンスは `Command` の定義から生成し、手で二重管理しない。

## 範囲

- スキーマ生成: `Command` に `schemars::JsonSchema` を導出する（`schemars` はワークスペース既存）。`tsukumoe-core` は `wasm32-unknown-unknown` でビルドできる制約を保つ。導出が wasm / FFI に重みを足すなら `schema` フィーチャで囲む
- CLI:
  - `tsukumoe commands`: コマンド名・説明・引数スキーマを JSON で出す
  - `tsukumoe edit <project> <command.json|->`: コマンド（または配列）を 1 つの undo 段として適用し、プロジェクトに保存する。失敗時は何も書かない
  - `tsukumoe docs commands`: API リファレンス（Markdown）を出す
- MCP サーバ `tsukumoe mcp <project>`: stdio の JSON-RPC 2.0（`initialize`、`tools/list`、`tools/call`、`ping`）。プロジェクト 1 つをメモリに開き、コマンドを適用する
  - ツール: `Command` のバリアントごとに 1 つ（名前は `type` のスネークケース、入力スキーマはそのバリアントのスキーマ）。加えて `batch`（`Batch` と同じ。複数コマンドを 1 undo）、`undo`、`redo`、`validate`（`Model::validate` の結果）、`get_model`（要素名と件数の概要）、`save`
  - コマンド失敗は MCP のツールエラー（`isError: true`）で、`CommandError` の文言を返す。プロトコル違反は JSON-RPC エラー
- API リファレンス: コマンドごとに名前・説明（`Command` の doc コメント由来）・引数の表・JSON 例を出す。`docs/reference/commands.md` に生成物をコミットし、最新かどうかをテストで確認する
- 範囲外: エディタ起動中のプロジェクトへの送信（エディタ本体が M5-02 以降。受入条件「UI からできる編集はすべて MCP / CLI からもできる」は、コマンドが増えるたびに満たし直す）、ヘッドレスのレンダ・差分・検査ツール（REQ-AI-002）、HTTP / SSE トランスポート、認証、複数プロジェクトの同時オープン

## 方針

- MCP は手書きの最小実装にする。トランスポートは stdio のみで、必要なメソッドは 4 つ。`rmcp` 等は非同期ランタイムごと依存に入るため使わない（M4-05 の WAV と同じく、小さい仕様は自前で持つ）。仕様の版は `initialize` で `2025-06-18` を返し、クライアントの要求版が違っても返す版で合わせる
- ツール定義は実行時に `schemars` のスキーマから作る。コマンドを足すとツール・CLI・リファレンスが同時に増え、手書きの一覧を持たない
- 入力は信頼境界。JSON の型不一致・未知の `type`・巨大な行（1 メッセージ 16 MiB 超は拒否）・不正な UTF-8 でパニックしない。コマンド適用は `Model::apply` の検証に任せ、MCP 層で別の検証をしない
- 標準出力は JSON-RPC 専用。ログは標準エラー
- 保存は明示。`tools/call` のたびに書かず、`save` ツール（と CLI の `edit`）でプロジェクトを書く。保存は一時ファイル → 置き換えで、途中で壊れたプロジェクトを残さない
- 保存先に履歴を残すかは `tsukumoe-format` の現状に合わせる。コマンド列の永続化が無ければ、本単位では追加せず、`save` は編集後のモデルだけを書く（undo 履歴はセッション内のみ）
- ツール名・スキーマは API。変更は `docs/reference/commands.md` の差分として見える

## 検査

- スキーマ: 全バリアントのスキーマが生成され、`type` の定数を持つ。各バリアントのサンプル JSON が自身のスキーマで検証でき（`jsonschema` はワークスペース既存）、`Command` に戻せる。バリアントを足してサンプルを足し忘れるとテストが落ちる（バリアントの網羅を `match` で強制する）
- MCP プロトコル: `initialize` → `tools/list` → `tools/call` の往復。未知のメソッド・未知のツール・不正な JSON・引数不足・巨大な行がエラーになりサーバは落ちない（変異テスト）
- 適用: `tools/call` で適用した結果が `Model::apply` を直接呼んだ結果と一致する。失敗コマンドでモデルが変わらない。`batch` が 1 回の `undo` で戻る。`undo` / `redo` が往復する
- CLI: `edit` で適用 → 保存 → 読み直しが一致する。失敗時にファイルが変わらない。`commands` の出力が `tools/list` と同じスキーマを返す
- 端から端: 子プロセスで `tsukumoe mcp` を起動し、標準入出力でコマンド列を流して保存、`tsukumoe validate` が通る
- リファレンス: 生成物がコミット済みの `docs/reference/commands.md` と一致する。全コマンドが載る
- 依存: 追加の外部依存は原則なし（`schemars` は既存。`tsukumoe-core` への導出は wasm ビルドで確認）

## 受入条件

- [ ] コマンド一覧と引数のスキーマを MCP から取得できる
- [ ] エージェント向けの API リファレンスがコマンド定義から自動生成される
- [ ] 現在の全コマンド（`Command` の全バリアント）を MCP / CLI から実行できる
- [ ] `tsukumoe-core` が `wasm32-unknown-unknown` でビルドできる

REQ-AI-001 の「UI からできる編集はすべて MCP / CLI からもできる」と、エディタ起動中のプロジェクトへの送信は、エディタ本体（M5-02 以降）ができてから満たす。そのため本単位では REQ-AI-001 全体は完了にしない。

## 決めたこと

- MCP は自前の最小実装（stdio、4 メソッド）。外部 MCP クレートは使わない
- ツールは `Command` のバリアントごと。単一の `apply(command)` ツールにはしない（エージェントが引数スキーマをバリアント単位で受け取れるため）
- スキーマは `schemars` で `Command` から生成する。手書きの一覧は持たない
- 保存は明示（`save`、CLI の `edit`）
- 計画時点で未確認: `schemars` の導出が `tsukumoe-core` の wasm サイズへ与える影響（`schema` フィーチャで囲めば無視できる）、`tsukumoe-format` にコマンド列を保存する場がある
