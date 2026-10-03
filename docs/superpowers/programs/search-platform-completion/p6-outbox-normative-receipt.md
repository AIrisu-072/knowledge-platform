# P6-N01 規範整合 receipt

- 状態: 規範仕様の限定改訂とfocused静的照合完了。P6実装・実DB試験・統合資格の証拠ではない。
- Branch / 作業時HEAD: `feat/search-platform-completion-core` / `80a47960d025e4dfdea1eacade28b15d218725ff`。
- 正本: `p6-outbox-freeze.md` SHA-256 `d9ac0835f8129b09e096486a8e18d8158932ce4ba101917758d94218d442dd9a`、`p6-outbox-design-revision-1.md` SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、`p6-outbox-plan.md` SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`。Traceの形式根拠は[W3C Trace Context](https://www.w3.org/TR/trace-context/)。

## RED: 既存規範との衝突

| 元の箇所 | 衝突・不足 |
| --- | --- |
| `spec/data/transaction-consistency-requirements-v0.md:760-772` | §7は`aggregate_version`と`processing_state`を全Outbox Eventの保持項目として列挙した。現行の物理`outbox_events`（`0001_document_authoritative_core.sql:74-84`）には両列がなく、P6 Freezeは前者の一律backfillを禁止し、後者を導出read modelとする。 |
| 同: `774-780` | at-least-once、retry、dead-letterのみで、単一Search配送先、厳密順序の非保証、上限到達後の結果不明・回復待ちが未定義だった。 |
| `spec/operations/observability-audit-requirements-v0.md:166-185,424-430,562-587` | W3C context伝播とpending/oldest age、Audit耐久は既定だが、P6の検証済みtraceだけの記録・使用、旧行の相関限界、lease/DLQ/reaper観測、Domain配送とAudit配送の独立性が明示されていなかった。 |

編集前のfocused規範チェックは `derived processing_state`、`single Search destination`、`no universal aggregate_version backfill`、`unordered at-least-once`、`validated W3C outbox trace`、`Audit outbox remains independent` の6項目すべてRED（exit 1）。

## GREEN: 限定改訂

- `spec/data/transaction-consistency-requirements-v0.md:760-788`: 現行物理列とP6のadditive列を分け、`DELIVERED|DEAD_LETTER|IN_FLIGHT|PENDING`をDB時刻による導出状態にした。上限到達・回復待ちを別表示し、旧NULL `attempt_limit`上限行のfail-closed監査とproducer別version契約を明記した。
- 同 `:780-788`: 単一Search bridge、at-least-onceと厳密順序非保証、Source再読・durable receipt後のgenericのみのack、Audit Outboxの独立を明記した。
- `spec/operations/observability-audit-requirements-v0.md:186,425-437,449,597`: W3C形式・長さを記録前/利用前に検証し、旧行では新spanとevent IDで相関する境界を記した。lease、DLQ、reaper、lag、low-cardinality観測とAudit独立を追記した。

## 確認結果と境界

- 同じfocused規範チェック6項目が全てPASS（exit 0）。
- P4追加節（SD-T8〜10 / SD-O1〜2）の本文はそれぞれHEADと完全一致（exit 0）。
- 対象2ファイルの`git diff --check` PASS（exit 0）。専用のMarkdown/spec lint taskは見当たらず、`mise run verify:fast`は並行実装中の全workspace gateのため本docs-only taskでは実行しない。
- 改訂後SHA-256: `spec/data/transaction-consistency-requirements-v0.md` = `71e4ed0d09c1b4368e78d8896c05c9126c14d34794cb37e26ea937826721aa0d`、`spec/operations/observability-audit-requirements-v0.md` = `d20dfac30fda3c826746e8f1814a844fe3d5578f10299f2f79c97a28afeb868d`。
- Design Freezeからの意味変更提案、semantic hard stop、未確定操作: なし。commit、push、PR、production migrationは行っていない。
- 次のexact action: 親の独立read-only reviewで本2仕様とFreezeの意味一致を確認し、P6-I02の物理migration/実DB RED-GREENへ引き渡す。
