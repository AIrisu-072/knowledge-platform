# Document Diff v0 — Capability Execution Status

## 2026-09-29 JST — DIF-01〜03 local GREEN、DIF-04 NEXT

- 状態: **ACTIVE / Delivery Unit A 実装中**。DIF-01〜03は局所TDDを完了。DIF-04〜15とAのhosted gateは未完了。
- 実装branch: `feat/document-diff-v0@c042a27ec82738462a47d8d74c6d06c536e4af0a`（隔離worktree）。設計・計画Draft PR #23は未merge。実装PRは未作成。凍結設計blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、承認計画blob `0bdbba832ebcfc2d211fddbba18dc31e69e1a28c` を維持。
- DIF-01: RED `cbbe46d`、GREEN `0d886d7`。Application identity 5/5、core protocol 5/5 PASS。
- DIF-02: RED `a128d6a`、GREEN `b4bfca0`。snapshot実DB5/5、identity5/5、history回帰2/2、対象strict Clippy PASS。DSI欠落markerと再生成後のsnapshot再取得を実装。
- DIF-03: RED `724d73e`、GREEN `c042a27`。最終認可/Audit実DB4/4、file access回帰1/1、対象strict Clippy・fmt PASS。cache hitの再監査、policy剥奪、WORKING更新、T10、actor期限切れ、Audit失敗を確認。
- CI: DIF-01〜03のhosted exact-head CIは未実行。計画どおりA末尾で一度確認する。main baseline CI `36419479837` はsecurity setup失敗でありDiffの成否ではない。
- blocker / 判断: 現時点のDIF-04着手blockerなし。設計意味の変更提案なし。新Diff parser dependencyは未promotion。`executing-plans` helperはDIF見出しを解析できないため、承認計画を変えずledgerに手動記録している。
- 次の exact action: `feat/document-diff-v0` のDIF-04 briefに従いworker/runner crateの最小scaffoldと二原本・隔離のRED試験を先に置く。DSI公開sandbox sealを再利用し、Linux強制canaryはA hosted gateで確認する。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — 計画承認・実装開始

- 状態: **PLAN APPROVED / IMPLEMENTATION AUTHORIZED / DIF-01 NEXT**。DIF-01〜15はまだ未着手。
- 完了: 書面設計とProduction Implementation Planの承認。設計blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、計画blob `0bdbba832ebcfc2d211fddbba18dc31e69e1a28c`。承認の範囲はそれぞれの承認記録に従う。
- 使用branch / PR: `design/document-diff-v0@88722648db35407ca38599312f93164796bf8b90` はoriginと一致。Document Diff PRはまだない。実装branchは別のclean worktreeでこの計画承認headから作る。基準mainは `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`。
- 検証: `AGENTS.md`、Active/Status、凍結設計・計画、GitHub branch/PR/main CI、既存worktreeを再確認。main標準CI `36419479837` はsecurity setup失敗。Diff製品試験・CIは未実行。
- blocker / 未解決判断: 現時点でDIF-01着手を妨げるものはない。mainのsecurity setup失敗は最終GREEN判定までに別途解消・確認が必要。新parser dependencyの自動承認はない。凍結設計からの差分提案なし。
- 次の exact action: この承認/Statusを設計branchへcommit/pushし、Draft計画PRを作る。再利用する隔離worktreeに `feat/document-diff-v0` を承認headから作成し、DIF-01の焦点RED試験を先に書く。

以下は計画承認前のcheckpointであり、現在の開始条件ではない。

## 2026-09-29 JST — 書面設計承認、実装計画レビュー待ち

- 状態: **DESIGN APPROVED / PLAN REVIEW PENDING / IMPLEMENTATION NOT STARTED**。
- 完了: D1〜D13の設計合意、書面設計のcommit/push、依頼者による書面設計承認、Production Implementation Plan草案の作成と設計照合。
- 現在の工程: `docs/superpowers/plans/2026-09-29-document-diff-v0-production-implementation.md` の依頼者レビュー。DIF-01〜15はすべて未着手。
- 凍結設計: `docs/superpowers/specs/2026-09-28-document-diff-v0-design.md`、承認対象blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、設計提示head `design/document-diff-v0@42bd94be8737efbea1b289be15740c17e3e54398`。承認の範囲は設計承認記録に従う。設計意味の変更提案なし。
- 作業branch: `design/document-diff-v0`。Document DiffのPRは未作成。基準mainは `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`。PR #20は別CapabilityのSearch / Discovery設計PR。
- 検証: repository/GitHubのbranch、PR、main CIを再取得し、計画のspec coverage・型名・Task間の依存・Review Focusを自己点検。製品コード、migration、dependencyは変更せず、Rust試験・Diff CIは未実行。main標準CI `36419479837` は同headのsecurity setup失敗であり、Diffの成否を示さない。
- blocker / 未解決判断: 実装計画と実行方法は未承認。今回の依頼は設計のみであり、製品実装の開始指示はない。新production parser dependency、merge、deployの承認もない。
- 次の exact action: 計画書とこの記録を `design/document-diff-v0` へcommit/pushして依頼者に計画レビューを依頼する。承認・実装開始指示が得られた場合だけ、main/branch/PR/CIを再取得し、DIF-01のREDへ進む。

計画本文の資源数値は資格試験候補であり、実測済みproduction profileではない。設計凍結からの差分は提案していない。
