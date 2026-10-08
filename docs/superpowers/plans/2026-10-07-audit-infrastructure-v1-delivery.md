# Audit Infrastructure v1 配送・保存・検証 実装計画

設計：[配送・保存・検証設計](../specs/2026-10-07-audit-infrastructure-v1-delivery-design.md)。決定記録：[envelope・Store・integrity](../../decisions/2026-10-07-audit-envelope-store-integrity.md)。状況：[Capability Execution Status](../execution/audit-infrastructure-v1-status.md)。

基点は main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`。branchは `claude/cool-darwin-7xh893` で、統合単位ごとにDraft PRを作る。各単位は、独立security/correctness review → exact-head hosted CI → main統合 → main CI確認の順で進める。統合後、branchを最新mainから作り直して次の単位へ進む。

試験はPostgreSQL 18.6（testcontainers、`postgres:18.6-bookworm`）と合成データだけを使う。ローカルでは `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` でdisk消費を抑える。

## 単位A：event契約（PR #98）

1. `crates/audit-core` を新設し、workspaceと `dependency-rules.toml` の境界に登録する。
2. `spec/telemetry/audit-event-catalog.json`：Document 21種とcontrol 14種を収める（control種別数は設計改訂3 §4.5で12→14に更新済み。正本はcatalog）。kind、subject形、correlation写像、reason扱いを定める。
3. envelope（CloudEvents 1.0.2 structured JSON、閉じた属性集合、重複key拒否、上限）、payload v1、catalog駆動のvalidator。
4. legacy投影：claim projection（SQL側でreasonを除去したrow）から `AuditEnvelope` を作るか、quarantine codeを返す。
5. chain/genesis計算、export検証（RawValueでの原文hash、連続chain、checkpoint照合）。
6. `spec/telemetry/audit-event.schema.json` の生成と再現性試験。Rust⊂schemaの関係を確かめる。
7. 互換試験：main producerの全payload形をacceptし、機微key・未知key・型違反・上限超過・重複key・correlation違反・control偽装を拒否する。`legacy_time` の形を固定する。投影の出力を `spec/telemetry/audit-adapter-golden.json` に (source_format, adapter_version) ごとに固定する（golden pin。entryは `<fixture>@<入力行hash>` で追加のみ、旧sectionは試験内のdigestで凍結）。版の上げ忘れと既存sectionの編集を試験で検出する。
8. `mise.toml` / CIにおけるcontract検査の位置を確認する（workspaceのnextestで実行されることを確認する）。

## 単位B：Store・配送

状況（2026-10-08）：手順1–4は実施済み（worktree branch、未push）。設計からの意図的な差分は設計の改訂4、経過と検証は[状況](../execution/audit-infrastructure-v1-status.md)に記す。残りは手順5。

1. `crates/audit-store-postgres`：migration（`audit_store`、ledger `audit_store_sqlx_migrations`、owner role、REVOKE PUBLIC、definer関数、guard、registered_types）、`AuditStore` portの実装、2段階開示、verify、retention/purge、権限・束縛、status/probe、roles.sql、bin `audit-admin`。
   - 実施：上記に加え、`privileges.sql`、検証の被覆、拒否の集約（`suppressed_since_last`）、`begin_recovery_epoch` の帯域外期待値とpreview、DB外の総合判定 `audit-admin assess` と帯域外recovery記録 `kp-audit-recovery-records-v1`、postureの拡張（定義済みrole、REPLICATION login、列権限）。
2. `crates/audit-relay`：migration（`audit_relay`、ledger `audit_relay_sqlx_migrations`、lock＋backfill＋trigger、guard、deliveries/history/policy、`BEGIN ATOMIC` digest、claim等のdefiner関数）、`OutboxStore` 実装、handler、circuit breaker admission、DeliveryLedger、replay/repair/reconcile/health、roles.sql、bin `audit-relay`。
   - 実施：上記に加え、`run` のcircuit報告（`report_runtime`）と進捗行、relay側保留の行ごとのbackoff、`max_referenced_store_seq`（`--relay-max-seq` の入力）、repairのepoch fence、postureの拡張（`owner_member`、`staging_read`、`table_access`、`column_privilege` ほか）、表の書き手guard。
3. TDDの順序（実施：下記の順で試験とともに実装した。独立review・確認reviewの指摘は、修正前に失敗する再現試験を先に書いて反映した）：
   1. Storeの基本（ingest、idempotency、append-only）
   2. role行列・shadowing
   3. 開示2段階
   4. integrity/retention
   5. relayの登録・atomic性
   6. claim/lease/ack
   7. 外部障害時の試行返却
   8. crash（子process kill -9）
   9. replay/reconcile
   10. restore/gate/epoch
4. 日本語運用手順 `docs/operations/audit-delivery-store.md`。
   - 実施：作成済み（未検証事項の一覧を含む）。
5. branch `claude/cool-darwin-7xh893` へのpushとDraft PR → exact-head hosted CI → main統合 → main CI確認。統合後、単位Cへ進む。

## 単位C：Document受入・handoff

1. 実Document producerからStoreまでのE2E（作成〜公開終了、scheduler attribution、拒否）、staging失敗時のrollback、Document migration追加時の互換。
2. 復旧の証拠：backup/restore、crash/restart、reconcile。
3. `docs/superpowers/handoffs/audit-infrastructure-v1-organization-handoff.md`（Organization・Search・Documentへの引継ぎ）。
4. capability matrixの最終更新と、未検証事項の記録。
