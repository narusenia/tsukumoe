# 引き継ぎ: 糊・設定ダイアログ完了後（M5-06e 着手前）

- 状態: `main` = 糊（M5-05c-2）と設定ダイアログ（M5-02f）をマージ済み。残したブランチ（削除は確認）: `feat/glue-edit`、`feat/settings-dialog`、`feat/mesh-symmetry-lasso-weld`、`feat/mesh-mask-color-order`、`feat/warp-bezier-interpolation`
- **サブエージェントは使わない**。検証は `RUSTC_WRAPPER=` 付き。cargo を並行させない。長いテストはバックグラウンド。`UPDATE_SNAPSHOTS=1 cargo test -p <crate> --test snapshots <name>` のあと `.diff.png` `.new.png` を消す。コマンドを足したら `cargo run -q -p tsukumoe-cli -- docs commands > docs/reference/commands.md`
- 今回入れたもの（実機確認は `docs/implementation/M5-editor/gui-checklist.md`。利用者が `[x]` にする）: 糊（コマンド・キーフォーム・ツリー・インスペクタ・節点線・右クリックメニュー）、設定ダイアログ（一般・操作・ショートカット・表示・編集・ファイル）、インスペクタのリアルタイム更新、色のカラーピッカー、順序ボタンのアイコン化、ソロ表示、パイメニューのフォーカス修正、長いパラメータ名のレイアウト
- 設計メモ:
  - 設定は `editor/options.json`（`options.rs`）。しきい値・半径・背景はスレッドローカル `options::tuning()` を UI のスレッドで毎フレーム設定（テストのスレッドは既定値）。履歴の上限は `Dispatcher::set_history_limit`。自動保存は `<名前>.autosave`（本体・パス・未保存状態は不変）
  - リアルタイム更新: インスペクタが `Requests.live` を返し、`Editor::submit_live` が `Dispatcher::amend_labeled`（直前の 1 ステップを取り消して置き換え）で 1 ドラッグ = undo 1 回にする
  - ソロ: `ViewState.solo`。`Scene::new_solo` がコンパイル済みの `mesh_hidden` に足す。モデル・履歴は変えない・保存しない
  - 単独キー（`Q` など）は `text_edit_focused()` のときだけ止める（`egui_wants_keyboard_input` は何かにフォーカスがあるだけで真になる）
  - 自動サイズのウィンドウ内で幅いっぱいの欄を置くと毎フレーム広がる: 中身の幅を固定する（`set_width`）
- 既知の割り切り: `mesh.glue-source` の指定頂点は画面に出ない／エディタのテーマは内蔵のみ（ユーザーテーマは別アプリのホストだけ）／自動保存の起動時復元なし／要件・仕様書は未更新
- 未決（利用者に確認）: 要件・仕様書に書くか（糊・設定ダイアログ・ソロ・ベジェ・左右対称・投げ縄・結合・マスク・色、`mesh_edit` の新関数、`Command` の追加）。M5-02e（表示補助）と設定の「表示」タブの住み分け
- 次の作業: M5-06e（パラメータの整理）→ 05a・07・08・09・09a → 02e・03a・10
- 手順: ブランチ → 計画書の確認 → 実装 → 検証 → `main` へ `--no-ff` マージ。コミットは英語 1 行・prefix・小文字・帰属行なし。`git add -A` は利用者が編集中の `gui-checklist.md` を巻き込むので、対象を指定する。macOS では `sed -i ''`、zsh は glob が一致しないと `rm` 全体が止まる
- 制約: moc3 クリーンルーム（ADR-0016）。検証用 PSD は `~/Downloads/DeepSeek-chan/psd/`（再配布不可）。`.rs` は先頭にライセンス 2 行
