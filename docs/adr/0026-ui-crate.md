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
- テーマは `Theme`（色 10 種 + 寸法 `Metrics`）。既定は高密度（文字 12px・コントロール高 24px・余白 4px 刻み・影なし、境界線と面の明度差で階層を出す）
- `set_mode`（ライト / ダーク / システム）で切り替え、`wgpui-base` の外観も同期する。`Theme::from_json` で JSON を読む（`{"appearance": "dark"|"light", "colors": {"accent": "#rrggbb"}}`、省いた色は組み込みテーマで補う、未知のキーは拒否）。ホットリロードは作らない
- `tsukumoe-host` と `tsukumoe-app` は `tsukumoe-ui` を使い、`wgpui-base` を直接使わない

## 結果

- 見た目が一か所で決まり、`wgpui-component` を依存に含めずに済む
- 部品を自前で保守する。必要な部品だけ作るので、増やすときは gallery に足す
