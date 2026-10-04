<a id="p6-durable-outbox-delivery-worker--設計改訂-1"></a>
# P6 永続 Outbox 配送ワーカー — 設計改訂 1

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

[公開原文の固定リンク（commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c`）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design-revision-1.md)

本書は意味保存の日本語訳であり、原設計の再承認・実装/資格の追加ではない。既存ハッシュは当時の原文/証拠のものであり、訳文のハッシュではない。以下の状態・予定・合否は当時の記録として保持する。本文の行番号は同 commit の各原文を指し、従来の見出しアンカーも維持している。掲載コマンドは今回の翻訳作業では実行していない。

用語: Outbox は配送待ちイベントの永続表、Source は検索対象の情報源。リースは期限付き処理権、フェンスは古い所有者の更新を拒否する条件、エポックは所有権の世代番号、Search のイベント処理記録（receipt）は索引処理結果の記録、検証の receipt は試験・実行結果の証拠記録を指す。CAS は期待する現値との一致を条件にした更新、GC は不要世代の回収、DLQ は打切りイベントの保管先。Search / Domain / Document / Folder / AccessPolicy / Projection / Unit / Graph などの構成名・型名は識別のため維持する。

- 状態: **DRAFT / アーキテクチャ再審査前**。設計凍結、実装、実 DB による資格確認の証拠ではない。
- 日付: 2026-09-30 JST
- 対象: [元設計](p6-outbox-design.md) に対する [独立アーキテクチャレビュー](p6-outbox-architecture-review.md) の P2-1〜4。元設計の内容は変更しない。本書がこの 4 件に関する凍結候補の差分を定め、その他の境界は元設計を引き継ぐ。
- 根拠: `0001_document_authoritative_core.sql:74-101` の実表、`search-source-document/src/outbox.rs:47-58,76-121,642-712,807-925` の D4 実装、`p3-graph-design.md:77-81` の P7 調整器接続境界。いずれも P6/P7 の新機能が実装済みであることは示さない。

## 1. 不変の責務と採用する実行単位

Domain 生成側は業務変更と `outbox_events` へのイベント記録を一つのトランザクションでコミットする。汎用 P6 ワーカーだけがそのイベントの処理権取得、`attempt_count`、`available_at`、リース、再試行、`delivered_at`、DLQ を所有する。Search は正本 Source を再読した派生 Projection（射影）、現在の索引世代ポインター、Search イベント処理記録を所有する。Search 処理記録と Domain の `delivered_at` は別の証拠であり、どちらか片方だけで他方の成功を宣言しない。`audit_outbox_events` の行と配送確認は本経路の対象外である。

v0 の必須宛先は一つの Search 接続処理とする。`Document`、`Folder`、`AccessPolicy` のイベントは設定された一つの Document `SourceId` に帰属させる。`aggregate_id` を Source の排他キーとして使わない。イベント型と集約型の組合せは現行生成側と `DocumentIndexingService` の明示的な許可リストで検査し、`AccessPolicyChanged` が複数の集約型に発生することを許す。未知の Document 系イベントは黙って配送確認せず打切りエラーとして観測する。Search 接続処理はペイロードを索引正本にせず、D4 と同じく現行 Source のスナップショットを読む。`Published` / `Unchanged` / `Duplicate` は後述の永続化を完了するトランザクションが成立した場合だけ成功、`Ignored` は何もしないことを明示した経路以外では成功にしない。

Search の初期本番接続は **一 Source 当たり全プロセスを通じて実行中の整合化を一つ** とする。P7 `SearchGenerationCoordinator` の永続的な Source リースを **処理権取得より前**に取得した接続処理プロセスだけが、その Source のイベントの処理権を取得する。v0 の Search 経路は一回に 1 件の処理権を取得し、Search ハンドラーが完了または取り消されるまで次の処理権を取得しない。Source リースはイベント処理ごとに解放し、次の取得でフェンスエポックを増やす。待機プロセスは Outbox 行を先取りせず、Source リースの解放/期限を上限を設けてポーリングする。汎用 P6 の複数ワーカー / バッチ処理権取得能力とは分けて試験する。P7 の共有実行基盤が接続される前にプロセス内限定 `MemoryDocumentIndexRuntime` の複数複製インスタンスを本番と呼ばない。

<a id="2-additive-migration-と-db-policy"></a>
## 2. 既存を保持した追加型のマイグレーションと DB 方針

`0009_outbox_delivery_v0.sql` は既存 `outbox_events` の全列、PK、既存行、既存生成側の挿入を保持する。元設計の `lease_token UUID`、`lease_owner UUID`、`lease_expires_at TIMESTAMPTZ`、`last_attempt_at`、`dead_lettered_at`、許可リスト `last_error_code`、任意の検証済み `traceparent/tracestate` に加え、`attempt_limit INTEGER NULL` を追加する。追加 CHECK は `attempt_limit IS NULL OR attempt_limit > 0`、`(lease_token IS NULL AND lease_owner IS NULL AND lease_expires_at IS NULL) OR (lease_token IS NOT NULL AND lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)`、`delivered_at IS NULL OR dead_lettered_at IS NULL`、`(delivered_at IS NULL AND dead_lettered_at IS NULL) OR lease_token IS NULL` とする。旧行は追加列が全て NULL なのでこれらを満たす。既存配送済み行は `DELIVERED`、既存保留行は `PENDING` のままで、イベント/ペイロード/時刻を再作成しない。`processing_state` はこれらの列から導く読み取りモデルとし、期限切れリースは再取得可能な保留状態、上限到達の期限切れリースは打切り回収待ちとして別に可視化する。未完了行を対象とする `(available_at, occurred_at, event_id)` 部分インデックスと有効期限/回収処理のクエリ実行計画は実表の件数で検証し、大表の索引作成はロック時間を確認して段階的展開手順を分ける。

新規 `outbox_delivery_policy` の v0 単一行に `policy_id`、`max_attempts`、再試行待機/リースの許容範囲、方針改訂を置く。全プロセスは起動時と処理権取得前に DB 方針と設定の一致を確認し、不一致なら処理権取得/回収を拒否する。処理権取得は初回に `attempt_limit = DB policy.max_attempts` を固定し、以後はその行の上限を使う。回収処理はローカル設定値で打切り判定しない。方針の変更は稼働プロセスを揃えた別段階的展開とし、既存行の `attempt_limit` を暗黙変更しない。旧行で `attempt_count >= DB max_attempts` かつ `attempt_limit IS NULL` があればワーカーを起動せず件数と ID を監査し、履歴を保持したまま運用判断を記録する。旧行の一律初期化、ペイロード変更、黙って打切り状態へ移行することはしない。

P7 が Search スキーマに持つ `search_source_coordination`（名称は凍結で確定）は `source_id PK`、単調増加 `fence_epoch BIGINT`、NULL 許容の `owner_token UUID` / `lease_expires_at`、永続的な現在のポインターとその `last_published_epoch` を持つ。Source リースの取得は短い DB トランザクションで未取得または期限切れの同一行をロックし、`fence_epoch + 1`、新しい所有者トークン、DB 時刻に基づく有効期限をコミットする。桁あふれは安全側に拒否。リース更新/解放は `source_id + owner_token + fence_epoch + lease_expires_at > DB clock` に条件付ける。失効した所有者の権限は解放・再使用せず、新しい取得で別エポックにする。P7 のポインター、処理記録、Source 調整と Domain `outbox_events` は **v0 では同一 PostgreSQL データベース** に置く。これで後述の短い公開トランザクションが両フェンスを原子的に検査できる。Graph/字句索引成果物は準備領域と準備完了判定を持てば別媒体でもよいが、現在のポインターと Search 処理記録は同じトランザクション対応の保存基盤に置く。この配置を満たせない案は P6/P7 の別設計・検証まで本番接続しない。

Search の処理記録表は少なくとも `(source_id, event_id)` PK、`generation_id`、マニフェスト `digest`、`fence_epoch`、`recorded_at` を保存する。索引世代を物理的に残すための固定参照ではない。既存 `IndexingReceiptStore::put` の無条件上書き契約は永続保存基盤との接続に持ち込まない。`complete_event_if_current` 相当の条件付き Search ポートに置き換える。P3 が提案する Source 現在のポインター / 評価用保持リース / GC の正本は P7 調整器とし、Graph に独立ポインターを作らない。

<a id="3-generic-worker-の短い-sql-transaction"></a>
## 3. 汎用ワーカーの短い SQL トランザクション

以下は SQL 契約の擬似形であり、実装計画では列名・バインド型・インデックス実行計画を実 DB で確認する。各 SQL 文は `READ COMMITTED` の短いトランザクション、時刻は DB の `clock_timestamp()` を一つの文内で一度得た値とする。`FOR UPDATE SKIP LOCKED` はキューの処理権取得競合回避であって処理中のロックや厳密な集約順序保証ではない。PostgreSQL の [SELECT のロック節](https://www.postgresql.org/docs/current/sql-select.html)、[行ロック](https://www.postgresql.org/docs/current/explicit-locking.html)、[時刻関数](https://www.postgresql.org/docs/current/functions-datetime.html) に従う。

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

`gen_random_uuid()` は PostgreSQL 14 以降の組込み関数で、バッチの **行ごとに異なる** トークンを作る（[PostgreSQL UUID 関数](https://www.postgresql.org/docs/14/functions-uuid.html)）。対象 DB 版はマイグレーションゲートで確認する。処理権取得コミット直後から処理権取得済み全行のリース管理を開始し、配送実行部は取得済み処理権を保持したままセマフォ待ちさせない。実行時の `limit <= free_permits` が必須で、`batch_size` はその上限に過ぎない。**配送実行直前**にも `renew(event_id, token)` を実行して現在のトークン / 未完了 / `lease_expires_at > DB clock` を確認し、1 行更新を得たイベントだけハンドラーに渡す。0 行または DB 応答不明なら配送実行しない。これが P2-1 の修正である。Search 接続処理は Source リースも同時に直前更新する。

`renew` は下記の条件付き更新で 1 行か 0 行かを区別する。ハンドラー中は Outbox と Source の両リースを期限より十分短い間隔で更新する。片方でも失効/応答不明なら取り消しシグナルを送り、その結果から Search 完了・Outbox 配送確認/失敗確定を推定しない。**公開直前**に両トークンを再確認し、最終の短い Search トランザクションでも再検査する。1 回の延長上限と総処理時間上限を設定し、超過時は再試行可能/結果不明とする。

`settle_success` は Search の永続化完了コミットを確認した場合だけ呼ぶ。`settle_failure` は再試行可能かつ `attempt_count < attempt_limit` ならリースを消し、`available_at=DB clock+bounded backoff/jitter`、許可リスト内のエラーコードを記録する。打切りエラーまたは上限到達なら `dead_lettered_at=DB clock` としてリースを消し、原イベントは保持する。3 操作の具体的なフェンスは次のとおり。`$terminal` と `$backoff` は検証済み値をバインドする。

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

0 行は Lost、DB エラーは結果不明と区別し、旧トークンで再更新しない。配送確認コミット応答不明なら DB の `delivered_at` を読み、未完了なら再配送を許容する。

`reap_exhausted` は **起動時、毎ポーリングサイクルの処理権取得前、終了時の完了待機の終端**に上限付きバッチで実行する。各プロセスが実行可能で、複数プロセスの重複は `FOR UPDATE SKIP LOCKED` と `dead_lettered_at IS NULL` で防ぐ。DB 方針と一致しないプロセスは回収処理も実行しない。具体的な回収は次の一つの文とする。

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

更新件数をメトリクス/警報に出す。最終処理権取得後の異常終了や Search 成功後配送確認不明の可能性があるため **失敗確定ではなく結果不明の打切り** と記録する。DB 障害中は回収処理成功を推定せず、復旧後の起動/周期で回収する。`attempt_limit` 未設定の旧上限行は前節の監査対象である。これが P2-2 の修正であり、原イベントの削除や自動再投入はしない。

<a id="4-source-fence公開receipt-の一つの整合境界"></a>
## 4. Source フェンス、公開、処理記録の一つの整合境界

Search 接続処理は Source リースを取得してから Outbox の処理権を取得し、Source エポックと Outbox トークンを `DeliveryContext` に渡す。Source リースは全 Source の再読・Projection/字句索引/Graph の準備中も定期更新する。準備領域はイベント/エポックに対応する一意の索引世代キーへ不可視に書く。Source リースと Outbox リースのどちらかを失ったら協調的取り消しを伝え、次の手順と公開を開始しない。既に開始した非同期の構築が直ちに止まるとは保証しない。残った準備領域は P7 の未公開後片付け対象である。公開済み/現在の/保持中の索引世代を古い後片付けが削除できない CAS を P7 に要求する。

`DocumentOutboxIndexer::gate` は現在インスタンス内限定であり、プロセス間の排他に数えない。P7 調整器は設定済み `SourceId` の行を事前に作成し、`acquire_source` を次の一つの SQL 文で行う。別プロセスは取得できず Outbox の処理権を取得しない。リース更新 / 解放は同じ `source_id + owner_token + fence_epoch + lease_expires_at > tick.t` を条件にした 1 行更新だけを成功とする。リース失効後に旧プロセスが動き続けても、新しい Source エポックが旧エポックより大きい。CAS 再試行は D4 の有限回に従うが、Source 競合自体を再試行の試行数に数えない。これが P2-3 の修正である。

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

新索引世代を公開する場合、P7 `complete_event_if_current` は **短い同一 PostgreSQL トランザクション** で以下を行う。手動 `rebuild()` は Outbox トークンのない別経路だが、同じ Source フェンス / ポインター条件を使う。

1. `outbox_events` のイベント行を `FOR UPDATE` し、トークンと未完了、`lease_expires_at > DB clock` を確認する。続けて `search_source_coordination` の Source 行を `FOR UPDATE` し、所有者トークン、エポック、`lease_expires_at > DB clock` を確認する。両方を満たさなければトランザクションをロールバックして Lost とする。ロック順は全経路で Outbox → Source に固定し、手動再構築は Source のみをロックする。
2. 候補索引世代の Projection/字句索引/Graph 成果物が同じマニフェストとダイジェストで永続化済み READY であることを確認する。外部媒体の準備完了判定は CAS 前に検証し、欠落・破損なら公開しない。候補はこのトランザクションの終了まで P7 の準備領域リースで保護し、並行後片付けが消せないようにする。`current_generation IS NOT DISTINCT FROM expected_current` と `last_published_epoch <= epoch` が成立する場合だけ Source ポインターを候補に切り替える。CAS 敗北は処理記録を書かずロールバック、最新 Source を再読する。P3 の Graph READY 判定も同じトランザクション内の公開境界へ接続する。
3. 同じトランザクション内で `(source_id,event_id)` 処理記録を候補索引世代/ダイジェスト/エポックで挿入、または **既存 `fence_epoch < new_epoch` のときだけ**更新する。同一エポックの既存行は索引世代/ダイジェストが完全一致する場合のみ冪等な成功、異なる場合はエラーとする。ポインターの現値が候補でなくなった場合は処理記録を書かずロールバックする。例えば次の SQL の 0 行は、既存行の照合に成功した場合を除きトランザクション全体をロールバックする。コミットを確認して初めて `Published` と呼ぶ。

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

`Unchanged` / `Duplicate` でも `get` の結果だけで成功にしない。同じ短いトランザクションで Outbox と Source の両フェンス、**現在のポインターが処理記録候補のキー/ダイジェストと一致し READY であること**を確認し、上記の単調更新の処理記録挿入・更新/照合を行う。条件が崩れたら再読または再試行可能とし、汎用 Outbox 配送確認はしない。現行 D4 の `put(event_id, receipt)` と `publish_if_current` が別々のポートであるため、P7 接続時はこの結合ポートを追加して `reconcile_once` の Published/Unchanged/Duplicate 全経路を通す必要がある。既存メモリ実装の挙動を永続保存の契約として流用しない。

Source 行のエポックと現在のポインター、処理記録は一つのトランザクションで直列化する。Outbox リースが期限直前に有効と判定され、短いトランザクション中に時刻が進んでも、行ロックが新所有者の再取得をコミットまで待たせる。コミット後の Outbox 配送確認が期限切れなら Lost として重複配送を許すが、古いエポックの公開/処理記録が新エポックのコミットを上書きすることはない。DB リースは任意のコンシューマー副作用を取り消さない。この保証は、Search の可視状態を上記 CAS だけで変更し、外部構築を不可視かつ索引世代キー付きに保ち、喪失時にハンドラーが協調的に停止する境界に限る。任意の外部送信を持つ別ハンドラーはその先にも冪等性/フェンスが必要である。

GC 後の歴史的処理記録は「当時処理したイベントのメタデータ」として保存してよいが、索引世代成果物への参照を再利用可能な保持用の固定参照として扱わない。重複判定は **現在のポインターと同じキー/ダイジェストが READY** の場合だけ成立する。処理記録が旧/GC 済み索引世代を指す場合は最新 Source を再読し、新エポックで現在の索引世代の処理記録に更新する。GC は現在の索引世代または評価のために保持中の索引世代を消さず、処理記録による旧索引世代の永続保持も要求しない。公開後に Outbox 配送確認が失敗した場合は再配送で現在の索引世代/処理記録を検証して収束する。手動再構築の成功だけで保留中イベントを配送確認しない。これが P2-4 の修正である。

<a id="5-shutdown故障検証境界"></a>
## 5. 終了処理、故障、検証境界

SIGTERM/CTRL-C では Source リースの新規取得と Outbox の処理権取得を止める。処理中の二つのリースを更新しつつ上限付き終了待機し、Search 完了を確認できた行のみフェンス付き配送確認・失敗確定する。期限に達したら取り消しを送り、未完了行を配送確認/失敗確定せずリース失効と起動時/周期回収処理に委ねる。Source リースの解放はトークン/エポック条件付きで行い、失効していれば触らない。`kill -9` では両リース失効後に別プロセスが Source エポックを進めて回復する。最終試行が配送未確認なら回収処理が `delivery_unknown_at_limit` として DLQ に保存する。DB/コンシューマー障害中は上限付き再接続・再試行待機と警報、無制限の処理権取得/リース更新連打はしない。

実 PostgreSQL による資格確認は少なくとも次を独立に固定する。

| 再現条件 | 必須検証条件 |
| --- | --- |
| 汎用 `batch_size=32` / `max_in_flight=8` / ハンドラーがリースより長い | 処理権取得は空き実行枠以下、処理権取得済み未開始の期限切れでの配送実行は 0、配送実行前リース更新 Lost はハンドラー未呼出、試行上限を待機だけで消費しない。Search 経路は常に処理権取得 1。 |
| 最終処理権取得コミット直後の `kill -9`、リース失効、複数回収処理/再起動 | 原行と `attempt_count` を保持し、ちょうど一度打切り状態への更新、`delivery_unknown_at_limit` 可視、処理権の再取得 0。異なるローカル `max_attempts` のワーカーは起動/処理権取得/回収拒否。 |
| 同 Source の集中発生、2/4/8 プロセス、処理中所有者停止/リース失効 | 同時に有効な Source 所有者は 1、待機側は先取り処理権取得なし、エポックは単調増加、障害のない競合だけで正常イベントが DLQ に行かない。汎用の複数ワーカーの処理権取得の非重複は別試験。 |
| G1 公開前/後の旧ハンドラー停止、期限切れ、G2 公開+処理記録+配送確認後の旧ハンドラー復帰 | 旧エポックと旧 Outbox トークンの公開/処理記録/配送確認/失敗確定は 0 行または Lost。現在の索引世代は G2、処理記録は G2 またはより新しい現行索引世代のみを示し、再起動/GC 後も重複判定が旧成果物を参照しない。 |
| Outbox 配送確認コミット応答不明、Search 公開トランザクションコミット応答不明、処理記録書込み失敗 | DB を再読して確認できない限り成功扱いせず、再配送でも現在のポインター/ダイジェストと処理記録が整合し、二重可視公開はない。 |
| 有効期限境界時刻、リース更新/再取得/配送確認/失敗確定、古い後片付け | 有効期限ちょうどの旧トークン更新は 0、旧エポックは公開不可、現在の索引世代の成果物/保持中成果物は削除不可。 |
| 既存を保持した追加型のマイグレーションと旧来の保留行/配送済み行、Audit Outbox | 行数/同一性/ペイロード/配送状態を保存し、Audit Outbox の `delivered_at` と Document 正本を変えない。旧上限行の監査/起動拒否を確認。 |

性能測定、エラー/遅延メトリクス、最小権限、トレースの妥当性、`aggregate_version` と `processing_state` の規範上の差分、少なくとも一度 / 順序非保証配送は元設計 §3, §6–7 を維持する。特に `aggregate_version` を異種生成側ペイロードから一律埋戻ししない。今回の設計で正確に一度だけの配送、P7 永続実行基盤の稼働、製品 SLO、マージ/デプロイを達成したとは扱わない。

<a id="6-実装責任の分離と-freeze-条件"></a>
## 6. 実装責任の分離と凍結条件

1. **汎用 P6 クレート** `crates/outbox-delivery`: Domain 非依存の処理権取得/リース更新/フェンス付き状態確定/回収処理、空き実行枠制御、上限付きループと終了処理。Search 型と P7 調整器をインポートしない。汎用の複数ワーカーの実 DB 試験を所有する。
2. **Search 接続処理** `crates/search-source-document` と `search-application`: イベント許可リスト、Source リース取得後の処理権取得経路、協調的取り消し、`complete_event_if_current` を通る全結果、永続処理記録と縦断試験を所有する。Source 調整器の実装とポインター/保持/GC は P7 と単一契約で接続する。P3 Graph READY 連携は P3/P7 の責任境界を維持する。
3. **共有マイグレーション・統合**: `0009`、Search 調整器/処理記録マイグレーション、ワークスペース `Cargo.toml` / `Cargo.lock`、プロセス構成の組み立て箇所、DB ロール権限を単独の編集担当の範囲で統合する。現行タスクグラフの `p6-implement.write_scope=crates/outbox-delivery` だけでは 2/3 を編集できないため、凍結と計画で編集範囲・依存順を別タスクにする。Domain の長い業務トランザクションは作らず、Source の索引構築の間は DB 行ロックを保持しない。

凍結には、この同一 DB のトランザクション内のフェンス、処理記録 / GC の意味、Source 単位の処理権取得前リース、DB 方針 / 旧上限行の処置、規範との差分を明記する。続く独立アーキテクチャ再審査と実 DB による資格確認が通るまでは本書を GO 判定としない。ユーザーは Completion Program（完成プログラム） の通常の自律進行を許可済みであり、本設計改訂そのものに人手の承認ゲートを追加しない。
