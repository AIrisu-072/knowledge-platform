# P3 typed n-ary HyperGraph backend PoC

2026-09-30 JST。**選定: PostgreSQL 18.6 の relation/participant incidence schema を P3 durable adapter の実装候補にする。** これは三方式の isolated PoC の選定であり、P3/P7 の READY/pointer/pin/GC/fence 実装資格や本番公開を意味しない。P7 draft が要求する Source control、outbox、Graph の単一 PostgreSQL transaction を実現しやすく、実測した 3,000 relation group でも oracle parity、relation-only delta、別 container restore が成立した。redb は lookup が速いが P7 と別の commit protocol が必要で、Neo4j Community も同じ理由に加えて別 DB 運用と GPLv3 の評価が必要になる。性能の絶対比較として採用しない。

## 境界と fixture

- SSOT: `docs/superpowers/programs/search-platform-completion/p3-graph-freeze.md`（改訂設計、build guard、FK-safe cleanup の合成）、`p7-runtime-contract-draft.md`。本 PoC は `experiments/search-graph-poc/**` 内だけで実行し、root Cargo/lockfile、production crate、他 container を変更していない。
- [main.rs](src/main.rs) が seed `314159` から 100 / 1,000 / 3,000 relation group を生成する。各 group は 4 Resource、1 つの typed n-ary `loan` relation に `borrower` 1、反復 `product` 2、`collateral` 1 を含む。qualifier `TypedValue::List` の順、provenance、evidence、authority を保存する。resource group 2 は nanosecond と `+05:30` offset、freshness anchor/basis、effective interval を持ち、group 3 は半開境界で不可視になる。
- `baseline` と `updated` は同じ Resource 集合。後者は **Resource を変えず** same-ID relation qualifier の List 順序変更、relation 1 件削除、新 relation 1 件追加を行う。実 `search-graph-memory::MemoryGraphRetriever` に `GraphTraversalPlan` を渡し、7 case×2 generation の path/participant/evidence/authority/current-access/temporal 結果を生成する。case は base、false composite、relation-only add/delete、denied participant、half-open resource、missing seed。各 backend に保存した行を再取得して結果を照合する。100 group fixture では、Source 2 に同じ bare ResourceId/RelationId を別 provenance で stage し、Source 1 への混入がないことも三方式で確認した。
- PostgreSQL は `generation/resource/relation/participant` と incidence index、exact `NUMERIC(30,0)` nanoseconds/offset 列を使用。redb は Source/generation を含む relation/resource key と Resource/role/relation incidence key を使用。Neo4j は relation-as-node と role/ordinal 付き `P3_PARTICIPANT` edge を使用。単なる resource 間 clique は使っていない。
- `bench.py` の path 比較は **1 hop** の PoC evaluator であり、要求された production traversal の全 budget、multi-hop、Source current access port を実装していない。backend は persisted resource を再読するが、アクセスの拒否は合成 `denied` 指定であり、実 Source adapter の認可証明ではない。redb は persisted row から再構築した実 memory oracle とも照合する。

## 実測

macOS arm64、Docker Linux arm64。測定前後の空きは 7.4→1.4 GiB（他作業の変動を含む）で、専用一時物の削除後 3.6 GiB。各 size は独立 reset、100 回の warm **raw incidence lookup**。p50/p95/p99 の単位は µs。PG は local psycopg、Neo4j は localhost Query API v2、redb は同 process read transaction。通信・runtime 差を含む integration measurement であり、engine-only latency や実 API E2E の比較ではない。

| backend / relation group | ingest ms | lookup p50 / p95 / p99 µs | relation-only incremental ms | independent updated full ms | primary storage evidence |
| --- | ---: | ---: | ---: | ---: | --- |
| PG / 100 | 26.3 | 101 / 175 / 191 | 10.0 | 18.4 | DB 9.6 MB |
| PG / 1,000 | 165.8 | 91 / 109 / 137 | 71.0 | 104.2 | DB 22.4 MB |
| PG / 3,000 | 436.5 | 604 / 648 / 705 | 207.9 | 456.1 | DB 50.9 MB、data dir 168 MiB |
| redb / 100 | 35 | 21 / 24 / 36 | 33 | 27 | file 2.1 MB |
| redb / 1,000 | 167 | 16 / 18 / 38 | 205 | 174 | file 16.8 MB |
| redb / 3,000 | 514 | 17 / 20 / 73 | 607 | 521 | file 67.4 MB |
| Neo4j / 100 | 411.6 | 2,982 / 4,410 / 7,029 | 125.2 | 65.3 | 1,500 nodes |
| Neo4j / 1,000 | 594.5 | 2,156 / 3,363 / 4,610 | 480.0 | 389.5 | 15,000 nodes |
| Neo4j / 3,000 | 2,730.3 | 2,478 / 8,082 / 10,055 | 1,232.2 | 1,176.6 | 45,000 nodes、data dir 571 MiB |

