# M4: VTuber

完了条件: 実際の配信で使える。

| ID | 単位 | REQ | 規模 | 依存 | 状態 |
|---|---|---|---|---|---|
| M4-01 | [トラッキング入力アダプタ](tracking-input.md)（VMC / OSC・iFacialMocap・MediaPipe） | TRK-001 | M | — | 完了 |
| M4-02 | [標準パラメータマッピング](standard-mapping.md) | TRK-002 | M | M4-01, M3-02 | 完了 |
| M4-03a | [ホスト: トラッキング配線・透過ウィンドウ](host-live.md) | TRK-003, TRK-004 | M | M1-11, M4-01, M4-02 | 完了 |
| M4-03b | [ホスト: Syphon 出力（macOS）](host-syphon.md) | TRK-003 | M | M4-03a | 完了 |
| M4-03c-1 | [ホスト: Spout2 出力（Windows）](host-spout.md) | TRK-003 | M | M4-03a | 完了 |
| M4-03c-2a | [ホスト: 仮想カメラ（Windows）メディアソース DLL・登録](host-virtualcam.md) | TRK-003 | L | M4-03a | 計画中 |
| M4-03c-2b | [ホスト: 仮想カメラ（Windows）フレームチャネル・出力](host-virtualcam.md) | TRK-003 | M | M4-03c-2a | 計画中 |
| M4-03c-3 | ホスト: 仮想カメラ（macOS） | TRK-003 | M | M4-03a | 見送り |
| M4-04 | [推論プロバイダ抽象](inference-provider.md) | AI-003 | M | — | 完了 |
| M4-05 | [リップシンク](lipsync.md) | AI-008 | M | M4-02, M4-04 | 完了 |

M4-03c-3（macOS 仮想カメラ）は見送り。macOS の仮想カメラは CoreMediaIO Camera Extension（システム拡張）が要り、Apple Developer の署名・公証・利用者の承認が必須。Syphon → OBS の仮想カメラで用は足りる。必要になったら再検討する。
