# P6 Durable Outbox Delivery Worker — 設計改訂 1

- Status: **DRAFT / architecture recheck 前**。Design Freeze、実装、実 DB qualification の証拠ではない。
- Date: 2026-09-30 JST
- 対象: [元設計](p6-outbox-design.md) に対する [独立 architecture review](p6-outbox-architecture-review.md) の P2-1〜4。元設計は変更しない。本書がこの 4 件に関する Freeze 候補の差分を定め、その他の境界は元設計を引き継ぐ。
- 根拠: `0001_document_authoritative_core.sql:74-101` の実表、`search-source-document/src/outbox.rs:47-58,76-121,642-712,807-925` の D4 実装、`p3-graph-design.md:77-81` の P7 coordinator 接続境界。いずれも P6/P7 の新機能が実装済みであることは示さない。

## 1. 不変の責務と採用する実行単位

Domain producer は業務変更と `outbox_events` への event 記録を一 transaction で commit する。generic P6 worker だけがその event の claim、`attempt_count`、`available_at`、lease、retry、`delivered_at`、DLQ を所有する。Search は正本 Source を再読した派生 Projection、current generation pointer、Search event receipt を所有する。Search receipt と Domain の `delivered_at` は別の証拠であり、どちらか片方だけで他方の成功を宣言しない。`audit_outbox_events` の行と ack は本経路の対象外である。

v0 の必須宛先は一つの Search bridge とする。`Document`、`Folder`、`AccessPolicy` の event は設定された一つの Document `SourceId` に帰属させる。`aggregate_id` を Source の排他 key として使わない。event type と aggregate type の組合せは現行 producer と `DocumentIndexingService` の明示 allowlist で検査し、`AccessPolicyChanged` が複数の aggregate type に発生することを許す。未知の Document 系 event は黙って ack せず terminal error として観測する。Search bridge は payload を索引正本にせず、D4 と同じく現行 Source snapshot を読む。`Published` / `Unchanged` / `Duplicate` は後述の durable 完了 transaction が成立した場合だけ成功、`Ignored` は明示 no-op route 以外では成功にしない。

Search の初期 production 接続は **一 Source 当たり全 process で一つの実行中 reconcile** とする。P7 `SearchGenerationCoordinator` の durable Source lease を **claim より前**に取得した bridge process だけが、その Source の event を claim する。v0 の Search route は一回に 1 件を claim し、Search handler が完了または取り消されるまで次を claim しない。Source lease は event 処理ごとに解放し、次の取得で fence epoch を増やす。待機 process は outbox 行を先取りせず、Source lease の解放/期限を bounded poll する。generic P6 の複数 worker / batch claim 能力とは分けて試験する。P7 の共有 runtime が接続される前に process-local `MemoryDocumentIndexRuntime` の複数 replica を production と呼ばない。

## 2. Additive migration と DB policy

`0009_outbox_delivery_v0.sql` は既存 `outbox_events` の全列、PK、既存行、既存 producer insert を保持する。元設計の `lease_token UUID`、`lease_owner UUID`、`lease_expires_at TIMESTAMPTZ`、`last_attempt_at`、`dead_lettered_at`、allowlist `last_error_code`、任意の検証済み `traceparent/tracestate` に加え、`attempt_limit INTEGER NULL` を追加する。追加 CHECK は `attempt_limit IS NULL OR attempt_limit > 0`、`(lease_token IS NULL AND lease_owner IS NULL AND lease_expires_at IS NULL) OR (lease_token IS NOT NULL AND lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)`、`delivered_at IS NULL OR dead_lettered_at IS NULL`、`(delivered_at IS NULL AND dead_lettered_at IS NULL) OR lease_token IS NULL` とする。旧行は追加列が全て NULL なのでこれらを満たす。既存 delivered 行は `DELIVERED`、既存 pending 行は `PENDING` のままで、event/payload/時刻を再作成しない。`processing_state` はこれらの列から導く read model とし、期限切れ lease は reclaim 可能な pending、上限到達の期限切れ lease は terminal 回収待ちとして別に可視化する。未完了行を対象とする `(available_at, occurred_at, event_id)` partial index と expiry/reaper の query plan は実表の件数で検証し、大表の index 作成は lock 時間を確認して rollout step を分ける。

