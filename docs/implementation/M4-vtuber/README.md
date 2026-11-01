# M4: VTuber

完了条件: 実際の配信で使える。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M4-01 | [トラッキング入力アダプタ](tracking-input.md)（VMC / OSC・iFacialMocap・MediaPipe） | TRK-001 | M | — | 完了 |
| M4-02 | [標準パラメータマッピング](standard-mapping.md) | TRK-002 | M | M4-01, M3-02 | 完了 |
| M4-03a | [ホスト: トラッキング配線・透過ウィンドウ](host-live.md) | TRK-003, TRK-004 | M | M1-11, M4-01, M4-02 | 着手中 |
| M4-03b | ホスト: Syphon 出力（macOS） | TRK-003 | M | M4-03a | 未着手 |
| M4-03c | ホスト: 仮想カメラ・Spout2（Windows / macOS） | TRK-003 | L | M4-03a | 未着手 |
| M4-04 | [推論プロバイダ抽象](inference-provider.md) | AI-003 | M | — | 完了 |
| M4-05 | [リップシンク](lipsync.md) | AI-008 | M | M4-02, M4-04 | 完了 |
