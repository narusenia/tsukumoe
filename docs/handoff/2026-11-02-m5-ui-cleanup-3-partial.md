# 引き継ぎ: エディタ UI の見直し（M5-02g）③ の途中

- 計画書: `docs/implementation/M5-editor/ui-cleanup.md`。ブランチ `refactor/viewport-overlay-layout`、作業ツリーはクリーン（全コミット済み）
- 済み: ① 配置部品 / ② フライアウト・グリッド・Secondary 枠線 / ⑤ 文言（`ai_diff` 除外のみ見送り）/ ③ のタブ既定・バッジ（`Row.bound`）・ホバー連動（`ViewState::set_hover` / `hover`、`selection_overlay::paint_hover`）
- 未了（計画書の順）:
  1. ③ 残り: ツリー行のメッシュ小サムネイル（16px）、0.5 秒ホバーのカード（サムネ・頂点数・バインド先・マスク・描画順）。Part のバウンディングボックス強調
  2. ② 残り: グリッド作成後の +/− キーとホイール
  3. ④ 右クリック（ビューポート新設・ツリー行・パラメータ行・メッシュ / デフォーマツールの整理）
  4. ⑥ 数値欄統一・Shift+R・スプラッシュ・パイ設定・キーパネル整理・Animation 非表示・ブレンドモード
  5. ⑤ の `ai_diff` 除外: レイアウトが全パネルを必ず持つ設計（`workspace.rs` の `with_all_panels`）と衝突。「閉じたパネル」の扱いを決めてから
- 決定済み: 自動グリッド=不透明領域の縦横比、輪郭ライブプレビューは後続、要件・仕様書は更新しない
- 守ること: サブエージェント不使用、`RUSTC_WRAPPER=`、変更クレートだけ検証、cargo 並行させない、ja/en 両方、`.rs` にライセンス 2 行、スナップショット更新後は `.diff/.old/.new.png` を消す（`find ... -delete`。zsh の glob 不一致に注意）、`git checkout -- .` は自動判定で拒否される
- 実機確認は `gui-checklist.md`（① ② 枠線 ⑤ ③ 追記済み。`[x]` は利用者）