新規 `outbox_delivery_policy` の v0 単一行に `policy_id`、`max_attempts`、backoff/lease の許容範囲、policy revision を置く。全 process は起動時と claim 前に DB policy と設定の一致を確認し、不一致なら claim/reap を拒否する。claim は初回に `attempt_limit = DB policy.max_attempts` を固定し、以後はその行の limit を使う。reaper は local 設定値で terminal 判定しない。policy の変更は稼働 process を揃えた別 rollout とし、既存行の `attempt_limit` を暗黙変更しない。旧行で `attempt_count >= DB max_attempts` かつ `attempt_limit IS NULL` があれば worker を起動せず件数と ID を監査し、履歴を保持したまま運用判断を記録する。旧行の一律 reset、payload 変更、黙った terminal 化はしない。

P7 が Search schema に持つ `search_source_coordination`（名称は Freeze で確定）は `source_id PK`、単調増加 `fence_epoch BIGINT`、nullable `owner_token UUID` / `lease_expires_at`、durable current pointer とその `last_published_epoch` を持つ。Source lease の取得は短い DB transaction で空きまたは期限切れの同一行を lock し、`fence_epoch + 1`、新 owner token、DB clock の expiry を commit する。overflow は fail closed。renew/release は `source_id + owner_token + fence_epoch + lease_expires_at > DB clock` に条件付ける。失効した owner は解放・再使用せず、新しい取得で別 epoch にする。P7 の pointer、receipt、Source coordination と Domain `outbox_events` は **v0 では同一 PostgreSQL database** に置く。これで後述の短い publish transaction が両 fence を原子的に検査できる。Graph/lexical artifact は staging と readiness を持てば別媒体でもよいが、current pointer と Search receipt は同じ transactional backend に置く。この配置を満たせない案は P6/P7 の別設計・検証まで production 接続しない。

Search receipt table は少なくとも `(source_id, event_id)` PK、`generation_id`、manifest `digest`、`fence_epoch`、`recorded_at` を保存する。generation を物理的に残すための pin ではない。既存 `IndexingReceiptStore::put` の無条件上書き契約は durable 接続へ持ち込まない。`complete_event_if_current` 相当の条件付き Search port に置き換える。P3 が提案する Source current pointer / evaluation pin lease / GC の正本は P7 coordinator とし、Graph に独立 pointer を作らない。

## 3. generic worker の短い SQL transaction

