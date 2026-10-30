# 基準モデルの仕様

- REQ: REQ-PERF-001（規模）, REQ-COMPAT-005（互換検証）, REQ-INFRA-003（CC-BY 4.0）
- 状態: 仕様のみ。制作は未着手
- 依存: なし

性能目標（REQ-PERF-002〜007）と互換検証の共通の物差しにする、Tsukumoe 自作の公開モデル。キャラクター・画像・リグはすべて自作し、CC-BY 4.0 で同梱する。

## 規模

| 項目 | 目標 | 内訳の目安 |
|---|---|---|
| ArtMesh | 150 | 顔 30 / 目 24 / 口 8 / 髪 40 / 体 20 / 服 20 / 小物 8 |
| 頂点 | 30,000 | 1 メッシュ平均 200。髪・服は多め、目・口は変形が大きいので密 |
| パラメータ | 100 | 標準 30 + 揺れ物・小物・表情の独自 70 |
| デフォーマ | 80 | ワープ 60 / 回転 20 |
| 物理 | 20 系統 | 前髪・横髪・後髪・服・小物 |

## 標準パラメータ（30 前後）

Live2D の慣習 ID を使う。トラッキングの既定マッピング（REQ-TRK-002）がそのまま効くようにするため。

| 区分 | ID |
|---|---|
| 顔の角度 | `ParamAngleX` / `ParamAngleY` / `ParamAngleZ` |
| 目 | `ParamEyeLOpen` / `ParamEyeROpen` / `ParamEyeLSmile` / `ParamEyeRSmile` / `ParamEyeBallX` / `ParamEyeBallY` |
| 眉 | `ParamBrowLY` / `ParamBrowRY` / `ParamBrowLX` / `ParamBrowRX` / `ParamBrowLAngle` / `ParamBrowRAngle` / `ParamBrowLForm` / `ParamBrowRForm` |
| 口・頬 | `ParamMouthForm` / `ParamMouthOpenY` / `ParamCheek` |
| 体 | `ParamBodyAngleX` / `ParamBodyAngleY` / `ParamBodyAngleZ` / `ParamBreath` |
| 揺れ物（物理の出力） | `ParamHairFront` / `ParamHairSide` / `ParamHairBack` |

独自パラメータ（70）は、衣装の揺れ・小物・表情差分（照れ・驚きなど）・腕の動きに使う。

## 含める機能

互換検証と性能計測の両方に効くよう、Cubism の機能を一通り使う。

- ワープ・回転デフォーマの多段階層、グルー、クリッピングマスク（反転を含む）
- 乗算色・スクリーン色、ブレンドモード（加算・乗算）、描画順のキーフォーム
- 2 次元以上のキーフォーム（顔の角度 XY × 目の開閉など）
- 物理 20 系統（振り子）
- Tsukumoe 独自機能版: ボーン 12 本、パラメータ式 10 本、物理チェーン 4 系統（髪）。これは Tsukumoe 形式版だけに入れる

## 成果物

| 版 | 用途 | 作り方 |
|---|---|---|
| Tsukumoe 形式（独自機能なし） | M1 の表示・性能計測 | 生成スクリプト（M1-13） |
| Tsukumoe 形式（独自機能あり） | ボーン・式・チェーンの検証 | 上の版に追加 |
| Live2D 形式（moc3 + JSON 群） | 互換検証（M2）と Cubism SDK 比較（M3） | 自分で Cubism Editor から書き出す（ADR-0016 の「自作モデル」） |

## Tsukumoe 形式版の作り方

生成スクリプトで作る（[ADR-0017](../../adr/0017-reference-model-generator.md)）。パーツ表から JSON を生成する CLI を `tsukumoe-cli` に置く（M1-13）。
