# ADR-0013: 開発体制とドキュメント規約は Ravel を引き継ぐ

- 状態: 採用
- 日付: 2026-10-30

## 背景

Ravel（NaruseNia/ravel）は、主開発者と AI エージェントによる大規模開発の規約を整えている。

## 決定

- 主開発者 + AI エージェント（Claude Code / Codex）を前提とする
- `AGENTS.md` が正本。`CLAUDE.md` は `@AGENTS.md` だけ
- 要件は REQ-{SCOPE}-{SEQ}
- 言語: 要件・計画・ADR は日本語。コード・コメント・コミット・形式仕様・README は英語（README_ja を併設）
- mise・lefthook を使う。Rust は blaze を使うため 1.99 に固定する（`mise.toml`）
- moc3 のクリーンルーム規則を AGENTS.md に明記する（ADR-0016）

## 結果

- implementation ディレクトリだけは Ravel の形を引き継がず、整理し直す（ADR-0014）
