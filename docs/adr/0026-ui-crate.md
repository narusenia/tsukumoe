# ADR-0026: UI 部品は `tsukumoe-ui` に自前で作り、gallery で確認する

- 状態: 採用
- 日付: 2026-10-30

## 背景

`wgpui-component`（約 1.9 万行）は見た目まで決まっていて大きい。Tsukumoe は見た目を自前のテーマで統一したい（REQ-UI-004）。`wgpui-base` は見た目を持たない挙動層（操作・フォーカス・アクセシビリティ・状態）で、この用途に合う。

## 決定

- 新クレート `tsukumoe-ui` を GPUI 境界の上に置く。依存は `wgpui` と `wgpui-base` だけ
- 中身はデザイントークン（色・余白・角丸・文字サイズ、ライト / ダーク）と、それを使うスタイル付きコンポーネント。`wgpui-base` の部品を包み、見た目はトークンから決める
- M1 で作るのは Slider・Button・Label・Panel・Switch。エディタ用（Dock・Tree・Table など）は M5-02 で足す
- `tsukumoe-ui` の example `gallery` に、各部品の状態（通常・hover・disabled、ライト・ダーク）を並べる。目視確認の場とし、CI は example のビルドまで
- `tsukumoe-host` と `tsukumoe-app` は `tsukumoe-ui` を使い、`wgpui-base` を直接使わない

## 結果

- 見た目が一か所で決まり、`wgpui-component` を依存に含めずに済む
- 部品を自前で保守する。必要な部品だけ作るので、増やすときは gallery に足す
