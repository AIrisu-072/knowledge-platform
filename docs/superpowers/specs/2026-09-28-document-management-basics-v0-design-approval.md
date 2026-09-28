# Document Management Basics v0 — 書面設計承認記録

- 日付: 2026-09-28 JST
- 状態: **APPROVED — written design freeze active**
- 対象PR: #15、`design/document-management-basics-v0`
- 承認対象ファイル: `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md`
- 提示済みPR head: `0314f91a4e68221ed06778d36eaf228d0adfec86`
- 承認対象blob: `38010802a04c285336810e9b9c637c656ed1a76b`
- 設計本文の作成commit: `ea2b1e7fcb43e12f3db2336903cf3128b61606ca`
- 基準main: `55dc3d3a430c8f36e1db8277fee15c4429258466`

## 承認の根拠と対象

PR #15の設計書を提示し、「次は書面設計レビュー、承認後に実装計画を作成・レビューする」と説明した後、依頼者は「これで進めてください。」と回答した。この回答を、上記の書面設計を承認して実装計画作成へ進む指示として記録する。

対象はT5〜T9、認可付き一覧・版/操作履歴・ファイル参照、必要なイベントと監査の原子的記録、設計第16節の25件の受入条件、および第17節の規範への追記方針である。最も近い明示policyによる置換、予約中の管理変更制限、予約実行時の再認可、本人の明示既読、履歴とOutbox保持の分離を含む。

承認された本文は変更せず保存する。原文冒頭のPROPOSED表示と「レビュー待ち」は作成時点の記録であり、上記blobの現在の承認状態は本記録とCapability statusが示す。後日本文を変更する場合は差分と新しい承認を別記録にし、この承認を流用しない。

## この承認に含まれないもの

- 今回作成するProduction Implementation Planの承認。
- 実装方法の選択、製品コード・DB migration・依存追加・API/CLI/GUIへの着手。
- 開発ログ一元管理の完了確認、実装開始指示、PR #15または後続PRのmerge指示。
- 検索Index、配送worker、Audit Store、Windows/AD実接続の実装。

必要な規範追記は実装前に意味の一致を差分レビューする。既存Versioningの認可除外へ今回の期限到達認可を接続することは、過去実装済みという意味ではない。既存の承認記録は上書きしない。

## 次工程

`docs/superpowers/plans/2026-09-28-document-management-basics-v0-production-implementation.md` を作成し、依頼者の計画レビューを受ける。計画と実行方法が承認されても、ログ一元管理の完了確認・実装開始指示・着手時の正本再検証が揃うまでは製品実装を開始しない。
