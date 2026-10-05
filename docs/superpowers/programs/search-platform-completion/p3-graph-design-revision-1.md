# P3 Durable HyperGraph — 設計改訂 1

Status: **DRAFT / 未凍結**（2026-09-30）。本書は p3-graph-design.md を残したまま、p3-graph-architecture-review.md の P2-1〜5 を反映した再審査対象である。PostgreSQL は暫定第一候補であり、製品選定、PoC 成功、復旧成功、production 接続を宣言しない。

## 1. 正本、修正対応、責務

規範は Search v0 Approved Design §22–27/34/54/62、Transaction & Consistency Requirements SD-T1〜T6。現行 search-core の TypedRelationInstance / GraphTraversalPlan と search-graph-memory の traversal を意味論 oracle とする。現行 P7 は durable current pointer、evaluation pin lease、Projection / lexical / Graph の一体公開・復旧を担当する。P3 は Graph artifact の格納・検証・取得を担当し、P7 接続前の isolated backend を稼働中の Search runtime と呼ばない。

| review 指摘 | 本書の修正 |
| --- | --- |
| P2-1 READY、publish、pin、GC の競合 | §4–5: BUILDING 行だけの stage、DB 側の READY 不変 fence、同一 PostgreSQL transaction と Source→generation の lock 順、lease 失効と retry |
| P2-2 owner 引数のすり替え | §3、§6: owner は検証済み generation resource row と Source snapshot の canonical ID 対応からのみ取得。Document / FolderPlacement の一対一 owner と Version の直接評価 |
| P2-3 TemporalProjection の欠落 | §3: valid / effective の四境界に freshness anchor / basis を足した全 field の SQL 表現と digest |
| P2-4 relation-only delta 欠落 | §7: resource が不変でも relation ID の add / update / delete を closure に含め、旧行を除去してから replacement |
| P2-5 絶対 timing 保証 | §8: 公開値の存在秘匿と timing の残余リスクを分け、検証可能な挙動と security review を定義 |

## 2. 不変の Graph 意味論

1. 永続 key は常に (source_id, generation_id, resource_id) と (source_id, generation_id, relation_id)。裸 ResourceId / RelationId は Source 間または generation 間で結合しない。
2. 一つの TypedRelationInstance が一つの typed n-ary HyperEdge である。relation row と同じ relation_id に属する participant 全集合で from_role / to_role / required_participants を評価する。別 relation の borrower / product / collateral を合成しない。discovery / semantic / evidence namespace、繰返し role、TypedValue qualifier の List 順序、半開 temporal、authority、provenance、evidence refs、path evidence を lossless に保つ。participant / evidence の集合的順序だけを canonicalize する。二項 shortcut は正本ではない。
3. GraphTraversalPlan の検証、seed / hop / relation / branch / path budget、authority と temporal filter、未対応 stop_conditions の拒否、出力順・重複除去・candidate identity は oracle に合わせる。全 participant の現在 Source access が Allowed である relation だけを展開する。不可視枝は visible branch / relation budget に数えない。budget 超過は partial success にしない。
4. Source retention が PersistentResource または PersistentDiscoveryMetadata でないときは durable stage を拒否する。SESSION_ONLY / NO_RETENTION の relation は persistent Graph に入れない。remote miss / outage から削除を推定しない。Graph path は retrieval trace であり Primary claim evidence に昇格しない。
5. READY generation は内容と manifest binding が不変で、current pointer とは別概念。新 generation は別 key へ構築し、旧 pin の key / digest / 結果を暗黙に変更しない。失敗・CAS 敗北で pointer を戻さない。同一 snapshot の full / incremental は論理等価とする。

## 3. PostgreSQL 第一候補の物理契約

追加 migration は Search 所有の crates/search-graph/migrations/0001_search_graph_v1.sql とし、Document 正本 migration を変更しない。search_graph schema は別権限とし、Domain / Application に SQLx 型を公開しない。次は候補 schema の必須列・制約であり、実装 SQL と index は PoC / plan で確定する。

