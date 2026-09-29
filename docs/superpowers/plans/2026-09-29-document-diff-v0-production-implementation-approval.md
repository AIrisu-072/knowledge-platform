# Document Diff v0 — Production Implementation Plan 承認記録

- 日付: 2026-09-29 JST
- 状態: **計画承認済み・実装開始指示済み**
- 計画: `docs/superpowers/plans/2026-09-29-document-diff-v0-production-implementation.md`
- 承認対象の計画blob: `0bdbba832ebcfc2d211fddbba18dc31e69e1a28c`
- 凍結済み設計blob: `afee4e9351c5027b1252e8c7b74e542a295f0e67`
- 提示済みbranch/head: `design/document-diff-v0@88722648db35407ca38599312f93164796bf8b90`

依頼者は計画提示後に「承認します。実装を開始して予定しているものを全て完了するまで続けてください。」と指示した。DIF-01〜15を計画の依存順に実行する。計画本文の当時の `PROPOSED / PLAN REVIEW PENDING` 表示は承認対象blobの同定のため維持し、この記録で現在の承認状態を示す。

## 実行境界

- 承認済み設計の意味を変更しない。必要な場合はamendment gateに戻る。
- Native/inlineで実装し、Taskごとの焦点RED/GREENを残す。hosted CIは計画のA/B/C/D単位と最終exact-headで確認し、毎Task起動しない。
- 新しいproduction parser dependencyは計画承認だけではpromotionされない。各形式の資格試験とlicense/security選定を経る。
- commit、push、Draft PRの作成・更新は実装指示に含む。PR merge、本番配備、本番データmigrationは別指示を要する。
- 現行 `main@6ea29e1ceea82bb0e20b195890b7c0e7efc85f68` の標準CI `36419479837` はsecurity setup失敗。Document DiffのGREEN判定には利用しない。
