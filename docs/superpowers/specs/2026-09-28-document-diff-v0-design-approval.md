# Document Diff v0 — 書面設計承認記録

- 日付: 2026-09-29 JST
- 状態: **APPROVED — written design freeze active**
- 承認対象: `docs/superpowers/specs/2026-09-28-document-diff-v0-design.md`
- 提示済みbranch/head: `design/document-diff-v0@42bd94be8737efbea1b289be15740c17e3e54398`
- 承認対象blob: `afee4e9351c5027b1252e8c7b74e542a295f0e67`
- 基準main: `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`

## 承認の範囲

設計書を提示し、書面レビューと修正または承認を依頼した後、依頼者は「承認します。」と回答した。上記blobのDocument Diff v0設計を承認し、Production Implementation Planの作成へ進む指示として記録する。

承認対象には、同一Documentの2つのVersionの比較、方式Cのsnapshot単位派生cache、形式固有の比較、曖昧な対応と未比較範囲の明示、現在認可と開示監査、有限資源profile、評価条件が含まれる。承認された本文は変更せず保存する。本文冒頭の `PROPOSED` は作成時点の表示であり、この記録が上記blobの現在の承認状態を示す。

## 承認に含まれないもの

- これから作るProduction Implementation Planの承認、実行方法・worker model/effortの指定、製品実装開始。
- 製品コード、migration、dependency、HTTP/CLI/GUI、実装PR、本番配備、既存PRのmerge。
- 既存規範仕様の即時変更。新しいDiff開示監査event等の規範反映は、計画承認後の実装工程で差分を確認して行う。

本文の意味を後で変更する場合は、新しいblobへの明示的な設計改訂承認を必要とする。

## 次工程

承認済み設計を基に `docs/superpowers/plans/2026-09-29-document-diff-v0-production-implementation.md` を作成し、依頼者の計画レビューを受ける。計画承認と実行方法の決定までは製品実装に進まない。