以下は SQL 契約の擬似形であり、実装計画では列名・bind 型・index plan を実 DB で確認する。各 statement は `READ COMMITTED` の短い transaction、時刻は DB の `clock_timestamp()` を一 statement 内で一度得た値とする。`FOR UPDATE SKIP LOCKED` は queue の claim 競合回避であって処理中の lock や厳密な aggregate 順序保証ではない。PostgreSQL の [SELECT locking clause](https://www.postgresql.org/docs/current/sql-select.html)、[row lock](https://www.postgresql.org/docs/current/explicit-locking.html)、[clock functions](https://www.postgresql.org/docs/current/functions-datetime.html) に従う。

```sql
-- claim: 呼出し前に free dispatch permit を確保する。
-- Search route はさらに Source lease 取得済みで limit = 1。
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t),
picked AS (
  SELECT o.event_id
  FROM outbox_events AS o CROSS JOIN tick
  WHERE o.delivered_at IS NULL AND o.dead_lettered_at IS NULL
    AND o.available_at <= tick.t
    AND (o.lease_expires_at IS NULL OR o.lease_expires_at <= tick.t)
    AND o.attempt_count < COALESCE(o.attempt_limit, $db_policy_max)
  ORDER BY o.available_at, o.occurred_at, o.event_id
  LIMIT $free_permits
  FOR UPDATE OF o SKIP LOCKED
)
UPDATE outbox_events AS o
SET attempt_count = o.attempt_count + 1,
    attempt_limit = COALESCE(o.attempt_limit, $db_policy_max),
    lease_token = gen_random_uuid(), lease_owner = $owner,
    lease_expires_at = tick.t + $lease_interval,
    last_attempt_at = tick.t
FROM picked, tick
WHERE o.event_id = picked.event_id
RETURNING o.*;
```

`gen_random_uuid()` は PostgreSQL 14 以降の組込み関数で、batch の **行ごとに異なる** token を作る（[PostgreSQL UUID functions](https://www.postgresql.org/docs/14/functions-uuid.html)）。対象 DB 版は migration gate で確認する。claim commit 直後から claim 済み全行の lease 管理を開始し、dispatcher は claim を保持したまま semaphore 待ちさせない。実行時の `limit <= free_permits` が必須で、`batch_size` はその上限に過ぎない。**dispatch 直前**にも `renew(event_id, token)` を実行して current token / 未完了 / `lease_expires_at > DB clock` を確認し、1 行更新を得た event だけ handler に渡す。0 行または DB 応答不明なら dispatch しない。これが P2-1 の修正である。Search bridge は Source lease も同時に preflight renew する。

`renew` は下記の条件付き更新で 1 行か 0 行かを区別する。handler 中は outbox と Source の両 lease を期限より十分短い間隔で更新する。片方でも失効/応答不明なら cancel signal を送り、その結果から Search 完了・outbox ack/fail を推定しない。**publish 直前**に両 token を再確認し、最終の短い Search transaction でも再検査する。1 回の延長上限と総処理時間上限を設定し、超過時は retryable/unknown とする。

`settle_success` は Search durable 完了 commit を確認した場合だけ呼ぶ。`settle_failure` は retryable かつ `attempt_count < attempt_limit` なら lease を消し、`available_at=DB clock+bounded backoff/jitter`、allowlist error code を記録する。terminal error または上限到達なら `dead_lettered_at=DB clock` として lease を消し、原 event は保持する。3 操作の具体的な fence は次のとおり。`$terminal` と `$backoff` は検証済み値を bind する。

```sql
-- renew
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t)
UPDATE outbox_events AS o
SET lease_expires_at = tick.t + $lease_interval
FROM tick
WHERE o.event_id = $id AND o.lease_token = $token
  AND o.delivered_at IS NULL AND o.dead_lettered_at IS NULL
  AND o.lease_expires_at > tick.t
RETURNING o.event_id;

-- ack
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t)
UPDATE outbox_events AS o
SET delivered_at = tick.t,
    lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL
FROM tick
WHERE o.event_id = $id AND o.lease_token = $token
  AND o.delivered_at IS NULL AND o.dead_lettered_at IS NULL
  AND o.lease_expires_at > tick.t
RETURNING o.event_id;

-- fail: one bounded retry or terminal transition
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t)
UPDATE outbox_events AS o
SET available_at = CASE WHEN $terminal OR o.attempt_count >= o.attempt_limit
                        THEN o.available_at ELSE tick.t + $backoff END,
    dead_lettered_at = CASE WHEN $terminal OR o.attempt_count >= o.attempt_limit
                            THEN tick.t ELSE NULL END,
    last_error_code = $allowed_error_code,
    lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL
FROM tick
WHERE o.event_id = $id AND o.lease_token = $token
  AND o.delivered_at IS NULL AND o.dead_lettered_at IS NULL
  AND o.lease_expires_at > tick.t
RETURNING o.event_id;
```

0 行は Lost、DB error は unknown と区別し、旧 token で再 update しない。ack commit 応答不明なら DB の `delivered_at` を読み、未完了なら再配送を許容する。

`reap_exhausted` は **起動時、毎 poll cycle の claim 前、shutdown drain の終端**に bounded batch で実行する。各 process が実行可能で、複数 process の重複は `FOR UPDATE SKIP LOCKED` と `dead_lettered_at IS NULL` で防ぐ。DB policy と一致しない process は reaper も実行しない。具体的な回収は次の一 statement とする。

```sql
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t),
exhausted AS (
  SELECT o.event_id
  FROM outbox_events AS o CROSS JOIN tick
  WHERE o.delivered_at IS NULL AND o.dead_lettered_at IS NULL
    AND o.attempt_limit IS NOT NULL
    AND o.attempt_count >= o.attempt_limit
    AND (o.lease_token IS NULL OR o.lease_expires_at <= tick.t)
  ORDER BY o.last_attempt_at NULLS FIRST, o.event_id
  LIMIT $reap_batch
  FOR UPDATE OF o SKIP LOCKED
)
UPDATE outbox_events AS o
SET dead_lettered_at = tick.t,
    lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL,
    last_error_code = 'delivery_unknown_at_limit'
FROM exhausted, tick
WHERE o.event_id = exhausted.event_id
RETURNING o.event_id;
```

更新件数を metric/alarm に出す。最終 claim 後の crash や Search 成功後 ack 不明の可能性があるため **失敗確定ではなく結果不明の terminal** と記録する。DB outage 中は reaper 成功を推定せず、復旧後の起動/周期で回収する。`attempt_limit` 未設定の旧上限行は前節の監査対象である。これが P2-2 の修正であり、原 event の削除や自動再投入はしない。

## 4. Source fence、公開、receipt の一つの整合境界

Search bridge は Source lease を取得してから outbox claim し、Source epoch と outbox token を `DeliveryContext` に渡す。Source lease は全 Source 再読・Projection/lexical/Graph staging 中も heartbeat する。staging は event/epoch に対応する一意の generation key へ不可視に書く。Source lease と outbox lease のどちらかを失ったら cooperative cancellation を伝え、次の step と公開を開始しない。既に開始した非同期の build が直ちに止まるとは保証しない。残った staging は P7 の未公開 cleanup 対象である。公開済み/current/pinned generation を stale cleanup が削除できない CAS を P7 に要求する。

`DocumentOutboxIndexer::gate` は現在 instance-local であり、process 間の排他に数えない。P7 coordinator は設定済み `SourceId` の行を事前に作成し、`acquire_source` を次の一 statement で行う。別 process は取得できず outbox を claim しない。renew / release は同じ `source_id + owner_token + fence_epoch + lease_expires_at > tick.t` を条件にした 1 行更新だけを成功とする。lease 失効後に旧 process が動き続けても、新しい Source epoch が旧 epoch より大きい。CAS 再試行は D4 の有限回に従うが、Source contention 自体を retry attempt に数えない。これが P2-3 の修正である。

```sql
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t)
UPDATE search_source_coordination AS s
SET fence_epoch = s.fence_epoch + 1,
    owner_token = gen_random_uuid(),
    lease_expires_at = tick.t + $source_lease_interval
FROM tick
WHERE s.source_id = $source_id
  AND (s.owner_token IS NULL OR s.lease_expires_at <= tick.t)
  AND s.fence_epoch < 9223372036854775807
RETURNING s.owner_token, s.fence_epoch, s.lease_expires_at;
```

新 generation を publish する場合、P7 `complete_event_if_current` は **短い同一 PostgreSQL transaction** で以下を行う。manual `rebuild()` は outbox token のない別経路だが、同じ Source fence / pointer 条件を使う。

1. `outbox_events` の event 行を `FOR UPDATE` し、token と未完了、`lease_expires_at > DB clock` を確認する。続けて `search_source_coordination` の Source 行を `FOR UPDATE` し、owner token、epoch、`lease_expires_at > DB clock` を確認する。両方を満たさなければ transaction を rollback して Lost とする。lock 順は全経路で outbox → Source に固定し、manual rebuild は Source のみを lock する。
2. candidate generation の Projection/lexical/Graph artifact が同じ manifest と digest で durable READY であることを確認する。外部媒体の readiness は CAS 前に検証し、欠落・破損なら公開しない。candidate はこの transaction の終了まで P7 の staging lease で保護し、並行 cleanup が消せないようにする。`current_generation IS NOT DISTINCT FROM expected_current` と `last_published_epoch <= epoch` が成立する場合だけ Source pointer を candidate に切り替える。CAS 敗北は receipt を書かず rollback、最新 Source を再読する。P3 の Graph READY 判定も同じ transactional publish 境界へ接続する。
3. 同じ transaction 内で `(source_id,event_id)` receipt を候補 generation/digest/epoch で insert、または **既存 `fence_epoch < new_epoch` のときだけ**更新する。同一 epoch の既存行は generation/digest が完全一致する場合のみ idempotent success、異なる場合は error とする。pointer の現値が候補でなくなった場合は receipt を書かず rollback する。例えば次の SQL の 0 行は、既存行の照合に成功した場合を除き transaction 全体を rollback する。commit を確認して初めて `Published` と呼ぶ。

```sql
INSERT INTO search_index_receipts
    (source_id, event_id, generation_id, digest, fence_epoch, recorded_at)
VALUES ($source_id, $event_id, $generation_id, $digest, $epoch, clock_timestamp())
ON CONFLICT (source_id, event_id) DO UPDATE
SET generation_id = EXCLUDED.generation_id,
    digest = EXCLUDED.digest,
    fence_epoch = EXCLUDED.fence_epoch,
    recorded_at = EXCLUDED.recorded_at
WHERE search_index_receipts.fence_epoch < EXCLUDED.fence_epoch
RETURNING event_id;
```

`Unchanged` / `Duplicate` でも `get` の結果だけで成功にしない。同じ短い transaction で outbox と Source の両 fence、**current pointer が receipt 候補の key/digest と一致し READY であること**を確認し、上記の monotonic receipt upsert/照合を行う。条件が崩れたら再読または retryable とし、generic outbox ack はしない。現行 D4 の `put(event_id, receipt)` と `publish_if_current` が別々の port であるため、P7 接続時はこの結合 port を追加して `reconcile_once` の Published/Unchanged/Duplicate 全経路を通す必要がある。既存 memory 実装の挙動を durable 契約として流用しない。

Source 行の epoch と current pointer、receipt は一 transaction で直列化する。outbox lease が期限直前に有効と判定され、短い transaction 中に時刻が進んでも、行 lock が新 owner の reclaim を commit まで待たせる。commit 後の outbox ack が期限切れなら Lost として重複配送を許すが、古い epoch の公開/receipt が新 epoch の commit を上書きすることはない。DB lease は任意の consumer side effect を取り消さない。この保証は、Search の可視状態を上記 CAS だけで変更し、外部 build を不可視かつ generation-keyed に保ち、loss 時に handler が協調的に停止する境界に限る。任意の外部送信を持つ別 handler はその先にも idempotency/fence が必要である。

GC 後の歴史的 receipt は「当時処理した event の metadata」として保存してよいが、generation artifact への参照を再利用可能な pin と扱わない。duplicate 判定は **現在 pointer と同じ key/digest が READY** の場合だけ成立する。receipt が旧/GC 済み generation を指す場合は最新 Source を再読し、新 epoch で current generation の receipt に更新する。GC は current または評価 pin 中の generation を消さず、receipt による旧 generation の永続保持も要求しない。公開後に outbox ack が失敗した場合は再配送で current/receipt を検証して収束する。manual rebuild の成功だけで pending event を ack しない。これが P2-4 の修正である。

## 5. shutdown、故障、検証境界

SIGTERM/CTRL-C では Source lease の新規取得と outbox claim を止める。in-flight の二つの lease を更新しつつ bounded drain し、Search 完了を確認できた行のみ fenced ack/fail する。deadline 時は cancellation を送り、未完了行を ack/fail せず lease expiry と起動時/周期 reaper に委ねる。source lease の release は token/epoch 条件付きで行い、失効していれば触らない。`kill -9` では両 lease expiry 後に別 process が Source epoch を進めて回復する。最終試行が未 ack なら reaper が `delivery_unknown_at_limit` として DLQ に保存する。DB/consumer outage 中は bounded reconnect/backoff と alarm、無制限の claim/renew 連打はしない。

実 PostgreSQL qualification は少なくとも次を独立に固定する。

| 再現条件 | 必須 assertion |
| --- | --- |
| generic `batch_size=32` / `max_in_flight=8` / handler が lease より長い | claim は free permit 以下、claim 済み未開始の失効 dispatch は 0、dispatch 前 renew Lost は handler 未呼出、試行上限を待機だけで消費しない。Search route は常に claim 1。 |
| 最終 claim commit 直後の `kill -9`、lease expiry、複数 reaper/restart | 原行と `attempt_count` を保持し、ちょうど一度 terminal 更新、`delivery_unknown_at_limit` 可視、再 claim 0。異なる local `max_attempts` の worker は起動/claim/reap 拒否。 |
| 同 Source の burst、2/4/8 process、処理中 owner 停止/lease 失効 | 同時有効 Source owner は 1、standby は先取り claim なし、epoch は単調増加、健康な競合だけで正常 event が DLQ に行かない。generic 多 worker の disjoint claim は別試験。 |
| G1 publish 前/後の旧 handler 停止、期限切れ、G2 publish+receipt+ack 後の旧 handler 復帰 | 旧 epoch と旧 outbox token の publish/receipt/ack/fail は 0 行または Lost。current は G2、receipt は G2 またはより新しい current のみを示し、restart/GC 後も duplicate 判定が旧 artifact を dereference しない。 |
| outbox ack commit 応答不明、Search publish transaction commit 応答不明、receipt write failure | DB を再読して確認できない限り成功扱いせず、再配送でも current pointer/digest と receipt が整合し、二重可視 publish はない。 |
| expiry 境界時刻、renew/reclaim/ack/fail、stale cleanup | expiry ちょうどの旧 token 更新は 0、旧 epoch は公開不可、current/pinned artifact は削除不可。 |
| additive migration と旧 pending/delivered、Audit outbox | 行数/identity/payload/delivery 状態を保存し、Audit outbox の `delivered_at` と Document 正本を変えない。旧上限行の監査/起動拒否を確認。 |

性能測定、error/lag metric、最小権限、trace の妥当性、`aggregate_version` と `processing_state` の normative 差分、at-least-once / unordered delivery は元設計 §3, §6–7 を維持する。特に `aggregate_version` を異種 producer payload から一律 backfill しない。今回の設計で exactly-once、P7 durable runtime の稼働、製品 SLO、merge/deploy を達成したとは扱わない。

## 6. 実装責任の分離と Freeze 条件

1. **generic P6 crate** `crates/outbox-delivery`: Domain 非依存の claim/renew/fenced settle/reaper、free permit 制御、bounded loop と shutdown。Search 型と P7 coordinator を import しない。generic 多 worker の実 DB 試験を所有する。
2. **Search bridge** `crates/search-source-document` と `search-application`: event allowlist、Source lease 取得後の claim route、cooperative cancellation、`complete_event_if_current` を通る全 outcome、durable receipt と縦断試験を所有する。Source coordinator の実装と pointer/pin/GC は P7 と単一契約で接続する。P3 Graph READY 連携は P3/P7 の責任境界を維持する。
3. **shared migration / integration**: `0009`、Search coordinator/receipt migration、workspace `Cargo.toml` / `Cargo.lock`、process composition root、DB role 権限を一つの writer 境界で統合する。現行 task graph の `p6-implement.write_scope=crates/outbox-delivery` だけでは 2/3 を編集できないため、Freeze と plan で write scope・依存順を別 task にする。Domain の長い業務 transaction は作らず、Source build の間は DB row lock を保持しない。

Freeze には、この同一 DB の transactional fence、receipt / GC の意味、Source 単位の claim 前 lease、DB policy / 旧上限行の処置、規範との差分を明記する。続く独立 architecture recheck と実 DB qualification が通るまでは本書を GO 判定としない。ユーザーは Completion Program の通常の自律進行を許可済みであり、本設計改訂そのものに人手の承認ゲートを追加しない。
