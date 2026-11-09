# MCP の取り込み・書き出しツール

- REQ: REQ-AI-001, REQ-AI-008, REQ-EDIT-001, REQ-EDIT-007
- 単位: M5-01g
- 状態: 計画中
- 依存: M5-01e, M5-03, M5-09a
- クレート: `tsukumoe-cli`（MCP のツール）、`tsukumoe-psd`・`tsukumoe-ai`・`tsukumoe-format`（既存の処理）

## 目的

CLI のサブコマンドやエディタのメニューにしかない取り込み・書き出しを、MCP のセッションの中からできるようにする。開いているモデルを閉じずに、PSD の差し替えやリップシンクの取り込みまで続けて行えるようにする。

## 範囲

- `import_psd { psd, plan_only?, missing?: keep|remove, map? }`: 既定は計画だけを返す（追加・更新・欠けるレイヤーの一覧）。`apply: true` で開いているモデルに適用する（1 undo 段）。CLI の `tsukumoe psd import` と同じ処理
- `lipsync { wav, name, fps?, tolerance? }`: WAV からモーションを作り、開いているモデルにモーションとして足す（`motion_import` と同じく、モデルにないパラメータの曲線は除いて件数を返す）
- `import_motion { path }`: モーションの JSON を取り込む
- `export_runtime { out }`: ランタイム形式で書き出す（CLI の `convert` と同じ）
- `export_frames { motion, out_dir, fps?, size?, video? }`: モーションを連番 PNG（`ffmpeg` があれば動画）に書き出す
- 範囲外: 取り込み元ファイルの選択 UI、HTTP 越しの受け渡し

## 方針

- ファイルのパスはツールの引数で受け取る。読み書きできる場所は、開いているプロジェクトのディレクトリと、サーバ起動時に `--allow-dir` で許した場所に限る（エージェントの入力は信頼境界）
- PSD は計画 → 適用の 2 段を既定にする。いきなり適用しない
- `frames_export` は `tsukumoe-gpu` と `ffmpeg` を使うため `tsukumoe-edit` には置けない。置き場（`tsukumoe-gpu` の上の薄いモジュールにする／`tsukumoe-cli` に移してエディタから呼べる形にする）を本単位で決め、エディタとの重複を残さない
- 取り込みのコマンド化は既存の処理（`tsukumoe-psd` の計画、`motion_import`）をそのまま使う

## 検査

- `import_psd` の計画が CLI の `--plan` の出力と一致する。適用結果が `--apply` と一致し、1 回の `undo` で戻る
- `lipsync` の結果が CLI の `tsukumoe lipsync` の出力を取り込んだものと一致する
- 許可外のパス（`..` を含む・絶対パスで外）がツールエラーになる
- `export_frames` の枚数と画像が、エディタの書き出しと一致する

## 受入条件

- [ ] PSD の取り込み・リップシンク・モーションの取り込みを MCP からできる
- [ ] ランタイム形式・連番画像・動画を MCP から書き出せる
- [ ] 許可した場所の外を読み書きしない
