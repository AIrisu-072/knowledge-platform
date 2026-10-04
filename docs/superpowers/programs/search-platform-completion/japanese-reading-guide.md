# Search 文書の日本語案内と原文の対応

この案内は、既公開のSearch文書を読みやすくするためのものです。実装、実行順、設計の承認、検証結果を変更するものではありません。

## 現在の状態を読む

- [2026-10-03 18:12 UTCの公開後の状態照合](../../execution/search-current-state-20261003.md)は新しい状態更新です。以下の意味保存訳とは別に、公開head、終端CI、G07/G08停止、公開保留を整理しています
- [最新の実行状態](../../execution/search-platform-completion-program-status.md)を先に確認してください。過去文書にある次の作業や実行手順は、現在の再開許可ではありません
- [G07の準備と実行停止の日本語記録](g07-owned-preparation-20261003/README.md)では、確認できた準備、Unixソケット制限による停止、未実行の復旧試験とG08を分けています

## 原文と日本語訳の扱い

元の説明は公開commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c` にバイト単位で保持されています。各日本語文書の冒頭から固定原文へ移動できます。設計凍結に記録された承認hashはその原文・当時の証拠を指し、日本語本文のhashではありません。

[機械可読の対応表](japanese-translations-20261003.json)には、翻訳元commit/blob/SHA-256と、日本語本文のblob/SHA-256を別々に記録しています。今回の独立レビューは意味保存だけを対象とし、元の設計承認や実装・実行の合格を追加しません。コードブロック、原ログ、機械キー、識別子、数値、合否と未達条件は保持しています。

## 第1組：意味保存を確認した13文書

| 日本語文書 | 固定原文 |
| --- | --- |
| [Searchルート依存関係メタデータの修復記録 — 2026-10-03](dependency-metadata-recovery-20261003.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/dependency-metadata-recovery-20261003.md) |
| [Search継続作業 — Draft公開時のチェックポイント、2026-10-01](draft-publication-checkpoint-20261001.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/draft-publication-checkpoint-20261001.md) |
| [Search Platform Completion — ローカル環境の調査記録](environment.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/environment.md) |
| [P6 G05/G06の合成入力によるランナー修正記録 — 2026-10-03](p6-g06-runner-correction-20261003.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-g06-runner-correction-20261003.md) |
| [P6 G07の純粋フィクスチャガードのチェックポイント — 2026-10-03](p6-g07-pure-guards-20261003.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-g07-pure-guards-20261003.md) |
| [P6 永続 Outbox 配送ワーカー — 改訂 1 独立アーキテクチャ再審査](p6-outbox-architecture-recheck.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-architecture-recheck.md) |
| [P6 永続 Outbox 配送ワーカー — 独立アーキテクチャレビュー](p6-outbox-architecture-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-architecture-review.md) |
| [P6 永続 Outbox 配送ワーカー — 設計改訂 1](p6-outbox-design-revision-1.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design-revision-1.md) |
| [P6 — 永続 Outbox 配送ワーカー設計案](p6-outbox-design.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design.md) |
| [P6 汎用永続 Outbox の設計凍結](p6-outbox-freeze.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-freeze.md) |
| [P6 永続 Outbox 配送の実装計画](p6-outbox-plan.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-plan.md) |
| [P6プロセス復旧の下書き — コンパイルのみの修正記録、2026-10-01](p6-process-recovery-compile-correction-20261001.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-process-recovery-compile-correction-20261001.md) |
| [Search実験用ロックのメタデータ修復記録 — 2026-10-03](poc-lock-metadata-recovery-20261003.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/poc-lock-metadata-recovery-20261003.md) |

## 第2組：意味保存を確認した9文書

| 日本語文書 | 固定原文 |
| --- | --- |
| [P7 本番ランタイムの設計・計画凍結記録](p7-runtime-freeze.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-freeze.md) |
| [P7 本番ランタイムの範囲限定実装計画と凍結記録](p7-runtime-plan.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-plan.md) |
| [P7 共有永続化基盤の統合設計凍結記録](p7-shared-durable-freeze.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-freeze.md) |
| [P7 共有永続化世代の本番実装計画](p7-shared-durable-plan.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-plan.md) |
| [P6 v0 ポリシーの有限な上限・下限に関する判断](p6-policy-bounds-ruling.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-policy-bounds-ruling.md) |
| [P6 PostgreSQL ポリシー行ロックに必要なロール権限の判断](p6-policy-lock-role-ruling.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-policy-lock-role-ruling.md) |
| [P6-G04 PostgreSQL 試行上限到達行の回収処理：実装記録](p6-postgres-reaper-code.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-reaper-code.md) |
| [Searchプラットフォーム完成プログラム — 状態](../../execution/search-platform-completion-program-status.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/execution/search-platform-completion-program-status.md) |
| [Search / Discoveryプラットフォーム v0 — Phase D 受入証拠](../../execution/search-discovery-platform-v0-acceptance.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/execution/search-discovery-platform-v0-acceptance.md) |

## 第3組：P6の限定判定と残るゲートを読む8文書

以下は過去の規範整合・局所的な実DB確認の記録です。現在のG07復旧試験や統合受入の成功を示しません。

| 日本語文書 | 固定原文 |
| --- | --- |
| [P6-N01 規範整合の検証記録](p6-outbox-normative-receipt.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-normative-receipt.md) |
| [P6-I02 Domainマイグレーションの独立レビュー](p6-domain-migration-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-domain-migration-review.md) |
| [P6-I02 Domainマイグレーションとポリシー上限・下限の独立再確認](p6-domain-migration-recheck.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-domain-migration-recheck.md) |
| [P6-I03 Search調整基盤のマイグレーション独立レビュー](p6-coordination-migration-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-coordination-migration-review.md) |
| [P6-G01 / P6-S01 型付きポート・モデルの独立レビュー](p6-delivery-port-model-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-delivery-port-model-review.md) |
| [P6-G02 PostgreSQLの処理権取得・永続化の独立レビュー](p6-postgres-claim-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-claim-review.md) |
| [P6-G04 PostgreSQLの試行上限到達行を回収する処理の独立レビュー](p6-postgres-reaper-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-reaper-review.md) |
| [P6-G03 PostgreSQLのフェンス付き結果確定の独立レビュー](p6-postgres-settle-review.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-settle-review.md) |

## P7正本：意味保存を確認した5文書

凍結記録が合成する元設計と改訂を対象にしました。元の承認hashは固定原文を指し、設計だけのGOを実装・運用の合格に置き換えません。

| 日本語文書 | 固定原文 |
| --- | --- |
| [P7 本番ランタイム・配備・SLOの完成に向けた設計提案](p7-runtime-design-completion.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-completion.md) |
| [P7 本番ランタイムの設計改訂1](p7-runtime-design-revision-1.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-revision-1.md) |
| [P7 本番ランタイムの設計改訂2](p7-runtime-design-revision-2.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-revision-2.md) |
| [P7 共有永続世代基盤 — 設計案](p7-shared-durable-design.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-design.md) |
| [P7 共有永続世代基盤 — 設計改訂 1](p7-shared-durable-revision-1.md) | [原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-revision-1.md) |

## activeのSearch4節だけの部分訳

[activeの依存修復・G06・G07履歴](../../execution/active.md)は、[原文15–42行](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/execution/active.md#L15-L42)の4節だけを翻訳しました。他trackと残りの履歴は不変です。対応表のpartial_filesは、この部分訳のsnapshotと訳節hashを記録し、その後の新しい現在状態の追記を含めません。ファイル全体を翻訳した35文書とは別に数えます。

## 継続する確認

Searchの人向けMarkdownは、完成プログラム配下とSearchの設計・計画・実行記録を合わせて159文書を棚卸ししています。この数は翻訳完了数ではありません。以前に日本語化したG07の説明2文書に加え、第1組13文書、第2組9文書、第3組8文書、P7正本5文書、計35文書の意味保存を確認しました。現在状態・受入説明とP7の主要な正本設計・凍結・計画を含みます。以降は今回更新した説明と、現在のP6/P7進捗・残るゲートを理解するための正本設計・計画・受入を優先します。元から日本語で変更不要と確認した文書は、翻訳済み件数と区別します。全履歴や第三者文書の一括翻訳は行いません。

原ログ、コード、機械可読の証拠、固定snapshotは原文を保持し、日本語の説明と区別します。文書の日本語化を理由にG07の実行停止を解除したり、G08へ進めたりすることはありません。
