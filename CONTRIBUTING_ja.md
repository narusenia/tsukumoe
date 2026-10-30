# Tsukumoe への貢献

> English: [`CONTRIBUTING.md`](CONTRIBUTING.md)

このページは案内図。規則そのものは、対象のそばに置いてある。

## 規則の置き場所

| 読むもの | 内容 |
|---|---|
| [`AGENTS.md`](AGENTS.md) | リポジトリの正本ガイド。構成・依存規則・moc3 クリーンルーム規則 |
| [`docs/specifications/architecture.md`](docs/specifications/architecture.md) | クレート構成と依存の向き |
| [`docs/implementation/roadmap.md`](docs/implementation/roadmap.md) | マイルストーンごとの計画 |
| [`docs/adr/`](docs/adr) | 各判断の理由 |

`CLAUDE.md` は `AGENTS.md` を 1 行で取り込むだけ。

## セットアップ

ツールは [mise](https://mise.jdx.dev/) で固定している。

```bash
mise trust               # clone ごとに 1 回
mise install             # Rust、blaze、lefthook、jq
mise run hooks:install   # 任意: pre-commit の fmt / clippy とコミットメッセージ検査
mise run check           # fmt + clippy -D warnings + 依存規則 + テスト
```

CI が実行するのは `mise run check`。

## 変更の作り方

- **ブランチ名**: 意味のある接頭辞 + 具体的な kebab-case（`feat/dag-topological-sort`。`feat/step2` は不可）
- **コミット**: 1 コミット 1 概念。英語 1 行、Conventional Commit の接頭辞（`feat:` `fix:` `refactor:` `docs:` `test:` `chore:` `perf:` `ci:`）、接頭辞の後は小文字、issue 番号は入れない
- **テスト**: バグ修正には、修正を戻すと失敗する回帰テストを付ける
- **依存**: 本番依存を足す前に相談する。依存のライセンスは `cargo deny` で検査し、GPL 系は拒否する
- **moc3**: `.moc3` に関わる作業の前に、`AGENTS.md` のクリーンルーム規則を読む

## 署名（DCO）

貢献は [Developer Certificate of Origin](https://developercertificate.org/) に基づいて受け付ける。各コミットに `git commit -s` で `Signed-off-by:` を付ける。CLA はない。

## ライセンス

貢献したコードは、プロジェクトと同じ Apache-2.0 OR MIT で提供されることに同意したものとする。
