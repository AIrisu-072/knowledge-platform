# audit-acceptance

Audit Infrastructure v1 単位Cの受入試験（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §14.3・§14.4、[運用手順](../../docs/operations/audit-delivery-store.md)）。production codeを持たない試験専用crate（`publish = false`、依存はすべてdev-dependency）である。Documentの実際の業務transaction（productionのapplication service・PostgreSQL repository・file-system storage）から、`audit_outbox_events` → relay → Audit Storeまでを通し、失敗と復旧の証拠を取る。データはすべて合成（`test-idp/editor` 等）である。

## 環境

- testcontainers `postgres:18.6-bookworm` の1 containerに、Document DBとStore DBの2 databaseを作る。
- Document DBは非superuserのLOGIN role `document_app`（schemaのowner）がDocument migrationを行い、全producerを実行する。`audit-relay migrate` と `roles.sql` はsuperuserが行う（`crates/audit-relay/README.md` の順序）。superuserは他に、試験用の妨害と読取専用の照合だけに使う。
- relayとStoreの実行経路は `roles.sql` で作ったLOGIN role（worker、operator、relay service、verifier、maintainer、administrator、DBA）で接続する。
- relayは `audit_relay::relay::run`（`audit-relay run` そのもの）をprocess内で動かす。T3だけは同じ組立て（runner・breaker・handler・monitorの報告）でStoreのclientを差し替える。

## 試験と証明すること

| 試験 | 証明すること |
|---|---|
| T1 `journey` | relay originのDocument全23 type（bootstrapと通常のACL変更、作成、公開、WORKING更新、予約・取消、`service/scheduler` による予約公開と権限喪失による予約の終端、rebase、取下げ、公開終了、metadata、folder作成・改名・移動、文書移動、初回既読、VIEW/RESET、原本アクセス、Diff、revision比較（同一版・版違い）、managementと既読の拒否）の40件が、動作中のrelayでStoreにevent_idごとにちょうど1件届く。受領のseq・envelope digest・source commitmentがrelayの台帳と一致し、actor・subject・type・source・resource・result・time・correlationがstaging行と一致する。schedulerの実行は依頼者がactorのまま `service_executor` が `service/scheduler` になる。理由文はStoreに `{provided, utf8_bytes, text_retained}` だけが届き、理由文・本文・storage locator・storage root・ACLにだけ現れる主体はStore databaseの全表（`pg_dump --data-only`）とexportのどこにも無い。healthのproduced・delivered・stored（head）・verifiedが別々の値を示し、verify（1..head）がok、checkpoint・export・offline assessが `authentic` |
| T2 `staging_failure` | stagingのINSERT、またはrelayの登録trigger（`audit_relay.deliveries`）が失敗すると、業務操作（metadata、folder改名、ACL、2件のaudit行を持つ文書作成、既読、VIEW、公開終了、folder作成）はerrorになり、Documentの全表と配送登録の内容が変わらない（件数とdigest）。stagingも登録も残らず、healthには何も出ない。妨害を外した後の同じ操作はcommitされ、その分だけがStoreに1回ずつ届く |
| T3 `store_outage` | 配送中にStore databaseが止まる（ingestの直前に `ALLOW_CONNECTIONS false`＋接続の強制終了）。claim済みの行は試行を返却したoutage保留（`last_outage_code` が `store_*`、quarantineなし）になり、停止中も業務はcommitされ、行はpendingのまま試行0で残る。healthは `store_unavailable`・`circuit_open`・`outage_held` を出し、produced＝registered（staging失敗ではない）。復旧後、同じrelayが全件を1回ずつ届け、試行は1 |
| T4 `relay_crash` | 子process（このtest binaryのignored fixture）のrelayがStoreへ保存した直後・ack前にSIGKILLで終わる。`audit-relay run` で再起動すると、leaseの失効後に再claimされ、Storeは元のseqで `duplicate` を返し、試行は戻らず（2）、欠落も重複も無い。read-onlyのreconcileが全行ok・修復なしで記録され、healthに警報が無い |
| T5 `store_restore` | relay停止→帯域外checkpoint→`pg_dump`→backup後の配送→新しいdatabaseへの `pg_restore`→`privileges.sql` の再適用。復元したStoreはrecovery modeで、healthは `store_recovery_required` と `stored.relay_max_seq` を出す。`verify --recovery`、identity chainのrecovery exportとcheckpointの照合（match）、previewを帯域外のrecovery記録へ追記してその値でepochを開始（損失を隠す記録は拒否）、relayの再開と `reconcile --repair` で消失範囲の6件が再配送され、Storeに1件ずつ、履歴に `repair_reset_missing` が残る。権限・retentionの再適用、verify、checkpoint、export、assessは `lost`（記録なし・偽の記録は `unverified_recovery`）。上限不明のepoch（宣言のみ、checkpointもrelay seqも無し）も `lost` で、`authentic` にならない |
| `document_migration` | relay migrationの後にDocument ownerがstagingへnullable列を足しても、producer・登録・source digestは変わらず、前後の行が1回ずつ届く。被digest列のDROP・型変更は失敗する（設計§14.4の最後の項目） |

## 実行

```sh
cargo test --locked -p audit-acceptance            # Dockerが要る（postgres:18.6-bookworm）
cargo nextest run -p audit-acceptance              # CI（mise run test:rust）と同じ
```

6試験は並列で約15秒（1試験5–8秒）。各試験は大きなstackの専用threadと専用runtimeで動く（未最適化buildでproductionのfutureが既定のtest threadのstackを超えるため）。

## 含まないもの

- DSI workerとDiff worker（binary・PDFium）は使わない。意味検査と差分はprocess内の合成実装（`SyntheticInspection`、`SyntheticDiff`）で置き換える。audit行はすべてproductionのproducerが書く。
- `DueScheduler`（Linux・DSI worker・sandbox probeが要る）は構築せず、その `poll_once`（DBの時刻、`list_due`、`execute_due_authorized` と `scheduler_executor()`）を同じ順で呼ぶ。依頼者の再解決は試験用のidentity directoryで、PoCの `StaticRequesterResolver`（`poc` 主体だけ）ではない。
- HTTP層（`document-api-http`）とidentity adapterは通さず、routerが呼ぶapplication serviceを検証済みcontextで直接呼ぶ。
- CLI binary（`audit-relay`、`audit-admin`）はprocessとして起動せず、同じlibraryの入口（`relay::run`、`connect_for_health`＋`health`、`Reconciler`、`AuditAdmin`、`files::export_to_dir`、`assess_dir`）を使う。T4で殺すrelayは `Relay::assemble`＋`run_cycle` の子processで、monitor（circuit報告）を持たない。
- Store停止は同じclusterのdatabaseの接続拒否・切断であり、別hostの停止・network分断（timeout）ではない。Storeは同じcluster内の別databaseで、別serverではない。
- Document DBのbackup・restore、Document DBとStore DBを組で戻すrestore、同じdatabaseへのin-place restore（`store_regressed`）、replay・quarantine・retention・purgeは、ここでは実producerの行で繰り返さない（単位Bの試験が合成行で扱う）。
- Search・OrganizationのsourceとDocument以外のproducer（設計§13）。
