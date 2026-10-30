# ADR-0003: UI に gpui-pre と gpui-kit を fork なしで使う

- 状態: 置き換え済み（[ADR-0025](0025-ui-wgpui.md)）
- 日付: 2026-10-30

## 背景

Ravel は gpui-ce（コミュニティフォーク）と、それに合わせた gpui-kit の fork に立っている。gpui-ce が生まれた理由は「Zed の gpui が公開されていない」ことだったが、現在は Zed 本体のスナップショットが `gpui-pre` としてほぼ毎日公開されている。gpui-kit 0.6 も上流では `gpui-pre` を前提にしている（Ravel `docs/implementation/gpui-kit-migration-plan.md`）。
Tsukumoe は新規なので、移行コストがない。

## 決定

- UI は `gpui-pre` と `gpui-kit`（`gpui-base` / `gpui-component`）で作り、fork を持たない
- `gpui-pre` / 兄弟クレート / `gpui-kit` を同じ Zed リビジョンに対応する版に固定し、同時に更新する
- `wgpu` は `gpui-pre-wgpu` と同じ版にし、ビューポートを UI と同じ device で描く
- ランタイム系クレートは GPUI に依存しない

## 結果

- 上流追従が楽になる。代わりに `gpui-pre` の破壊的変更に定期的に追従する必要がある
- Ravel の gpui 二重化問題（MED-GPU-07）を最初から避けられる
- 上流に足りない機能は、まず上流への PR で解決する。fork が必要になったら ADR で理由と解消条件を残す