全 9 run で baseline/updated の 7 query×2 が memory oracle と一致し、updated incremental と別 key の full build の relation payload 集合が一致した。redb 3,000 の process check は `/usr/bin/time -l` で最大 RSS 176,095,232 bytes。Docker `stats --no-stream` の 3,000 時点で PG 約 91 MiB、Neo4j 約 828 MiB（Neo4j heap 上限 256 MiB、pagecache 128 MiB）。RSS は取得時点と対象 process の違いがあるため厳密な同条件比較ではない。Neo4j 100 の再実行は、他の restore container が並走する状態で p50 8,207 µs となり、上表には入れていない。

PG の 3,000 run で `valid_from_ns=100000000000`、`valid_from_offset=19800`、`freshness_anchor_ns=99123456789`、effective [100000000000,101000000000) を実 DB の列から確認した。lookup の `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)` は実行 0.359 ms、shared hit 8 blocks（別時点の単発 probe）。同一 generation row の競合 `FOR UPDATE` は `lock_timeout=100ms` で SQLSTATE `55P03` を観測した。これは READY mutation trigger、P7 lock 順、lease、GC の資格試験ではない。

## restart / backup / restore

| backend | 観測 |
| --- | --- |
| redb | close→open 後、3,000 group の updated seed から 2 relation を取得。copy した 67,375,104-byte DB を再 open して baseline/updated 14/14 oracle query が一致。再 open 時間は 100 / 1,000 / 3,000 で 10 / 68 / 198 ms。copy は clean close 後の offline copy。 |
| PostgreSQL | `pg_dump -Fc` 905 KiB を別の専用 PG 18.6 container へ `pg_restore`。復元後 DB 51,492,543 bytes、baseline/updated 14/14 oracle query 一致。 |
| Neo4j Community | 専用 container 停止後、同一 pinned image の `neo4j-admin database dump/load` で 7.7 MiB dump を別の専用 data dir/container に復元。45,000 nodes と baseline/updated 14/14 oracle query 一致。Community の dump/load は offline 作業。 |

専用 container は `p3-poc-20260930-pg` (`9b73e9ef3607790984906bdc8b3bcc0ac480f2db4f050fcadd268675df2d68bd`)、`p3-poc-20260930-neo` (`17e9d1ad8fe368c0d26648cb6bfd75e60593fd5b62228d68d33fbe6b35253ec1`)、restore PG (`61d535e0dbd1398431039b10232e55bf69d94efa41154d88fb845559d666e3f5`)、restore Neo4j (`b75b1eaff1136e3fd3082218ae796725ef59258d2f6dbe450ecba90df8564b38`) の 4 件。すべて `docker rm -f` で削除済み。専用 data dir、PoC target、redb DB は証拠記録後に削除し、`data/backups/pg-3000.dump` と `neo4j.dump`、fixture JSON、source/lockfile を保持した。Docker の `127.0.0.1::PORT` は再起動後に host port が変わったため毎回 `docker inspect` を使う。

## 再現コマンドと pin

```sh
cd experiments/search-graph-poc
uv venv .venv
uv pip install --python .venv/bin/python -r requirements.txt
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target cargo run --locked -- make 100 fixture-100.json
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target cargo run --locked -- redb fixture-100.json redb-100.db
# 専用 PG/Neo4j container を下記 pinned image、localhost dynamic port、synthetic auth で起動後:
.venv/bin/python bench.py --backend pg --port "$(docker port p3-poc-20260930-pg 5432/tcp | sed 's/.*://')" fixture-100.json
.venv/bin/python bench.py --backend neo --port "$(docker port p3-poc-20260930-neo 7474/tcp | sed 's/.*://')" fixture-100.json
```