| table | key、必須列と制約 |
| --- | --- |
| search_graph.generation | (source_id,generation_id) PK、graph_schema_version=typed-nary-v1、projection_manifest_digest、source_snapshot、source_mapping_digest、resource_count、relation_count、graph_content_digest、state、timestamps。state は BUILDING / READY / FAILED / DELETING。count 非負。manifest / source_snapshot / mapping / digest / counts は READY 以降更新不可。READY は Graph の準備完了であり公開ではない。 |
| search_graph.resource | (source_id,generation_id,resource_id) PK と generation FK、resource_kind、resource_version_id、source_native_document_id、folder_id、owner_document_id、全 TemporalProjection 列。Document / FolderPlacement の owner_document_id は NOT NULL、他 Source kind の owner 必須性は登録済み Source adapter 契約で検証。Document row は folder_id 不要、FolderPlacement row は folder_id 必須。owner key は外部 candidate に返さない。 |
| search_graph.relation | (source_id,generation_id,relation_id) PK と generation FK、namespace、relation_type、versioned typed qualifiers、temporal scope、authority、provenance、evidence refs、canonical payload digest。型と半開区間を検証する。(source_id,generation_id,namespace,relation_type,relation_id) を候補 index とする。 |
| search_graph.participant | (source_id,generation_id,relation_id,ordinal) PK、role、resource_id。同一 generation の relation / resource への複合 FK、(source_id,generation_id,relation_id,role,resource_id) UNIQUE、role 非空、(source_id,generation_id,resource_id,role,relation_id) incidence index。role は複数 resource に反復可能。 |

Resource の temporal は TemporalProjection の resource_ref、valid_from、valid_to、profile.freshness_anchor_at、profile.freshness_basis、profile.effective_from、profile.effective_to を全て保存する。resource_ref は PK と一致させる。五つの Optional OffsetDateTime はそれぞれ nullable な signed epoch-nanoseconds NUMERIC(30,0) と offset-seconds INTEGER の対で保存し、両方 NULL または両方非 NULLを CHECK する。DB 側の TIMESTAMPTZ への丸めを経由しない。Rust の OffsetDateTime が表現できない値、区間の from >= to、無効な offset は stage / recover で拒否する。freshness_basis は nullable UTF-8 text とし、NULL と空文字を区別する。valid / effective は [from,to) とし、NULL は境界なしであって未知値を勝手に埋めない。NUMERIC の比較・index 費用は PoC で測る。

graph_content_digest は版付き domain separator と length framing を使い、Source ID、graph schema version、resource_id 順の kind / resource_version / owner / folder / Source native mapping / 上記五 timestamp の instant と offset / freshness_basis、relation_id 順の typed payload と participant / evidence refs を含める。Optional は明示タグで NULL と空値を分け、timestamp は signed epoch nanoseconds と offset seconds を固定幅または長さ付き canonical encoding にする。qualifier map の key と participant / evidence の集合的順序は安定整列し、TypedValue::List の順序は保持する。generation ID、build time は除外し、ProjectionGenerationManifest.digest と別欄に保存する。同じ canonical encoder を full / incremental / validate / recover が使い、None、nanosecond、offset、半開境界、freshness anchor / basis を roundtrip・digest parity で確認する。

relation payload と participant index 列は二重の独立正本にしない。stage と recover は row を TypedRelationInstance に再構成し、TypedRelationInstance::validate、canonical payload digest、全 participant の同 generation FK、attachment 先全 resource の同一 canonical relation 定義を検査する。FK、UNIQUE、CHECK に加え、§4 の row-level mutation fence を DB 側に置く。source/manifest/count/owner/digest の不一致は READY として扱わない。

## 4. BUILDING→READY と DB 側の mutation fence

P3 の stage_full / stage_incremental は新しい generation を BUILDING として作る。stage の全 batch transaction は最初に当該 generation row を SELECT ... FOR UPDATE し、state=BUILDING を確認してから resource / relation / participant を変更する。同じ row の子表 INSERT / UPDATE / DELETE にも BEFORE row trigger を設け、親 generation row を FOR UPDATE で取得して BUILDING 以外の通常変更を拒否する。これは adapter 以外の書込みと validate の競合も塞ぐ。親 row がない DML は FK と trigger で拒否する。

