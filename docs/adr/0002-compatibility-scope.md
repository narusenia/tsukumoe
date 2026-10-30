# ADR-0002: 互換性の範囲

- 状態: 採用
- 日付: 2026-10-30

## 背景

Live2D 互換の層ごとに、難しさと法的リスクが大きく違う。公開 JSON 群は低リスク、moc3 は非公開バイナリ、cmo3 はエディタ形式でさらに難しい。

## 決定

- 読み込み: moc3、Live2D JSON 群（model3 / motion3 / exp3 / physics3 / pose3）、Inochi2D
- 書き出し: Tsukumoe 独自形式と Inochi2D
- moc3 は v1 に入れる。ただし法務ゲートを通す（ADR-0016）
- cmo3 / can3 は対象外。Live2D 形式への書き出しはしない

## 結果

- 既存 Live2D モデル資産を Tsukumoe ランタイムで再生できる
- Live2D の仕様変更に書き出し側が縛られない
