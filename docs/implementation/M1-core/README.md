# M1: core・参照レンダラ・最小ビューア

完了条件: 基準モデル（Tsukumoe 形式）が表示され、パラメータで動く。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M1-01 | [編集用データモデル](edit-model.md)（Cubism 機能の全要素） | CORE-002 | L | M0-02 | 完了 |
| M1-02 | [編集コマンドと undo / redo](edit-commands.md) | CORE-007 | M | M1-01 | 完了 |
| M1-03 | [評価用モデルへのコンパイルと評価パイプライン](eval-pipeline.md)（キーフォーム・デフォーマ・グルー・ブレンドシェイプ） | CORE-001, CORE-008 | L | M1-01 | 完了 |
| M1-04 | [パラメータ式](driver-expressions.md) | CORE-004 | M | M1-03 | 完了 |
| M1-05 | [ボーンとスキニング](bone-skinning.md)（適用順は [ADR-0021](../../adr/0021-skinning-order.md)） | CORE-003 | M | M1-03 | 完了 |
| M1-06 | [物理](physics.md)（振り子と物理チェーン、[ADR-0022](../../adr/0022-physics-stage.md)） | CORE-005 | M | M1-03 | 完了 |
| M1-07 | [描画リスト](draw-list.md) | CORE-006 | M | M1-03 | 完了 |
| M1-08 | JSON プロジェクト形式（zip / 展開ディレクトリ）と JSON Schema | FMT-001〜003, FMT-005 | M | M1-01 | 未着手 |
| M1-09 | 描画規則の仕様化と wgpu 参照レンダラ（ヘッドレス出力を含む） | RT-005, RT-006 | L | M1-07 | 未着手 |
| M1-10 | `tsukumoe-cli`: 形式変換・検証・ヘッドレスレンダ・画像差分 | AI-002 | M | M1-08, M1-09 | 未着手 |
| M1-11 | `tsukumoe-host` の芽: パラメータスライダ付きビューア（gpui-pre + gpui-kit） | TRK-003, UI-001, UI-002 | M | M1-09 | 未着手 |
| M1-12 | criterion ベンチマークと CI での回帰検知 | PERF-002, PERF-009 | S | M1-03 | 未着手 |
| M1-13 | 基準モデルの生成 CLI（パーツ表 → Tsukumoe 形式 JSON、ADR-0017） | PERF-001 | M | M1-08, M1-10 | 未着手 |
