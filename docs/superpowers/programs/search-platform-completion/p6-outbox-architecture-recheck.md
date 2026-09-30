# P6 Durable Outbox Delivery Worker — 改訂 1 独立 architecture 再審査

- 判定: **GO for P6 design architecture gate（P1/P2 残件なし）**。Design Freeze、実装、実 PostgreSQL qualification、P7 接続の完了判定ではない。
- 対象: `p6-outbox-design-revision-1.md` SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、既報 `p6-outbox-architecture-review.md`、現行 Domain outbox / D4 Search consumer、P3 改訂の共有 lock 境界。審査中に設計文書の commit が進んだため、上記 SHA を審査対象の同一性とする。
- 方法: 設計・既存コードの静的照合、および PostgreSQL の [row lock](https://www.postgresql.org/docs/current/explicit-locking.html) / [SKIP LOCKED](https://www.postgresql.org/docs/current/sql-select.html) 契約の確認。新 SQL の実行、fault injection、性能測定は行っていない。

## 既報 P2-1〜4 の再判定

| 指摘 | 判定と根拠 |
| --- | --- |
| P2-1 claim 後の dispatch 待ちで lease 失効 | **閉鎖**。改訂 31–56 行は free permit 確保後に `limit <= free_permits` で claim し、Search は 1 件に固定する。dispatch 直前の `renew` が current token・未完了・DB 時刻での期限を検査し、0 行または応答不明なら handler を呼ばない。renew 中の喪失は cancel / unknown とする（58–60 行）。`SKIP LOCKED` を handler 期間の排他と誤認していない。 |
| P2-2 最終 claim 後の crash で行が永久滞留 | **閉鎖**。行ごとの `attempt_limit` と DB policy を固定し、policy 不一致や旧上限行を fail closed にする（18–20 行）。起動時・毎 poll cycle の claim 前・shutdown drain 終端に bounded `reap_exhausted` を実行し、期限切れ最終試行を `delivery_unknown_at_limit` として原行を残したまま一度 terminal 化する（102–126、174 行）。結果不明を失敗確定と偽らない。 |
| P2-3 process-local gate では Source 単位を排他できない | **閉鎖**。改訂 14、22、130–145 行は共有 DB の Source lease を outbox claim 前に取得し、全 process で同一 Source の reconcile を 1 件にする。取得ごとの単調な `fence_epoch`、owner token と期限による renew/release、待機側の非 claim を規定する。現行 `DocumentOutboxIndexer::gate` が instance-local（`crates/search-source-document/src/outbox.rs:642-648,701-712`）である点を正しく区別している。 |
| P2-4 stale handler が新しい receipt / current を上書き | **閉鎖**。改訂 22–24、147–170 行は Domain outbox、Source pointer、receipt を同じ PostgreSQL DB に置き、publish と receipt を同一短期 transaction で両 lease / epoch、current pointer、READY を条件付き検査する。receipt は低い epoch からのみ更新し、同 epoch は key / digest 完全一致時だけ冪等とする。`Published` だけでなく `Unchanged` / `Duplicate` も結合 port を通る。旧/GC 済み receipt は pin と見なさず、現在 pointer と READY の一致を再検証する。現行の無条件 `IndexingReceiptStore::put`（`crates/search-source-document/src/outbox.rs:55-58,821-845,910-921`）を durable 実装に流用しない。 |

## 境界の照合

- 現行 `outbox_events` は lease / DLQ 列を持たず、`audit_outbox_events` は別表である（`crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql:74-101`）。改訂 18–20 行の nullable 列・CHECK・旧行保持と、10 行の Domain / Audit 責務分離は additive migration の契約として整合する。event / payload / `delivered_at` の既存値を移行で再作成・更新しない。
- Search bridge は `Document` / `Folder` / `AccessPolicy` の型付き event を allowlist に照合し、payload ではなく現行 Source snapshot を読む（改訂 12 行、`crates/search-application/src/indexing_service.rs:43-85`）。generic P6 は配送状態だけを所有し、Search receipt は Search が所有する（改訂 10、192–194 行）。
- 複合 transaction の lock 順は outbox → Source（改訂 149 行）に、P3 の Source → generation key 順 → lease（`p3-graph-design-revision-1.md:52-62`）を接続できる。manual rebuild は outbox 行を持たず Source から始める。handler の build 中に Domain 行 lock を保持しない。lease 喪失時の cancellation は協調的であり、既に走る外部 build の即時停止を保証しないため、可視状態の変更を両 fence 付き CAS に限定する記述（改訂 130、168、174 行）が必要十分な設計境界である。
- P3 の別再審査は incremental base / target の durable build guard を未解決としている（`p3-graph-architecture-recheck.md:18-28`）。P6 の candidate staging 保護（改訂 150 行）と P7 の GC / pointer / evaluation pin を統合する際、その guard と P3 の `pointer_revision` 条件を保持する必要がある。これは P3/P7 composition の未完了事項であり、既報 P6 四件の再発や P6 単独の新規 P2 とは判定しない。

## 次の exact action

上記 SHA の改訂を P6 の Design Freeze 候補として扱い、同一 DB の両 fence、receipt / GC の意味、Source lease の claim 前取得、DB policy と旧上限行、normative `aggregate_version` / `processing_state` 差分を Freeze / plan に反映する。P6 の production 完了は、計画された実 PostgreSQL の crash・再配送・fence・migration qualification と P7 の durable composition を確認した後に別判定する。本再審査から実装試験済み、P3 設計 gate 通過、merge / deploy 済みとは推定しない。
