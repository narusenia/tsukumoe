# ADR-0012: 対応プラットフォーム

- 状態: 採用
- 日付: 2026-10-30

## 背景

VTuber 配信の大半は Windows で行われる。主開発機は macOS。

## 決定

- macOS（Metal）: エディタ・ホストとも主対象。Syphon
- Windows（D3D12 / Vulkan）: エディタ・ホストとも主対象。Spout2・仮想カメラ
- Linux（Vulkan）: ベストエフォート。CI でのビルドとテストだけ保証
- Web: ビューアのみ。エディタは対象外

## 結果

- CI は 3 OS で回す。画像差分テストはヘッドレスで Linux 上で行う
