# M5-05c-2 糊（グルー）の作成・編集

- REQ: REQ-EDIT-003（方針は [live2d-parity.md](live2d-parity.md)）
- 状態: 計画中
- 依存: M5-05c-1、M5-06a〜c（キーフォーム）
- クレート: `tsukumoe-core`（コマンド）、`tsukumoe-cli`、`tsukumoe-app`（ツール・インスペクタ）。形式と評価は変えない

## 前提（調査済み）

- モデル `Glue { mesh_a, mesh_b, nodes: Vec<GlueNode{index_a, index_b, weight}>, intensity, keyforms: Option<KeyformGrid<f32>> }`、`.tmod`（0x23〜0x25）、`.tsukumoe` の DTO は既にある
- 評価（`eval.rs`）は糊を反映済み。各節点で 2 頂点を `weight` で決まる合流点へ `intensity` の割合だけ寄せる。キーフォームがあれば `intensity` を補間する
- 編集コマンドはない。メッシュの頂点数を変える操作は、糊が使うメッシュでは既に拒否される
- キーフォームの共有・バインドの操作（06a）は、メッシュ・ワープ・回転だけが対象で、糊はまだ対象外

## 決めること（案）

- **コマンド**（`command.rs`）
  - `AddGlue { name, mesh_a, mesh_b, nodes, intensity }`。検証: 2 メッシュが別、頂点番号が範囲内、`weight` が有限で 0〜1、`intensity` が有限。取り消しは 1 ステップ
  - `RemoveGlue { name }`
  - `SetGlueIntensity { name, intensity }`（キーフォームなしの基本値）
  - キーフォーム付きの強さ: 既存の `Bind` 系と `SetXxxKeyform` に倣い、糊を要素種別に加える（`ElementRef` に糊を足す案。実装時に実際の型を確かめる）。キー位置で強さを変える `SetGlueKeyform { name, key, intensity }`
  - 節点の差し替え・重みの編集は作らない（削除して作り直す）
- **作り方（UI）**: 「糊」ツール。メッシュ A の頂点を選び、メッシュ B の頂点を選んで確定。A と B の選択を、近い順の 1 対 1 で組にする。重みは 0.5 固定
- **インスペクタ**: 糊を選ぶと、2 メッシュの名前・節点数・強さ（スライダ）。パラメータにバインド済みなら、06b と同じくキー位置でだけ強さを編集
- **ツリー**: 糊を一覧に出す（選択・削除・名前変更）
- **ビューポート**: 選択中の糊の節点を、2 頂点を結ぶ線で表示

## 段階

1. コマンド（基本値）とテスト、CLI・アプリのコマンド名、コマンド一覧の再生成
2. キーフォーム対応（バインド・`SetGlueKeyform`）
3. ツール・インスペクタ・ツリー・ビューポート表示

## 範囲外

- 節点ごとの重みの編集、節点の追加・削除
- 頂点の自動対応づけ（距離しきい値で自動で組む）
- 糊を使うメッシュの頂点数を変える操作の解禁