同じ `make` と三つの runner を `1000`、`3000` に適用した。PG は既存 `postgres:18.6-bookworm` image ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`、Neo4j Community は `neo4j:2026.09.0` digest `sha256:91fb0bf237c41b7b3dcbe84703aa0b82e0d7d067b16e1c8ab21f03fc679edf4e`。redb は `=4.3.0`、`Cargo.lock` に解決版を固定。Python は `psycopg[binary]==3.2.10`。`--locked` は生成済み lockfile と一致するかを確認する。local container は synthetic credential、外部 provider/有料 API は使用しなかった。

保持 fixture の SHA-256 は 100: `489a780c5c1152b9d530c7c1134a25577a1a035c1194eb221149f03041f4ac3b`、1,000: `b7ed5b68caab488b7c1b58870943052900159ee789a1eb718097d131e94b2f91`、3,000: `f98ac755d524c15656d26c3a54a523b97236bba206e1c69effdd4f48d153e3fb`。backup SHA-256 は PG: `3621c21383a3593aba489e84b39974180edfc884d7969c58cbebbad089a41506`、Neo4j: `0b5e0f11a53e2f2f5a838a03fb4b22b0fc32996e8b7592347f2e0a5da4b11cba`。fixture/backup は `.gitignore` 対象の local evidence で、versioned source と seed から再生成できる。

Docker run の主要引数は PG: `--memory=1g -e POSTGRES_PASSWORD=p3syntheticpass -e POSTGRES_DB=p3poc -p 127.0.0.1::5432 -v "$PWD/data/postgres:/var/lib/postgresql" postgres:18.6-bookworm`、Neo4j: `--memory=2g -e NEO4J_AUTH=neo4j/p3syntheticpass -e NEO4J_server_memory_heap_max__size=256m -e NEO4J_server_memory_pagecache_size=128m -p 127.0.0.1::7474 -v "$PWD/data/neo4j:/data" neo4j:2026.09.0`。当日の Neo4j は initial heap も 256m に固定した。restore は `pg_dump -Fc`/`pg_restore`、`neo4j-admin database dump/load` を別の専用 data dir/container で実行した。

## 選定条件と未資格項目

1. PostgreSQL incidence を選ぶ理由は P7 と同一 DB transaction の設計整合、exact NUMERIC/offset 保存の実 DB 確認、oracle/update/restore 合格、PostgreSQL License。redb の microsecond lookup は計測上の優位だが、P7 outbox/pointer/lease と別 storage に跨る atomicity をこの PoC は解決していない。Neo4j の Graph モデルは relation-as-node で表現できるが、HTTP integration overhead、data dir/RSS、別 DB coordination と GPLv3 評価が残る。
2. 本 PoC の PG schema に **READY child mutation trigger、Source control/pointer、evaluation lease、build guard、guarded GC、canonical graph digest、owner mapping commitment は未実装**。Neo4j stage は複数 HTTP transaction、redb は単一 writer fileで、いずれも production fence を証明しない。PG の incremental は DB baseline copy と relation-only delta、redb は DB copy と delta、Neo4j は baseline fixture の unchanged relations を新 generation へ O(base) 挿入して delta を足すため、更新時間は同一アルゴリズムではない。
3. 1-hop/100 warm lookup 以外の high-degree、multi-hop、1/2/4/8 reader+writer、cold cache、hidden-degree timing distribution、kill -9、corrupt row/index/digest、concurrent stage↔validate、publish↔pin/GC、expired lease、CAS loss、permission role/triggers、Source owner/FolderPlacement spoof、負 offset と空文字 freshness basis、full P7/E2E、hosted exact-head gate は未実施。production adapter 前に P3/P7 plan の real DB fault tests と Security/Architecture review が必要。

## 公式資料

- [PostgreSQL 18 numeric](https://www.postgresql.org/docs/18/datatype-numeric.html)、[explicit locking](https://www.postgresql.org/docs/18/explicit-locking.html)、[PostgreSQL License](https://www.postgresql.org/about/licence/)
- [redb 4.3.0 API と MIT/Apache-2.0](https://docs.rs/redb/4.3.0/redb/)、[crate metadata](https://docs.rs/crate/redb/4.3.0/source/Cargo.toml)
- [Neo4j Community Docker pin](https://neo4j.com/docs/operations-manual/current/docker/introduction/)、[Query API v2](https://neo4j.com/docs/query-api/current/query/)、[Community GPLv3](https://neo4j.com/open-source-project/)、[offline dump/load](https://neo4j.com/docs/operations-manual/current/docker/dump-load/)
