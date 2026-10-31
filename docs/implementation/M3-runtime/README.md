# M3: ランタイム形式とバインディング

完了条件: ネイティブ・WASM・Unity で出力が一致し、PERF 目標を満たす。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M3-01 | [ランタイム形式（ゼロコピー）・仕様書・ファズテスト](runtime-format.md) | FMT-004〜006 | L | M1-03 | 完了 |
| M3-02 | [`tsukumoe-runtime`（モーション・表情・自動動作）](runtime-api.md) | RT-001 | M | M3-01 | 完了 |
| M3-03 | [`tsukumoe-ffi`（C ABI・cbindgen・C サンプル）](ffi.md) | RT-002 | M | M3-02 | 実装済み（CI 待ち） |
| M3-04 | [`tsukumoe-wasm` と npm パッケージ（WebGPU / WebGL2）](wasm.md) | RT-003 | M | M3-02 | 実装済み（CI 待ち） |
| M3-05 | [Unity パッケージ（別 repo）](unity.md) | RT-004 | L | M3-03 | 実装済み（Windows・性能は未確認） |
| M3-06 | [環境間の出力一致テスト](parity.md) | RT-007 | M | M3-04, M3-05 | 完了 |
| M3-07 | [性能目標の達成](performance.md)と Cubism SDK 比較（手元。比較は M2 の後） | PERF-002〜007 | L | M3-01 | 完了（PERF-006 は評価を並列にする前提で合格、PERF-007 は保留） |
