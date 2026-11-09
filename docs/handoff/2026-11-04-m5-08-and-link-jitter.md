# 引き継ぎ: M5-08 計画書とリンク時の震え

- ブランチ `feat/motion-expression-editor`（`main` から切った直後。作業ツリーはクリーン）。M5-09 は `main` にマージ済み（push はしていない）
- 決定済み: Animation ワークスペースの `Workspace::SHOWN` 解除は M5-08 ①の完了後。リップシンク取り込みは M5-08 に含める
- 次の一手: 計画書 `docs/implementation/M5-editor/M5-08-*.md` を新設。先に `tsukumoe-core/src/tmod/`（`MotionDef` `CurveDef` `SegmentDef`）と `tsukumoe-runtime` の `MotionPlayer` `ExpressionPlayer` を調べる。分割案は ①モーションのデータ編集とタイムライン ②カーブエディタ（線形・ベジェ・ステップ・逆ステップ）③表情（パラメータ差分セット）。README の M5-08 行にリンクと状態を入れ、`gui-checklist.md` に追記
- 未解決（リンク時の震え）: パラメータをパッドでリンクし、ワープを (1,0) で変形したあと (0,0) へドラッグで戻すと、ビューポートの絵が震える。選択はワープ、モデルに物理・ドライバは無い
  - 除外済み: モデル評価の補間（2 軸ワープで滑らかと確認）、パッドの値（ポインタ固定で安定）、カメラのフィット（`scene.bounds` は作成時固定）、`Frame::advance` のスキップ
  - 未確認: パッドの吸着（`SNAP_POINTS * 1.5` = 9px）の入り切り、`tsukumoe-gpu` の描画パス
  - 実機での切り分け: ①Shift を押してドラッグ（吸着なし）②リンクを外した 2 本のスライダーで同じ操作 ③選択ツールで同じ操作
- 見つけたずれ: ワープ選択中の選択ボックスは `selection_overlay.rs` の `Shape::of`（既定値）で描き、スライダー位置と食い違う（`of_at` が正）。直すなら小さい修正
- 守ること: `docs/handoff/2026-11-04-m5-09-export.md` の「守ること」と同じ（サブエージェント不使用、`RUSTC_WRAPPER=`、変更クレートだけ検証、ja / en 両方、コミットは lefthook を通す）
