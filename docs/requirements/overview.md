# 要件定義書: Tsukumoe

## プロジェクト概要

Tsukumoe は Live2D Cubism の代替となる 2D リグ・アニメーション基盤である。
モデルを作るエディタと、各環境でモデルを再生するランタイム SDK の両方を提供する。

- **高速**: GPU 非依存のデータ指向な評価パイプラインで、Cubism SDK の 2 倍以上の評価速度を狙う
- **AI ネイティブ**: エディタの全操作をコマンドとして公開し、AI エージェントから操作・検証できる。リグ支援とリップシンクを内蔵する
- **互換性**: 既存の Live2D モデル（moc3 と JSON 群）と Inochi2D モデルをそのまま読み込める
- **オープン**: ランタイム・エディタともに Apache-2.0 OR MIT。ランタイム形式の仕様を公開する

開発はランタイムから始め、エディタは後から載せる（`docs/implementation/roadmap.md`）。
設計判断の経緯は `docs/adr/` に記録する。

## スコープ

### 対象範囲（v1）

- リグ評価コア（Cubism の機能をすべて含み、ボーン・パラメータ式・物理チェーンを追加したもの）
- JSON プロジェクト形式と、仕様を公開したランタイムバイナリ形式
- moc3・Live2D JSON 群（model3 / motion3 / exp3 / physics3 / pose3）の読み込み、Inochi2D の読み書き
- ランタイム: C ABI / WASM（WebGPU・WebGL2）/ Unity
- VTuber 用途: トラッキング入力層、標準パラメータマッピング、最小の配信ホスト
- AI: エージェント操作 API（MCP / CLI）、リグ支援、リップシンク
- エディタ（PSD 読み込みからランタイム形式の書き出しまで）
- 対応 OS: macOS / Windows（主対象）、Linux（ベストエフォート）

### 対象外

- `.cmo3` / `.can3`（Cubism Editor 形式）の読み書き
- Live2D 形式への書き出し
- ランタイム形式の暗号化・難読化
- 顔追跡の推論モデルの自前学習
- 1 枚絵からのパーツ分割・補完（v2）
- テキストからのモーション生成（v1.x）
- IK・コンストレイント、擬似 3D、物理の衝突（v1.x）
- RBF 等の非格子パラメータ空間（v2）
- GPU（compute）による変形（性能目標未達時のみ検討）
- 本格的な配信ホストアプリ（v2。v1 中は必要に応じて育てる）
- Godot / Unreal / モバイル専用バインディング（v1.x）
- 収益化の仕組み

## 要件一覧