validate_ready は **一つの PostgreSQL transaction** で generation row を FOR UPDATE し、BUILDING のまま全 row / owner / relation / count / manifest binding / digest を再読・検証し、同じ transaction の最後に READY と検証済み digest / counts を確定する。lock 待ちの stage は READY 確定後に再判定して失敗する。stage 中の crash は未 commit batch を rollback し、残る BUILDING を recovery cleanup の対象にする。READY を BUILDING へ戻す transition と READY 内容の UPDATE / INSERT / DELETE は DB trigger / grants で拒否する。

削除だけは P7 の guarded GC が §5 の source lock と generation lock を保持した transaction で READY / FAILED / BUILDING を DELETING にし、participant→relation→resource→generation の順に明示削除する。子 FK は ON DELETE RESTRICT とし、DELETING 中の子 DELETE のみ専用 coordinator DB 操作に許す。READY を直接 DELETE しない。state transition trigger は READY→BUILDING、DELETING→READY 等を拒否し、DELETING と物理削除の権限は P7 coordinator の DB 操作に限定する。P3 builder には READY 退役の直接権限を与えない。schema 権限・trigger を無効化できる DB 管理者は trust boundary 外であり、運用監査対象とする。

## 5. P3/P7 共有 PostgreSQL transaction と lock 順

PostgreSQL を採用する場合、P7 の durable source control/current pointer と evaluation lease を Graph と**同じ database**に置く。source control は Source ごとに必ず一行存在し、source_id、current_generation_id、current_manifest_digest、pointer_revision を持つ。新 Source では一意 key の INSERT ... ON CONFLICT DO NOTHING を済ませてから lock する。lease は (source_id,evaluation_id,lease_id) を一意 key とし、generation_id、manifest / graph digest、expires_at を持つ。pointer / lease の直接 DML は拒み、P7 coordinator の transaction-bound DB 操作を唯一の mutation 経路にする。

全 publish_if_current、pin_current、lease renew / release、discard_unpublished、retire_unpinned は同じ順序を守る: **source control row FOR UPDATE → 必要な generation row を key 順に FOR UPDATE → lease row を id 順**。複数 Source を扱う操作は Source ID 順に分ける。stage / validate は generation lock だけを取り、後から source lock を取りに行かない。P7 は application port の背後にある infrastructure adapter で transaction を開始し、P3 repository の private transaction-bound 操作へ同じ接続を渡す。SQLx Transaction / PgConnection を search-core や search-application の public type に出さず、別接続で READY 確認、pointer 更新、lease / GC を分割しない。外部 lexical file の I/O や Source access 呼出しは source lock 保持中に実行しない。

| 操作 | 一つの transaction 内の確定順 |
| --- | --- |
| publish_if_current | Projection / lexical / Graph artifact の同一 manifest と digest を事前検証。source lock 後、expected current key と pointer_revision を再確認し、同じ DB 内の Projection READY と対象 Graph generation lock 下の READY、manifest digest、graph digest、count を再確認する。条件付き pointer CAS を最後に一回実行し、成功時だけ commit。CAS 敗北は pointer を変えず、敗北 artifact は別の guarded discard へ渡す。 |
| pin_current | source lock 下で current key を一度選び、対象 generation lock 下で READY と manifest / graph digest を確認し、同じ transaction で evaluation lease を INSERT して commit した後に key / lease を返す。current が変われば次の pin は新 key、既存 pin は旧 key。未公開 READY key を任意指定して pin できない。 |
| retire_unpinned / discard_unpublished | source lock 下で pointer と DB clock 時点の有効 lease を再読し、対象 generation lock 後にも一致を確認する。current または有効 lease の key は拒否する。DELETING 遷移、子行と親行の削除を**同じ transaction**で commit する。CAS 敗北 cleanup も同じ確認を通す。 |

P7 が未接続のとき P3 は READY の delete / retirement を公開 port から提供しない。P3 単体の BUILDING cleanup も公開 pointer / lease の非存在を証明できる隔離 fixture に限る。P7 の source control が unavailable なら GC は拒否する。lock wait、deadlock、SQLSTATE 40001 / 40P01 は transaction 全体を rollback し、同じ expected key / revision と idempotency key から bounded retry する。retry 後も条件を再評価し、CAS 敗北を成功へ変換しない。上限後は明示 failure とし partial commit を返さない。Read Committed で連続 SELECT が同じ snapshot になるとは仮定せず、共有 source lock と generation lock、および commit 直前の条件付き更新で直列化する。

