# 引き継ぎ: UI/UX 大改修（M5-02h）

## 決定事項
- 共通部品: `form_row` `collapsible` `group_rule` `button_wide`、11 件超の選択欄は検索付き
- コマンドパレットはパネルをやめ、カーソル位置のポップアップ（Mod+K）。`PANELS` は 13
- 割り当て可能な操作 49→68。履歴は日本語化。ja の英数字前後の半角スペースと括弧書きの説明は排除
- 変更点の表: `docs/implementation/M5-editor/ui-overhaul.md`

## 未完了（実機確認が必要）
- 右クリックメニューの実機の見た目
- ツリー行の親デフォーマ表示の右端のずれ（原因未特定）
- GPU ビューポート、パイメニューの実機表示
- 保存済みレイアウトに "palette" が残る場合の読み込み（未テスト）

## 次の一手
- `gui-checklist.md` の M5-02h を上から通し、ずれを洗い出す
- 検証は変更クレートだけ（tsukumoe-ui / app / i18n）。スナップショット更新は `UPDATE_SNAPSHOTS=1`、後で `*.diff.png` 等を削除

## 関連
- `crates/tsukumoe-ui/src/components/`, `crates/tsukumoe-app/src/panels/`, `editor/settings_dialog.rs`, `actions.rs`
- 最新コミット: e8ef5fb（MCP 計画書。他セッション分）、1a4538e（ショートカット絞り込み）