| ID | タイトル | 優先度 | ステータス |
|----|---------|--------|-----------|
| REQ-CORE-001 | GPU 非依存の固定段評価パイプライン | Must | Draft |
| REQ-CORE-002 | Cubism の機能をすべて含むリグモデル | Must | Draft |
| REQ-CORE-003 | ボーンと頂点ウェイトによるスキニング | Must | Draft |
| REQ-CORE-004 | パラメータ式（ドライバ） | Must | Draft |
| REQ-CORE-005 | 物理演算（振り子互換と物理チェーン） | Must | Draft |
| REQ-CORE-006 | 描画リストの出力 | Must | Draft |
| REQ-CORE-007 | 編集コマンドと undo | Must | Draft |
| REQ-CORE-008 | データ指向レイアウトとフレーム中の無アロケーション | Must | Draft |
| REQ-CORE-009 | Live2D 互換モード | Should | Draft |
| REQ-FMT-001 | JSON プロジェクト形式（zip コンテナ） | Must | Draft |
| REQ-FMT-002 | 展開ディレクトリ形式 | Must | Draft |
| REQ-FMT-003 | JSON Schema の公開 | Must | Draft |
| REQ-FMT-004 | ゼロコピーのランタイムバイナリ形式 | Must | Draft |
| REQ-FMT-005 | 形式のバージョニングと機能フラグ | Must | Draft |
| REQ-FMT-006 | ランタイム形式仕様の公開 | Must | Draft |
| REQ-COMPAT-001 | moc3 読み込みの法務ゲート | Must | Draft |
| REQ-COMPAT-002 | moc3 のクリーンルーム実装 | Must | Draft |
| REQ-COMPAT-003 | moc3 機能の分離可能な構造 | Must | Draft |
| REQ-COMPAT-004 | Live2D JSON 群の読み込み | Must | Draft |
| REQ-COMPAT-005 | 画像差分による同等表示の検証 | Must | Draft |
| REQ-COMPAT-006 | Inochi2D の読み書き | Should | Draft |
| REQ-RT-001 | 再生用ランタイム API | Must | Draft |
| REQ-RT-002 | C ABI | Must | Draft |
| REQ-RT-003 | WASM と JS/TS パッケージ | Must | Draft |
| REQ-RT-004 | Unity パッケージ | Must | Draft |
| REQ-RT-005 | wgpu 参照レンダラ | Must | Draft |
| REQ-RT-006 | レンダラの差し替え | Must | Draft |
| REQ-RT-007 | 環境間の出力一致 | Must | Draft |
| REQ-TRK-001 | トラッキング入力アダプタ | Must | Draft |
| REQ-TRK-002 | 標準パラメータマッピング | Must | Draft |
| REQ-TRK-003 | 最小の配信ホスト | Must | Draft |
| REQ-TRK-004 | 配信ホストを育てられる構造 | Should | Draft |
| REQ-AI-001 | 全編集操作のエージェント公開（MCP / CLI） | Must | Draft |
| REQ-AI-002 | ヘッドレスのレンダと検証 | Must | Draft |
| REQ-AI-003 | 推論プロバイダの抽象化 | Must | Draft |
| REQ-AI-004 | AI 出力を編集コマンドとして適用 | Must | Draft |
| REQ-AI-005 | 自動メッシュ生成 | Must | Draft |
| REQ-AI-006 | デフォーマ配置とパラメータ雛形の生成 | Must | Draft |
| REQ-AI-007 | 物理の自動設定 | Should | Draft |
| REQ-AI-008 | リップシンク | Must | Draft |
| REQ-EDIT-001 | PSD 読み込み | Must | Draft |
| REQ-EDIT-002 | メッシュ編集 | Must | Draft |
| REQ-EDIT-003 | デフォーマとボーンの編集 | Must | Draft |
| REQ-EDIT-004 | パラメータとキーフォームの編集 | Must | Draft |
| REQ-EDIT-005 | 物理の設定 | Must | Draft |
| REQ-EDIT-006 | モーションと表情の編集 | Must | Draft |
| REQ-EDIT-007 | 書き出し | Must | Draft |
| REQ-UI-001 | egui を UI の土台に採用 | Must | Draft |
| REQ-UI-002 | egui 系と wgpu の同一版固定 | Must | Draft |
| REQ-UI-003 | ランタイム系クレートの UI 非依存 | Must | Draft |
| REQ-UI-004 | UI 部品とテーマを自前で持つ | Should | Draft |
| REQ-UI-005 | UI の多言語対応 | Should | Draft |
| REQ-PERF-001 | 基準モデルの定義と同梱 | Must | Draft |
| REQ-PERF-002 | ネイティブの評価時間 | Must | Draft |
| REQ-PERF-003 | WASM の評価時間 | Must | Draft |
| REQ-PERF-004 | 参照レンダラの描画時間 | Must | Draft |
| REQ-PERF-005 | ランタイム形式のロード時間 | Must | Draft |
| REQ-PERF-006 | 多数体の同時表示 | Should | Draft |
| REQ-PERF-007 | Cubism SDK 比の評価速度 | Must | Draft |
| REQ-PERF-008 | エディタの操作応答 | Must | Draft |
| REQ-PERF-009 | ベンチマークの回帰検知 | Must | Draft |
| REQ-INFRA-001 | Cargo workspace とクレート構成 | Must | Draft |
| REQ-INFRA-002 | 3 OS の CI | Must | Draft |
| REQ-INFRA-003 | ライセンスと DCO | Must | Draft |
| REQ-INFRA-004 | 対応プラットフォームの優先度 | Must | Draft |
| REQ-INFRA-005 | ドキュメント構成と状態管理 | Must | Draft |
| REQ-INFRA-006 | エージェント向け規約と言語の使い分け | Must | Draft |
| REQ-INFRA-007 | 開発ツールチェーン | Should | Draft |

## 用語集

| 用語 | 定義 |
|------|------|
| リグ | 画像パーツ・メッシュ・デフォーマ・パラメータからなる、動かせるモデルの構造 |
| ArtMesh | テクスチャを貼ったメッシュ。描画の最小単位 |
| デフォーマ | 子要素の頂点を変形する要素。ワープ（ベジェ格子）と回転がある |
| パラメータ | モデルを動かす入力値（例: `ParamAngleX`）。範囲と既定値を持つ |
| キーフォーム | あるパラメータ値でのメッシュ・デフォーマの形。間の値は補間する |
| パラメータ式 | パラメータ同士の依存を表す式。物理なしで連動を書ける |
| 描画リスト | core が出す 1 フレーム分の描画データ（頂点・不透明度・描画順・マスク・色） |
| プロジェクト形式 | エディタが読み書きする JSON 形式（zip または展開ディレクトリ） |
| ランタイム形式 | 配布・再生用のバイナリ形式。ゼロコピーで読み込める |
| 基準モデル | 性能目標と互換検証に使う、Tsukumoe 自作の公開モデル |
| ホスト | モデルを表示して配信に流すアプリ（透過ウィンドウ・Spout / Syphon・仮想カメラ） |
| 法務ゲート | moc3 対応に着手・公開する前に通す法的確認 |
| クリーンルーム実装 | 既存の非公開実装・第三者の解析成果を参照せずに行う実装 |
