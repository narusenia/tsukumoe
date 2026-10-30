# ADR-0027: UI に egui（eframe）を使う（ADR-0025・ADR-0026 を置き換える）

- 状態: 採用
- 日付: 2026-10-30
- 置き換え: [ADR-0025](0025-ui-wgpui.md)、[ADR-0026](0026-ui-crate.md)

## 背景

ADR-0025 で wgpui を採用し、M1-11 で macOS 上のビューアまで作った。その後、同じ画面（基準モデル 150 メッシュ・パラメータ 100 個・Space + マウスでの操作）を egui でも作り、比べた。

- `viewer`（UI を知らない層）は無変更で流用でき、egui-wgpu の device 共有（wgpu 30）も問題なく動いた。`app` は約 330 行から約 110 行になった
- 日本語 IME は、egui は標準で候補窓が正しい位置に出る。wgpui は毎フレーム `invalidate_character_coordinates` を呼ぶ対処が要った
- 性能は UI 層で大差ない（リリースビルド・同じ窓サイズ・連続再描画: CPU は wgpui 約 165%、egui 約 131%。バイナリは strip 後 12.8 MB と 10.5 MB。起動は約 325 ms と約 260 ms。メモリは約 224 MB と約 250 MB）。支配的なのは参照レンダラ
- wgpui は個人の 0.x fork で、依存木が大きい（HTTP・async-std・zbus など）。`libc` の固定と `deny.toml` の例外が要った。egui は保守が活発で依存が軽い
- wgpui が優れる点（アクション / キーマップ、挙動層の部品の揃い）は、egui では自前になる

## 決定

- UI は egui（eframe + egui-wgpu、wgpu は 30）。ビューポートは egui-wgpu の device で `tsukumoe-gpu` が描く
- `tsukumoe-ui` は残し、egui の上の層にする。中身は ADR-0026 から引き継ぐ
  - デザイントークン（色 10 種 + 寸法）。既定は高密度（文字 12pt・コントロール高 24pt・余白 4pt 刻み・影なし）
  - テーマの切り替え（ライト / ダーク / システム）と JSON の読み込み（`{"appearance": "dark"|"light", "colors": {"accent": "#rrggbb"}}`、省いた色は組み込みで補い、未知のキーは拒否）。egui の `Style` へ反映し、部品はトークンを読む
  - 部品: Button・Switch・Slider・Label・Panel。gallery（example）で全状態を確認する
  - フォントは欧文を Inter、日本語を Noto Sans CJK JP（かな・約物・漢字に絞った部分集合）で同梱し、バイナリに埋め込む（`install_cjk_font`）。OS のフォントは使わない。システムの CJK フォントを fallback にすると基準線がずれた。どちらも OFL で、`crates/tsukumoe-ui/assets/fonts/` にライセンスを置く
- `tsukumoe-host` と `tsukumoe-app` は `tsukumoe-ui` を使う。`viewer`（UI 非依存）は変えない
- ホストは「操作の後の一定時間（物理が落ち着くまで）だけ」再描画し、止まっているときは描かない
- ランタイム系クレートは egui 系（`egui` / `eframe` / `egui-wgpu` / `epaint` など）に依存しない。CI で検査する

## 結果

- 保守の不安と依存の重さが減る。egui_dock・egui_plot など部品の資産を使える
- キーの割り当て（ツール別・利用者による変更）は自前で作る。M5 の前に、`tsukumoe-ui` に小さなキーマップ層を足す想定
- 即時モードなので、重い画面は「必要なときだけ再描画」と描画の分離で対処する
- 疎結合（`viewer` が UI を知らない）を保ったので、再び乗り換える場合も UI 層の書き直しで済む
- egui の同梱フォント（`epaint_default_fonts`）は SIL OFL と Ubuntu Font Licence で、`deny.toml` にそのクレート限定の例外を置いた。配布するバイナリには、これらのライセンス表記を付ける（M3 の配布時に対応。欧文フォントを同梱するか自前で選ぶかは M1-14 で決める）
- 未検証: Windows / Linux の窓と IME、複数行の入力、ライトテーマの全部品の見た目
