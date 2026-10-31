# M4: VTuber

完了条件: 実際の配信で使える。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M4-01 | [トラッキング入力アダプタ](tracking-input.md)（VMC / OSC・iFacialMocap・MediaPipe） | TRK-001 | M | — | 完了 |
| M4-02 | 標準パラメータマッピング | TRK-002 | M | M4-01, M3-02 | 未着手 |
| M4-03 | ホスト: 透過ウィンドウ・Spout2 / Syphon・仮想カメラ | TRK-003, TRK-004 | L | M1-11 | 未着手 |
| M4-04 | [推論プロバイダ抽象](inference-provider.md) | AI-003 | M | — | 未着手 |
| M4-05 | リップシンク | AI-008 | M | M4-02, M4-04 | 未着手 |
