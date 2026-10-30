# データモデル

関連: REQ-CORE-002〜005, REQ-CORE-009, REQ-COMPAT-004, REQ-COMPAT-006

状態: 骨子。各要素のフィールド定義は M1 で詰める。

## 要素

| 要素 | 内容 |
|---|---|
| Parameter | ID・表示名・範囲・既定値・反復（角度など）・グループ |
| Part | 不透明度と描画順をまとめる単位。親子を持つ |
| ArtMesh | 頂点・UV・三角形・テクスチャ・ブレンドモード・マスク・乗算色 / スクリーン色・ボーンウェイト |
| WarpDeformer | ベジェ格子（行 × 列）。子を持つ |
| RotationDeformer | 原点・角度・拡大率・反転。子を持つ |
| Glue | 2 つの ArtMesh の頂点を重み付きで結ぶ |
| Bone | 親子・ローカル変換。ArtMesh の頂点ウェイトから参照される |
| KeyformGrid | 対象（メッシュ / デフォーマ / ボーン / 描画順 / 不透明度 / 色）ごとの、パラメータ格子上のキーフォーム |
| BlendShape | 基本形に加算する差分形。重みはパラメータで駆動する |
| Driver | パラメータ式。出力パラメータ 1 つと式 |
| PhysicsRig | 振り子（physics3 互換）と物理チェーン |
| Motion / Expression / Pose | 再生側データ（motion3 / exp3 / pose3 相当） |

## Cubism 機能との対応

| Cubism | Seimei | 備考 |
|---|---|---|
| ArtMesh | ArtMesh | |
| Warp Deformer | WarpDeformer | |
| Rotation Deformer | RotationDeformer | |
| Parameter / キーフォーム（多重線形補間） | Parameter / KeyformGrid | |
| Blend Shape（Cubism 5） | BlendShape | |
| Glue | Glue | |
| Clipping Mask（反転含む） | ArtMesh のマスク参照 | |
| 乗算色 / スクリーン色 | ArtMesh の色 | |
| 描画順（キーフォーム付き） | KeyformGrid（描画順） | |
| Part / Part 不透明度 | Part | |
| physics3 | PhysicsRig（振り子） | |
| motion3 / exp3 / pose3 | Motion / Expression / Pose | |
| — | Bone / Driver / 物理チェーン | Seimei 独自。互換モードでは無効 |

## スキニングとデフォーマの適用順

TODO（M1）: 「ボーンでスキニング → 親デフォーマで変形」か「デフォーマの後にボーン」かを決め、ADR に記録する。
