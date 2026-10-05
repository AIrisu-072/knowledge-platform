<a id="p6-n01-規範整合-receipt"></a>
# P6-N01 規範整合の検証記録

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-normative-receipt.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 状態: 規範仕様の限定改訂と対象限定の静的照合完了。P6実装・実DB試験・統合資格の証拠ではない。
- ブランチ / 作業時HEAD: `feat/search-platform-completion-core` / `80a47960d025e4dfdea1eacade28b15d218725ff`。
- 正本: `p6-outbox-freeze.md` SHA-256 `d9ac0835f8129b09e096486a8e18d8158932ce4ba101917758d94218d442dd9a`、`p6-outbox-design-revision-1.md` SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、`p6-outbox-plan.md` SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`。トレースの形式根拠は[W3C Trace Context（トレース文脈）](https://www.w3.org/TR/trace-context/)。

## RED: 既存規範との衝突

| 元の箇所 | 衝突・不足 |
| --- | --- |
| `spec/data/transaction-consistency-requirements-v0.md:760-772` | §7は`aggregate_version`と`processing_state`を全Outboxイベントの保持項目として列挙した。現行の物理`outbox_events`（`0001_document_authoritative_core.sql:74-84`）には両列がなく、P6 設計凍結は前者の一律埋戻しを禁止し、後者を導出読み取りモデルとする。 |
| 同: `774-780` | 少なくとも1回の配送、再試行、デッドレターのみで、単一Search配送先、厳密順序の非保証、上限到達後の結果不明・回復待ちが未定義だった。 |
| `spec/operations/observability-audit-requirements-v0.md:166-185,424-430,562-587` | W3C コンテキスト伝播と未処理件数・最古の経過時間、Audit耐久は既定だが、P6の検証済みトレースだけの記録・使用、旧行の相関限界、リース/DLQ/上限到達行の回収処理観測、Domain配送とAudit配送の独立性が明示されていなかった。 |

編集前の対象限定の規範チェックは `derived processing_state`、`single Search destination`、`no universal aggregate_version backfill`、`unordered at-least-once`、`validated W3C outbox trace`、`Audit outbox remains independent` の6項目すべてRED（exit 1）。

## GREEN: 限定改訂

- `spec/data/transaction-consistency-requirements-v0.md:760-788`: 現行物理列とP6の追加列を分け、`DELIVERED|DEAD_LETTER|IN_FLIGHT|PENDING`をDB時刻による導出状態にした。上限到達・回復待ちを別表示し、旧NULL `attempt_limit`上限行の安全側に拒否する監査と生成側別のバージョン契約を明記した。
- 同 `:780-788`: 単一Search 橋渡し処理、少なくとも1回の配送と厳密順序非保証、Source再読・永続化したイベント受領記録後の汎用側だけの配送成功確定、Audit Outboxの独立を明記した。
- `spec/operations/observability-audit-requirements-v0.md:186,425-437,449,597`: W3C形式・長さを記録前/利用前に検証し、旧行では新スパンとイベントIDで相関する境界を記した。リース、DLQ、上限到達行の回収処理、遅延、種類数を抑えた観測とAudit独立を追記した。

## 確認結果と境界

- 同じfocused規範チェック6項目が全てPASS（exit 0）。
- P4追加節（SD-T8〜10 / SD-O1〜2）の本文はそれぞれHEADと完全一致（exit 0）。
- 対象2ファイルの`git diff --check` PASS（exit 0）。専用のMarkdown・仕様文書の静的検査タスクは見当たらず、`mise run verify:fast`は並行実装中のワークスペース全体の検証ゲートのためこの文書だけを変更するタスクでは実行しない。
- 改訂後SHA-256: `spec/data/transaction-consistency-requirements-v0.md` = `71e4ed0d09c1b4368e78d8896c05c9126c14d34794cb37e26ea937826721aa0d`、`spec/operations/observability-audit-requirements-v0.md` = `d20dfac30fda3c826746e8f1814a844fe3d5578f10299f2f79c97a28afeb868d`。
- 設計凍結からの意味変更提案、意味上の強制停止条件、未確定操作: なし。コミット、push、PR、本番マイグレーションは行っていない。
- 当時の次の具体的な作業: 統括担当による独立した読み取り専用レビューで本2仕様と設計凍結の意味一致を確認し、P6-I02の物理マイグレーション/実DB RED-GREENへ引き渡す。