Lexical 等の DB 外 artifact は CAS 前に存在と digest を検証する。CAS 後に失われたら別 generation へ暗黙 rebind せず、該当評価を fail closed とし Source snapshot から再構築する。CAS commit 後の event receipt 保存だけが失敗した場合は公開済み artifact を保持し、同じ event の再処理を既存 D8 の Unchanged と receipt 保存に収束させる。Graph READY と pointer の atomicity だけで外部 lexical file まで原子的になったとは主張しない。

Query は P7 が server-side に発行した評価 lease と key を使う。外部 request が generation ID / evaluation ID を自己申告しても pin 権限にしない。P3 backend を外部へ直接公開せず、P7 wrapper が lease と key を検証してから呼ぶ。Graph reader は READY の snapshot を REPEATABLE READ READ ONLY の read transaction で読み、結果返却前に P7 が新しい transaction と DB clock で lease の key / expiry を再確認する。期限切れ、lease 不在、Graph 欠損では旧 key から新 key へ静かに移らず、結果全体を明示失効 / unavailable とする。GC が期限切れ lease の後に削除を始めても、Query は再確認に失敗して partial result を返さない。pin / retire の線形化点は各 transaction の commit、Query の有効性判断は返却前の lease 確認時点とする。lease の長さ・renew cadence は P7 plan に記録し、無期限 pin にしない。

