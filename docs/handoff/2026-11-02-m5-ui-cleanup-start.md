# 引き継ぎ: エディタ UI の見直し（M5-02g）の着手

- 計画書: `docs/implementation/M5-editor/ui-cleanup.md`（所見・決定事項・着手順 ①〜⑥・未決。まずこれを読む）
- ブランチ: `refactor/viewport-overlay-layout`（`main` から作成）。**未コミット**の作業ツリーに ① の途中がある
- 作業ツリーの状態（clippy は通る。`overlay` 関連のユニットテスト 11 件は通る。**スナップショットテストは未実行・未更新**）:
  - 新規 `crates/tsukumoe-app/src/panels/overlay.rs`: `boxed` / `options_stack` / `tools` / `status` / `help`
  - `toolbar.rs` を `overlay::tools` へ（`TOP` を削除）
  - `mesh_overlay.rs` `deformer_overlay.rs` `proportional_overlay.rs`: 箱の関数が `ui` を受け取る形に（`caption` / `key_caption` は削除）
  - `viewport.rs`: ヒントとキー表示を `overlay::help`（左下）、ソロバッジを `overlay::status`（上中央）、右上の箱を `options_stack` に
  - `tests/snapshots.rs`: 新しい呼び出しに更新済み。画像は未更新
- 次の一手:
  1. `UPDATE_SNAPSHOTS=1 cargo test -p tsukumoe-app --test snapshots` で見た目が変わった画像（`mesh_edit` `deformer` `proportional` `keyform_caption` `toolbar`）を更新し、目で確認。`.diff.png` `.new.png` を消す
  2. ① をコミット（論理単位ごとに分ける。英語 1 行・prefix・小文字・帰属行なし）。`git add -A` は使わない
  3. ② 以降は計画書の順
- 引き継ぎで守ること（前回の引き継ぎ `2026-11-02-m5-glue-settings-done.md` と同じ）:
  - **サブエージェントは使わない**。検証は `RUSTC_WRAPPER=` 付き。cargo を並行させない
  - 検証は変更したクレートだけ。フル一式は節目だけ
  - コマンドを足したら `cargo run -q -p tsukumoe-cli -- docs commands > docs/reference/commands.md`
  - ja / en の両方に文言を足す。`.rs` の先頭にライセンス 2 行
  - 実機確認の項目は `gui-checklist.md` に足す（`[x]` は利用者）
- 利用者に確認する点: 「自動」グリッド推定の式、輪郭のライブプレビューを ② に入れるか、要件・仕様書を更新するか（計画書の「未決」）
- 参考（監査で見た画面）: `crates/tsukumoe-app/tests/snapshots/*.png`
