# M2: Live2D 互換

完了条件: 自作の Live2D モデルが画像差分テストを通る。

moc3 の単位（M2-03 以降）は法務ゲート（ADR-0016）を通るまで着手しない。待つあいだは M2-01 / M2-02 を進める。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M2-01 | Live2D JSON 群（model3 / motion3 / exp3 / physics3 / pose3 / cdi3 / userdata3）の読み込み | COMPAT-004 | M | M1-08 | 未着手 |
| M2-02 | 法務ゲート（着手前確認）と ADR 記録 | COMPAT-001 | S | — | 未着手 |
| M2-03 | 検証用の自作 Live2D モデル群と期待画像の作成 | COMPAT-005, PERF-001 | M | M2-02 | 未着手 |
| M2-04 | moc3 のクリーンルーム解析と形式ノート（出所付き） | COMPAT-002 | L | M2-02 | 未着手 |
| M2-05 | `tsukumoe-compat` の `moc3` feature で読み込み | COMPAT-002, COMPAT-003 | L | M2-04 | 未着手 |
| M2-06 | Live2D 互換モードと画像差分テストの CI 組込み | CORE-009, COMPAT-005 | M | M2-03, M2-05 | 未着手 |