PG の row lock、Read Committed の snapshot、deadlock retry は [PostgreSQL isolation](https://www.postgresql.org/docs/current/transaction-iso.html)、[explicit locking](https://www.postgresql.org/docs/current/explicit-locking.html) に基づく。BEFORE trigger と exact NUMERIC の選択は [trigger behavior](https://www.postgresql.org/docs/current/trigger-definition.html)、[numeric type](https://www.postgresql.org/docs/current/datatype-numeric.html) を参照する。上記は設計要求であり、migration / role / trigger の実 DB 検証前に成立済みとはしない。

## 6. 信頼済み owner と現在認可

追加 application port の概念形は次のとおり。SQLx 非依存の型だけを置く。GraphResourceRecord の kind / resource_version / temporal / Source native mapping / owner は Source adapter が同じ authoritative snapshot から作り、公開 request や retrieval candidate が指定できない。

    GraphResourceRecord {
        resource_ref, kind, resource_version_ref,
        temporal: TemporalProjection,
        source_native_mapping, owner_document_ref?, attached_relations[]
    }
    GraphGenerationReceipt {
        key, projection_manifest_digest, source_mapping_digest, graph_content_digest,
        resource_count, relation_count, graph_schema_version
    }
    DurableGraphGenerationPort:
        stage_full(manifest, source_retention, source_mapping_digest, resources, relations) -> key
        stage_incremental(manifest, source_retention, base_key, delta) -> key
        validate_ready(key) -> GraphGenerationReceipt
        recover_ready(key, expected_manifest_digest) -> GraphGenerationReceipt
    GenerationScopedGraphAccessPort:
        evaluate(key, resource_ref, access_context) -> AccessDecision

stage_full の relations 入力は同一 relation ID の複数 Source attachment を受け取り得るが、participant ごとの attached_relations と突き合わせ、重複する relation 定義が同一であることを確認してから一件へ正規化する。evaluate は (source,generation,resource) の検証済み resource row を backend が読み、その kind と owner を Source 別 current-access adapter に渡す。呼出側 owner 引数は存在しない。resource row 不在、mapping 欠落、不一致、Source adapter 不在、Denied / Unknown / error は不可視。Graph に保存した AccessProjection は最終認可に使わない。access_context と evaluation lease は信頼済み API 境界で作られ、公開 request の文字列をそのまま権限にしない。

Document Source では ResourceKind::Document と FolderPlacement の**全件**に、同じ Source snapshot から得た一対一の DocumentId owner が必須であり、余剰 owner と重複を拒否する。stage / validate / recover は保存 row から document_resource_id(source,owner) と folder_resource_id(source,owner,folder_id) を再計算して resource_id に一致させ、DirectoryProjection.kind、source native DocumentId / FolderId、manifest.source_snapshot の canonical mapping commitment と照合する。FolderPlacement を共有 Folder ID として扱わない。source_mapping_digest は Source adapter が authoritative snapshot の canonical ID 対応から発行して generation に固定し、recover は row から再計算して一致を検証する。再取得可能な snapshot がある場合は同じ mapping と再照合する。commitment や mapping を検証できなければ fail closed / rebuild とする。owner と kind は graph digest に含める。

現行 Document の主検索 Resource は ResourceKind::Knowledge、ResourceId は現行 DocumentVersionId と同じ値である。これは構造行の owner を流用せず、保存済み resource_version_ref と resource_ref の一致、および Source snapshot の version→Document 対応を stage / recover で検査した上で、DocumentCurrentAccessAdapter の version 直接評価に委譲する。現在の Version / Read / publication end は query ごとに Source 正本で再判定する。Document / FolderPlacement は保存済み owner_document_id に対する現在の Read を評価し、同じ Folder にある別 Document の許可を流用しない。他 Source の kind は登録済み Source adapter が row から current authority を解決できる場合のみ可視にする。

同じ裸 ResourceId を二つの Source に置く oracle parity fixture では、現行 memory oracle の CurrentAccessEvaluatorPort(ResourceId, ...) を Source 別に束縛した evaluator で実行する。Source を渡さない旧 port の単一 evaluator を cross-Source 比較に使わない。

## 7. Full / incremental と relation closure

GraphIncrementalDelta は changed_resources、retired_resources、changed_relation_ids、retired_relation_ids、replacement_relations、target_source_mapping_digest と、Source adapter の authoritative snapshot / complete relation closure proof を持つ。proof は base / target snapshot ID、対象 resource と relation の scope、旧 relation ID 集合、新 relation ID 集合、Source の complete enumeration または authoritative change stream の根拠を束縛する。P3 は旧集合を base READY 行、新集合を replacement と照合する。単なる「変更通知を見た」という主張を complete proof にしない。Source が旧・新の対象 segment を完全列挙できるか、関係 ID の update / delete を authoritative に特定できることを契約とする。証明できない partial Source は full rebuild へ切り替え、full も不能なら READY にしない。

新 BUILDING generation へ base READY の row を複製した後、影響 ID 集合 A を「changed / retired resource に接する base relation」「changed_relation_ids」「retired_relation_ids」「replacement_relations の既存 ID」の和として決める。A の participant を全削除し、次に A の relation を削除する。その後 retired / changed resource を反映し、最後に replacement relation と全 participant を挿入する。同 ID の qualifier / authority / provenance / temporal / evidence / participant 変更は旧 ID を丸ごと退役してから replacement を一度だけ挿入する。relation-only add は新 ID を挿入、relation-only delete は旧 ID を削除、旧 ID→新 ID の置換は両方を delta に明記する。

Source は resource 変更に接する**新 snapshot の全 relation**と relation-only 変更の全 replacement を供給する。残すべき同 ID relation も closure 内なら再供給する。新 participant は新 generation の resource に全員存在し、relation attachment は全参加 resource で同じ canonical 定義とする。A の算出、削除、再挿入、count / digest 検証は BUILDING fence 下で行う。resource が一切変わらず relation qualifier だけ変わる場合、authority の取消、evidence の追加、participant の差替え・削除も同じ規則を通す。full と incremental の graph digest、traversal hit / path / evidence parity が一致しなければ公開しない。初期 PG 案は O(base rows) copy となり得るため実測対象である。

## 8. 外部挙動、timing threat model、受入

正常な generation と現在 Source access が動作する範囲で、欠落 seed と未認可 seed は同じ空結果と response class にする。未認可 relation / participant は candidate、path、evidence、件数、visible budget error に入れない。個々の access error は Unknown として抑止し、外部へ resource 固有の error / trace / owner を出さない。plan invalid / visible budget 超過だけを定義済み request error とする。backend integrity 障害や lease 失効は全体を fail closed にし、partial result を返さない。内部監査・metrics の resource ID と拒否理由は管理者境界に置く。

脅威モデルは、認証された Search 利用者が同じ Source に反復 query を送って HTTP status、response body / count、概略の wall-clock latency を観測できる場合とする。relation incidence scan と各 participant の current access 回数は隠れた degree に依存し得る。上記の公開値非漏洩を受入要件とする一方、**microtiming / latency distribution の完全な noninterference は主張しない**。hidden relation 数で発火する request-time 物理 scan cap / timeout を、欠落 seed と未認可 seed で異なる外部 status / body / count に変換してはならない。その条件を満たせない cap は使わず、全 generation に共通の build-time 制約、または DB 参照前に公開 plan のみから判定する制約にする。visible branch / relation budget は現行 oracle と同じ位置で適用する。運用 timeout と障害の latency / error 差は残余 side channel として測定・記録する。Graph reader は batch 化、内部 pagination、Source access cache を検討できるが、認可省略や出力順変更を許さない。P5 の API 側 rate / concurrency limit と P7 の運用監視を組み合わせ、反復 timing probe の残余リスクを security review に記録する。padding / strict timing 保証が必要という判断が出た場合は、別の測定 gate と明示的な security 決定を設ける。

## 9. 候補選定、PoC、後続工程

PostgreSQL incidence を第一候補に置く理由は、既存 SQLx / PostgreSQL 境界と P7 pointer / lease を同じ transaction に置ける見込みである。redb は immutable KV adjacency と ACID / MVCC、Neo4j Community は reified relation vertex を比較候補とする。redb の別ファイル、Neo4j の別 service は PG pointer と原子的に commit できないので、選定するなら同等の publication / pin / GC fencing と crash recovery protocol を先に設計・独立 review しなければならない。Neo4j Community の GPLv3 と offline backup、redb の Rust dependency / backup 条件も選定証拠に含める。現時点では製品採用も具体的 SLO も未決定である。

隔離 PoC は production Cargo / lockfile を触らず、同じ fixture generator、GraphTraversalPlan、Source 別 access oracle で三候補を比較する。必須 correctness fixture は n-ary role / false composite / cross-Source 同一裸 ID、全 TemporalProjection の NULL / nanosecond / offset / 半開境界、owner 欠落・入替・FolderPlacement spoof、Version 直接評価、現在権限取消、relation-only add / same-ID update / delete、full↔incremental digest / path parity、visible budget と access error である。

PG 実 DB 故障注入は concurrent stage↔validate、READY 直後の stage 拒否、publish↔pin、pin↔GC、lease expiry↔Query、CAS 敗北 cleanup、kill -9 / restart、backup→別環境 restore、row / index / digest 欠損を含む。source / generation lock 順と DB grants / triggers を実際に検証する。合成 corpus の小・中・大、high-degree、1/2/4/8 reader と writer、cold / warm で build / incremental / traversal p50/p95/p99、lock wait、WAL / disk / RSS、backup / restore、EXPLAIN (ANALYZE, BUFFERS) を記録する。timing probe は欠落 / 未認可 seed と hidden-degree 差の分布を測り、残余リスクを security review に渡す。閾値と母集団は PoC plan で事前固定する。Docker daemon 到達だけを PoC 成功とみなさない。

独立 architecture 再審査で本改訂の P2 が閉じた後、p3-graph-freeze.md は意味論と選定規則、p3-graph-plan.md は PoC 手順、DB role / trigger / lock 実装、port と write scope を確定する。P3 isolated adapter は crates/search-graph/**、必要な SQLx 非依存 port は search-application/src/ports.rs、Document wiring は search-source-document/src/outbox.rs、P7 coordinator / lease は別責務とする。現 task-graph の p3-implement 書込み範囲を超える変更は controller が割当を更新してから行う。isolated backend、Document runtime 接続、P7 multi-index E2E、exact-head qualification を receipt で分ける。

Design Freeze の破壊、不可逆 migration、live deploy / credential / payment、法務 license blocker、重大な security tradeoff、業務意味論を変える代替案は Completion Program の Hard Stop とする。通常の schema / index / adapter 具体化と今回の P2 修正は設計工程内の自律修正であり、追加の人手承認を要求しない。
