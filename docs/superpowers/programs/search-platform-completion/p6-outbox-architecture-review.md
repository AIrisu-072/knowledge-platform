# P6 Durable Outbox Delivery Worker — 独立 architecture review

- 判定: **NO-GO（P1 なし、P2 4 件）**。設計案の Freeze 前に以下の配送・receipt 契約を確定する。
- 対象: `p6-outbox-design.md`（2026-09-30 の DRAFT）、現行 Domain outbox schema/producer、Search D4 consumer、関連 normative spec。実装・配備の合格判定ではない。
- 方法: repository の read-only 照合と PostgreSQL 公式文書の SQL semantics 確認。新 SQL・worker は存在しないため、実 PostgreSQL qualification は未実施。

## Blocking findings

### P2-1: claim 済みで dispatch 待ちの lease が失効し得る

設計案 `p6-outbox-design.md:68,70,77,99` は `batch_size <= 32`、`max_in_flight <= 8` を許し、renew を「長い handler の間」と記す。32 件を同時 claim して 8 件だけを handler に渡すと、残り 24 件は semaphore 待ちの間に 120 秒 lease が失効し得る。他 process はそれらを再 claim でき、元 process が後から同じ event を dispatch すると不要な重複と試行数消費が起こる。`FOR UPDATE SKIP LOCKED` が保護するのは claim transaction 中の行だけである（[PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html)）。

**修正条件:** dispatch permit を確保してから `claim(limit <= free_permits)` するか、claim commit 直後から待機中を含む全 lease を renew し、dispatch 前にも token/期限を再検証する。前者を初期契約とすれば単純。`batch=32 / in_flight=8 / handler > lease` の実 DB 試験で、未開始 claim の重複処理と上限到達が起きないことを確認する。

### P2-2: 最終 claim 後の crash を DLQ へ移す実行契約がない

設計案 `p6-outbox-design.md:68,73` では `attempt_count >= max_attempts` を claim から除外し、別の `reap_exhausted` で terminal 化する。一方、run loop `:99` は `poll -> claim -> dispatch -> renew -> settle` だけで、reaper の起動時・周期・停止時の実行を定めていない。最終試行を claim した直後に process が落ちると、lease 失効後も `delivered_at/dead_lettered_at` が NULL のまま claim 対象から永久に外れる。

**修正条件:** 起動時と周期実行に bounded `reap_exhausted` を明記し、DB clock で `lease_expires_at <= clock_timestamp()`、未完了、上限到達を確認する locked/fenced 更新と alarm を契約化する。複数 worker の max-attempt 設定差で早期 DLQ が生じない設定境界も固定する。最終 claim commit 直後の kill と再起動を実 PostgreSQL で試験し、原行が残ったまま一度だけ `DEAD_LETTER` になることを確認する。

### P2-3: 「Source あたり in-flight 1」は複数 process で成立しない

設計案 `p6-outbox-design.md:54,68,77,99` の semaphore は process-local と読める。2 worker が同じ Document Source に属する別 event を claim すれば、それぞれが全 Source 再構築を並列実行できる。D4 の `DocumentOutboxIndexer::gate` も instance-local（`crates/search-source-document/src/outbox.rs:642-648,701-704`）。generation CAS と最大 3 回の再読（同 `:705-712`）は投影の破壊を防ぐが、健康な burst で CAS 敗北と再試行を繰り返し、`max_attempts` に達して event を DLQ に送る可能性は残る。`aggregate_id` は Document/Folder 等であり、Source 単位の dispatch key ではない（`crates/document-repository-postgres/src/targeted_events.rs:10-15`）。

**修正条件:** P6 初期接続で Search bridge を単一 active process に限定する配置契約、または process 間の Source 単位の排他・coalescing 契約を明示する。generic worker の 2/4/8 台 claim 試験とは別に、同一 Source の event burst と worker 交代を実 DB + Search handler で検証し、CAS contention が正常 event を DLQ にしないことを示す。P7 の shared runtime は別途必要であり、process-local runtime の複製を production 成功と数えない。

### P2-4: lease 失効後の古い Search handler が durable receipt を上書きできる

設計案 `p6-outbox-design.md:70,81-83,95` は旧 token の outbox ack/fail を fence するが、handler が行う Search publish/receipt 書込みは fence しない。現行 `IndexingReceiptStore::put(event_id, receipt)` には条件付き更新契約がなく（`crates/search-source-document/src/outbox.rs:47-58`）、`Published` と `Unchanged` の後に呼ばれる（同 `:821-845,897-921`）。例: 旧 handler が G1 を publish して receipt put 前に停止し、lease を再取得した新 handler が更新後の G2 を publish・receipt put・ack した後、旧 handler が G1 の receipt を put できる。event receipt は古い generation を指し、設計案 `:83` の再起動後の整合条件と衝突する。G1 が GC されればその参照は読めない。

**修正条件:** P7 coordinator と Search receipt の関係を Freeze で定義する。少なくとも receipt の `put` が新しい current generation を古い generation で上書きできない条件と、GC 済み generation を指す歴史的 receipt の扱いを決める。lease expiry 中の二重 handler、publish 後の停止、G2 ack 後の旧 put、restart/GC を実 DB で再現し、current projection と receipt の契約が保たれることを確認する。単なる `event_id` PK / 無条件 upsert では足りない。

## 確認済みの境界と qualification 条件

- 現行 `outbox_events` は `event_id` PK、`available_at`、`attempt_count`、`delivered_at` を持つが lease/DLQ 列はない（`crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql:74-84`）。`audit_outbox_events` は別表（同 `:86-101`）。additive migration と既存行保持の方針は適切。`0009` は現行の最大 `0008` の次である。
- 現行 producer は業務 transaction 内に Domain event を記録する（例: `crates/document-repository-postgres/src/repository.rs:177-193`、`targeted_events.rs:18-41`）。Search bridge が payload を正本とせず、D4 が現行 snapshot を再読する境界は妥当。`AccessPolicyChanged` は Document と Folder の target にも発生するため、allowlist は一対一対応と仮定しない（`access_policy.rs:417-431,568-580`）。
- `SKIP LOCKED` は queue-like table の複数 consumer に使えるが、厳密な aggregate 順序を与えない。設計案の `available_at, occurred_at, event_id` を優先順とする説明は妥当（[PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html)）。期限の境界は `claim <= expiry`、`renew/ack/fail > expiry` として実 DB でちょうど同時刻の 0 行更新、旧 token、ack commit 応答不明を試験する。`clock_timestamp()` は statement 内でも変化する（[PostgreSQL date/time](https://www.postgresql.org/docs/current/functions-datetime.html)）。
- `aggregate_version` と物理 `processing_state` は normative list（`spec/data/transaction-consistency-requirements-v0.md:760-772`）にあるが、現行 producer/table に共通値はない。設計案 `:17,46,50,114` の read model・非保証の規範差分は Freeze で明示的に決裁し、既存 payload から一律 backfill しない。
- `traceparent/tracestate` は旧 producer では空のままとし、格納前と使用前に妥当性・長さ・機密値の allowlist を確認する。`tracestate` の構文適合だけを機密性の保証にしない。payload/actor を log・metric label に出さない方針と、Audit outbox の ack 非更新を縦断試験で確認する。
- 実 PostgreSQL の新 worker/migration がまだないため、この review は SQL 実行結果や throughput を証明しない。P6 の完了には `p6-outbox-design.md:109-116` の migration 保存性、複数 worker claim/fence、crash/retry/DLQ、Search receipt/ack、shutdown の focused qualification receipt が必要。
