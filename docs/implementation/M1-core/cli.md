# CLI（形式変換・検証・ヘッドレスレンダ・画像差分）

- REQ: REQ-AI-002
- 単位: M1-10
- 状態: 着手中
- 依存: M1-08, M1-09
- クレート: `tsukumoe-cli`（バイナリ名 `tsukumoe`）。`tsukumoe-core` / `tsukumoe-format` / `tsukumoe-gpu` に依存

## コマンド

| コマンド | 内容 |
|---|---|
| `tsukumoe convert <in> <out>` | プロジェクトの zip ⇄ 展開ディレクトリ。入力・出力とも「ディレクトリか、そうでなければ zip」で判定 |
| `tsukumoe validate <project>` | 読み込みとコンパイルを行い、結果を JSON で標準出力へ。失敗は終了コード 1 |
| `tsukumoe render <project> -o <png> [--param NAME=VALUE]… [--size WxH] [--view X0,Y0,X1,Y1]` | 指定姿勢の PNG |
| `tsukumoe diff <a.png> <b.png> [--tolerance N] [-o <png>]` | 画素差を JSON で標準出力へ。許容を超えると終了コード 1。`-o` は差分画像 |

- 終了コード: 0 成功、1 検査・差分の不合格または実行時エラー、2 使い方の誤り
- `render` の既定: ビューは頂点の外接矩形、サイズは長辺 512。物理は進めない（`dt = 0`）
- テクスチャは `textures/*.png`（パス順、`file-formats.md`）
- 引数は手書きで解析する（`clap` は未承認のため追加しない）

## 範囲

| 含める | 含めない |
|---|---|
| 上の 4 コマンド | MCP サーバ（M4 以降）、モーション再生・表情の適用 |
| `Image::from_png` / `Texture::from_png` / `Image::diff`（`tsukumoe-gpu`） | 未使用パラメータなどの意味検査（`validate` は読み込み・検証・コンパイルの結果まで） |

## 受入条件

- [ ] zip ⇄ ディレクトリの変換で内容が変わらない
- [ ] `validate` が正常・異常を構造化データ（JSON）で返し、終了コードで区別できる
- [ ] `render` がパラメータ指定の姿勢を PNG で出力し、テクスチャを読み込む
- [ ] `diff` が差分の最大値・画素数を返し、許容超過で失敗する
- [ ] 不正な引数・存在しないファイルでパニックしない
- [ ] `mise run check` が通る
