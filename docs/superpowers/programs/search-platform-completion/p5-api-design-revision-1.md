# P5 Common Search / Discovery API — 設計修訂 1

- 状態: **DRAFT / P5 architecture review の P1・P2 修正案**。本書は `p5-api-design.md` の差分であり、P1〜P4 の revised/frozen contract との最終照合、`spec/`、OpenAPI、実装、実 HTTP 試験を完了したことを示さない。
- 規範: `spec/` が SSOT。承認済み Search / Discovery v0 design §4、§41、§62〜64 の Search と Discovery、Source-owned truth、S1、current access、NO_RETENTION、Graph 非公開の意味を変えない。
- 根拠: `p5-api-architecture-review.md`、`crates/search-application/src/{ports,discovery_service}.rs`、`crates/search-core/src/{discovery,source,evidence}.rs`、`crates/search-source-document/src/{evidence,postgres}.rs`、別 ref の `document-api-http` identity/error/limits。別 ref は P5 checkout に接続済みではない。

## 1. 四 operation 共通の trusted 入口

`POST /v1/search`、`POST /v1/discover`、`GET /v1/resources/{resourceId}`、`GET /v1/sources` は同じ composition root と request context を使う。既存 `VerifiedActorContext` には tenant がなく、`from_trusted_adapter` は認証処理ではない。P5 は trusted identity resolver が一回の検証で作る内部 `TrustedSearchContext { tenant: TrustedTenantContext, actor: VerifiedActorContext, session: Option<TrustedSessionBinding>, access_binding, deadline }` を追加する。tenant と actor の所属・有効期限を同時に検証する。複数 tenant に所属していて trusted session が対象 tenant を一意に定められなければ処理しない。HTTP body、path、query、未検証 header、cursor、provider 応答から tenant・principal・role・group・subjects・service executor・`InvocationKind` を採用しない。`InvocationKind` は監査属性であり権限ではない。内部 `access_binding` は request ごとに server が作り、同じ actor/tenant を保持する Source adapter だけが照合する。外部 DTO、log、cursor に出さない。

実装順序は次のとおり。各段階の失敗は後段を開始しない。

```text
trusted identity + tenant resolution
  → actor/session validity and operation authorization
  → tenant-scoped, actor-visible Source snapshot
  → public sourceIds intersection / visible Claim resolution / visible Resource locator
  → routing, generation pin, remote call, retrieval, qualification
  → final disclosure gate
  → safe DTO serialization
```

`ActorVisibleSourceRegistryPort::snapshot(&TrustedSearchContext)` は tenant 内で現在の Source **存在開示**を認められた `AuthorizedSourceSet` と、変更検知に使える可視性 stamp を返す。tenant-scope と Source-owned current visibility を一つの port 契約として強制し、provider の稼働確認・remote call より先に確定する。Source 個別の policy `Denied` / `Unknown` はこの request では不可視として集合へ入れず、ID・件数・障害・coverage・時間依存の error class を gap、trace、`partial`、Source 一覧に反映しない。registry/visibility 基盤自体の障害で完全な actor-visible snapshot を作れない場合は四 operation とも 503 の generic dependency failure とし、部分 Source 一覧を正常扱いしない。この基盤は隠れた provider の稼働状態を照会しないので、その障害で公開応答を変えない。可視性の authoritative stamp がない場合、初回の一覧・検索は可能でも完全な複数 page 継続を発行しない。

既存の projection / Graph generation key は `SourceId + generation` であるため、server-owned `SourceId` は同一 runtime 全体で一意とし、異なる tenant への同じ `SourceId` の割当ては registry の起動検証で拒否する。この一意性を認可の代用にはせず、全 lookup は引き続き trusted tenant に束縛する。初期 single-tenant 構成では、設定 tenant と異なる trusted tenant context を routing・locator より前に拒否する。caller に任意 tenant field を認めない。

現行 `SourceRegistryPort::list_sources()` は actor/tenant を受けず、`DiscoveryService::discover` がその結果を routing に直結する。P5 の公開入口はこれを直接呼べない型境界へ改める。`SearchQueryService` と `DiscoveryService` は private constructor の `AuthorizedSourceSet` またはそれに束縛した request-scoped registry だけを受け、`SourceRouter::plan`、generation pin、Graph、remote action、gap/trace 生成まで同じ集合に制限する。取得後の candidate は Source ID が集合内にあることを再確認し、Source すり替えを破棄する。port 不在・未配線なら四 route とも起動拒否する。

`sourceIds` は可視集合との intersection だけに使う。形式違反は公開 field の validation error、形式が正しい未知・別 tenant・不可視 ID は同じ空 intersection として扱う。これらを区別する `unknownSourceIds`、失敗数、route gap、trace、`partial` は作らない。結果を組み立てるときは最終 gate 後の可視 item から rank と count を再計算する。Source-specific gap/trace はその Source の**現在の存在開示**が許される場合に限り、固定 code だけを返す。許可されない Source を aggregate count にも含めない。

`ResourceLocatorPort` は `resolve_visible(tenant, AuthorizedSourceSet, resourceId)` の意味を持つ tenant-scoped port とする。裸 `ResourceId` の global uniqueness、provider native locator、projection だけで Source を決めない。lookup は可視 Source 集合の範囲に制限し、その後 Source/item/current publication を確認する。未知、他 tenant、不可視、旧版、T10 終了、権限取消は同じ `404 RESOURCE_NOT_FOUND`、同じ公開 body/header/cache policy にする。target の現在可視性を確定する前の target-specific lookup/access error も 404 とし、内部診断にだけ残す。可視性が確定した後の content 依存障害は generic 503/504 にできる。tenant 内の重複 mapping は内部 integrity incident として記録して任意の一件を選ばず、外部では一律 404 に倒す。未認可 caller に重複だけ別の 500/503 を返さない。内部調査用 locator/Source 名は Problem、trace、通常 audit に出さない。request duration の絶対的な非干渉は主張せず、可視性確定前の provider I/O を禁止し、同一 error class と bounded path の差を security test で測る。

## 2. Claim ID と最終開示

公開 `requiredClaimIds` は構文と個数だけを HTTP で検証する。現行 `ClaimSelectorPort::selector_for(generation, claim_id)` は actor/tenant を受けず、Document catalog は projection 内 assertion の存在を見ても caller visibility を見ない。P5 は二段の内部 port にする。`ActorVisibleClaimCatalogPort::bind(TrustedSearchContext, AuthorizedSourceSet, ClaimId)` が routing 前に tenant/source と現在の subject/field 可視性を確認して非公開 `VisibleClaimBinding` を返し、`selector_for_visible(binding, pinned generation)` が pin 後に同じ Source/generation を検証して selector を返す。前段が解決できない required Claim は routing へ渡さない。selector の subject、predicate、expected value は公開 DTO と free-form trace に出さない。

正しい形式の未知・他 tenant・不可視・失効 Claim ID は外部では同じ opaque unresolved outcome とする。初期 P5 案は `200 DiscoveryEvaluation`、`evidenceSufficiency=unresolved`、汎用の blocking required-claim gap とし、Claim 存在・Source ID・selector の差を validation 文、route、gap、trace、件数、時間依存 error class に出さない。不可視 required Claim を落として残りだけで `Sufficient` にしない。この wire outcome と Claim ID catalog の namespace は P1/P4 freeze 後に最終照合する。HTTP 422 は ID の形式・個数等の公開構文違反だけに使い、実在性の検査に使わない。

Search / Discover / Resource GET は、Source adapter が承認した field と同一 generation、current Version/publication、Source policy、item access、actor validity を**出力確定直前**に検査する。title、metadata、snippet、claim value、citation、evidence、Graph participant はそれぞれ field-specific grant の対象である。safe DTO は private buffer に作り、検査中に Source/item の取消、revision 変化、`Unknown`、backend error を観測した item は、そこから派生した evidence、citation、rank、count、matched field、gap/trace entry まで一体で破棄して再計算する。actor/session 自体が失効した場合は全 buffer を破棄して 401 とする。required evidence を失った Discovery は `Sufficient` にしない。安全に再計算できなければ generic 503/504 とし、検査前の buffer を送らない。HTTP はこの gate が終わるまで body を streaming しない。check 後に将来発生する取消との原子的な整合までは保証しない。

`SourcePage` も page 作成と送信前に Source 存在開示を再検査する。page cursor は現在の authoritative visibility revision、または同じ actor/tenant に対する可視集合の canonical digest に束縛する。集合の増減・取消・再付与を検知したら `409 CURSOR_STALE` で先頭から再開始させる。registry revision だけで visibility 変更を検知できない構成では `nextCursor` を発行しない。これにより、新たに見える小さい key の飛ばしを完了と誤認しない。

## 3. 公開 cursor と retention

初期 P5 は**server-side の bounded RAM session cursor**のみを採る。client に渡す `cursor` は OS の安全な乱数源から作る推測困難な handle で、意味のある payload を含まない。署名しただけの可読 payload、client へ出す native/provider cursor、encoded Source ID・candidate key・principal fingerprint は禁止する。handle も bearer secret として log、URL query を含む access log、trace、analytics から除く。HTTP `no-store` と session binding を併用する。暗号化 token や新しい暗号 library は初期 API の前提にしない。

RAM cursor state は tenant、trusted actor/session binding、正規化 request digest、可視性 stamp、許可された Source の generation/snapshot と continuation、fusion last key、retention ceiling、expiry に限る。raw query、body、snippet、evidence、credential、自由文字列 trace は保持しない。client が `cursor` とともに他の request field を再送するときは digest 一致を要求する。別 actor/tenant/session、subjects/access revision、可視集合、Source policy/retention、generation、provider snapshot、query、page size の変化、失効・eviction・process restart はすべて同じ `CURSOR_STALE` とする。current access は各 page の各 item に再適用する。外部に stale 理由や state 有無を教えない。trusted session binding を resolver が供給できなければ cursor を発行しない。

初期 RAM store の絶対寿命上限を 5 分、idle 上限を 1 分とし、actor/session 有効期限、Source/provider TTL、cache grant のうち最短の期限を採る。session close、logout、policy/ACL/visibility/retention 変更、generation retire、deadline/cancel、expiry で関連 state を即時無効化・消去する。process RAM 以外への serialization、spill、debug dump、分散 cache 複製はしない。P4 の SESSION_ONLY / CACHE_WITH_EXPIRY ceiling がより厳しければそれに従う。これらは安全側の設計上限であり production SLO や P4 freeze 済み値ではない。

| Source retention | 公開 continuation に保持できる状態 |
|---|---|
| `PERSISTENT_RESOURCE` / `PERSISTENT_DISCOVERY_METADATA` | 許可済み generation/key と最小 continuation metadata を上記 RAM session に置く。各 page で current access と source policy を再検査する。 |
| `CACHE_WITH_EXPIRY` | provider grant の TTL 内だけ RAM に置く。tenant、actor、access revision を跨いで再利用せず、失効・取消・retention 変更で消去する。 |
| `SESSION_ONLY` | 同じ trusted session の RAM に限定する。session が不明、終了、再作成、他 node へ移動した場合は継続不可。disk/共有 cache へ出さない。 |
| `NO_RETENTION` | 評価 call の返答 buffer 以外に candidate ID、query/result trace、provider cursor、snapshot、content/evidence、per-call provider-derived telemetry を残さない。公開 `nextCursor` の state にも載せない。call 終了・取消で buffer を破棄する。 |

Search の `nextCursor` は全ての完全性に必要な Source が安定した continuation を提供し、その retention が上表の session state を許すときだけ発行する。一つでも `NO_RETENTION` または非継続 Source が完全性に必要なら `nextCursor=null`、`partial=true`、型付き `PAGINATION_UNAVAILABLE` gap とする。現行 `PlanningOnly` の全リストを slice して continuation と称しない。`SourcePage` は可視性 stamp と安定 keyset が揃う場合だけ発行し、Source の非公開 metadata や provider payload を cursor state に保存しない。P4 の remote cursor は内部 port 専用であり外部 handle の中身にもならない。Discovery は一回の bounded evaluation で page cursor を持たない。`NO_RETENTION` の一時的な許可済み live disclosure は response body にだけ可能で、Audit、Projection、Graph、cache、test fixture、trace へ複製しない。

## 4. Deadline、partial、HTTP Problem

HTTP 層が operation deadline と cancellation token を作り、Search Application の routing、各 Source call、Probe/materialization、evidence resolution、最終 gate、DTO serialization へ渡す。各依存 timeout は残り operation 時間以下とし、期限・client cancel で未完了 task と body 読取を取消して RAM state を消去する。資格を満たす結果を期限内に private buffer で確定できない限り `200` を送らない。具体的な production 秒数は workload/PoC と既存 `spec/operations/error-handling-resilience-requirements-v0.md` §11 の階層に従い freeze する。別 ref の通常 30 秒を P5 の接続済み設定と見なさない。

| 完了条件 | Search / Discover の公開応答 | Discovery の証拠・完了性 |
|---|---|---|
| 可視範囲の計画した作業と最終 gate が期限内に完了 | 200、`partial=false`。0 件も正常 | Need に対する通常評価。`Sufficient` は実際に検証した required evidence が満たす場合のみ |
| optional Source/Probe が失敗・timeout、または上限に到達し、安全な独立結果を確定できる | 200、`partial=true`、見える Source に限る型付き availability/budget gap | `evaluationCompleteness` に bounded/interrupted reason を示す。required evidence が揃い completion policy が認める場合だけ `Sufficient` 可 |
| required Source/evidence が失敗・timeout/未評価でも安全な独立結果を確定できる | 200、`partial=true`、generic blocking gap。隠れた Source の名前・数は出さない | `Sufficient` 禁止。`Unresolved` 等へ落とし、未評価を absent としない |
| deadline 到達後に安全な最終 gate / DTO を確定できない、または必須基盤が動かない | timeout は 504 `TIMEOUT`、非 timeout 依存障害は 503 `DEPENDENCY_UNAVAILABLE`。途中 buffer を送らない | Problem のみ。Identity/tenant/visibility snapshot の障害も partial 200 にしない |

`GET /v1/resources/{id}` と `GET /v1/sources` は部分成功を返さない。前者の target 不在/不可視は一律 404、後者の可視集合を完全に確定できない場合は 503/504。response size 上限では黙って item/evidence を切らず、期限内に正しい `partial` + 型付き gap へ再構成できる Search/Discover のみ 200 とする。再構成不能なら generic 503。`partial=false` は hidden Source がないことや全世界を網羅したことを意味しない。gap/completeness の追加 wire enum は P1〜P4 と `search_core::GapReason` の最終照合後に `spec/` へ型付きで定義し、既存 `MissingFact` への読み替えや handler の任意文字列追加を禁止する。

Problem は [RFC 9457 §3.1.1](https://www.rfc-editor.org/rfc/rfc9457.html#section-3.1.1) の `type` を primary identifier とする。P5 では `type = urn:knowledge-platform:problem:<CODE>` と `code` を一対一に固定し、HTTP `status` と body `status` を一致させる。`code` は既存 client 向け extension、`retryable` は補助判断である。公開 error では `instance` を**省略**する。path に resource ID を再掲せず、個別発生の調査には無作為な `trace_id` extension を使う。`detail`、`errors[]` の pointer/message、trace ID から内部 Source、Claim selector、locator、SQL、query 本文を出さない。`errors[].pointer` は公開 request field だけに限定する。現行 `spec/operations/error-handling-resilience-requirements-v0.md` §7 の code/status 中心の client 規則は、P5 freeze 前に RFC の `type` 主識別子と整合する規範差分が必要である。

| status | `code` (`type` の末尾) | 条件 |
|---|---|---|
| 400 | `MALFORMED_REQUEST` | JSON 構文・query encoding が不正 |
| 401 | `AUTHENTICATION_REQUIRED` | identity 不在・無効・失効。adapter 診断を省く |
| 403 | `FORBIDDEN` | operation 全体を拒否。target の存在を示さない |
| 404 | `RESOURCE_NOT_FOUND` | GET の不存在・他 tenant・不可視・旧版・T10 終了・曖昧 mapping |
| 409 | `CURSOR_STALE` | cursor 不一致・失効・不明・他 session。理由は省く |
| 413 | `PAYLOAD_TOO_LARGE` | request body の承認上限超過 |
| 415 | `UNSUPPORTED_MEDIA_TYPE` | 承認外 request media type |
| 422 | `VALIDATION_FAILED` | 公開 field の形式・範囲違反。Source/Claim の実在性には使わない |
| 429 | `RATE_LIMITED` | actor/tenant の公開 rate policy。retry は許可された場合のみ |
| 431 | `REQUEST_HEADERS_TOO_LARGE` | request header の承認上限超過 |
| 503 | `IDENTITY_UNAVAILABLE` / `DEPENDENCY_UNAVAILABLE` | trusted resolver または必須基盤が利用不能。隠れた個別 Source 障害は原因にしない |
| 504 | `TIMEOUT` | operation deadline までに安全な応答を確定できない |

`spec/api/openapi.yaml` の四 operation に、閉じた request schema、成功/Problem 全 status、上記 `type/code/status` の対応、実際の trusted transport に合う security scheme と operation-level security requirement、`application/problem+json`、`Cache-Control: private, no-store`、`X-Content-Type-Options: nosniff` を記述する。認可前の parse/error、404、409、429、503/504 にも同じ no-store/nosniff policy を適用する。存在しない bearer/cookie/mTLS 機構を schema 上で仮定せず、選んだ resolver の実配線と一致させる。別 ref と Problem extension を共通化する際は、`trace_id` と `instance` の差を契約 diff として解消する。

OpenAPI 3.2.1 の埋込 Schema Object は既定で [OAS dialect](https://spec.openapis.org/oas/v3.2.1.html#schema-object)（Draft 2020-12 を拡張）を使い、`jsonSchemaDialect` を省略する場合もこの既定値として検証する。別 dialect を採る場合だけ `jsonSchemaDialect` または schema root の `$schema` に URI を明示する。独立 `.schema.json` は自身の `$schema` を明示し、Draft 2020-12 を使うなら OAS 専用 keyword を混ぜない。OpenAPI parser/linter と handler/schema parity で security、Problem、header、成功 DTO、未知 field 拒否を検証する。

## 5. Freeze 前の照合と受入反例

本修訂は review の P1/P2 を設計上塞ぐが、P1〜P4 の凍結後に Claim catalog、KnowledgeUnit/field-level coverage、Graph participant、remote identity/retention/continuation、stable fusion order を照合する作業は別に残る。P6 の outbox freshness も未接続を「最新」と呼ばない。P1 `BodyRequired` は承認済み body hit と同一 generation/権限が成立するまで成功扱いせず、title/Graph hit で代用しない。Graph path、native locator、raw score、provider credential は公開しない。Search は資格/実行認可を発行せず、live/current GET は history に fallback しない。これらの照合結果を先に `spec/` と OpenAPI に反映してから implementation plan と freeze を判定する。

最低限の反例は次を同じ実 HTTP + Application 経路で通す: 二 tenant と同形 ID、隠れた障害 Source、`sourceIds` の未知/不可視、重複 locator、未知/別 tenant/失効 Claim、router 前の visibility failure、候補・Graph participant・field/evidence read 中の取消、SourcePage の page 間取消と再付与、NO_RETENTION call 後の RAM/log/cache/trace 空確認、SESSION_ONLY の他 session/restart、cursor の他 actor/変更済み visibility/generation/query/retention、optional/required timeout と client cancel、response 上限、Problem/OpenAPI parity。`200`、schema parse、別 ref の HTTP stack、mock port だけを P5 公開資格とはしない。

### Architecture review との対応

| 指摘 | 修訂箇所 |
|---|---|
| P1 trusted tenant・Source existence | §1 の context、actor-visible Source snapshot、routing 前制限、tenant-scoped locator、起動拒否 |
| P1 cursor 秘匿・retention | §3 の RAM random handle、retention 表、失効/消去、NO_RETENTION の continuation 禁止 |
| P2 Claim visibility | §2 の actor-visible selector と opaque unresolved outcome |
| P2 最終 gate・SourcePage cursor | §2 の field-specific recheck、一体除外、visibility stamp |
| P2 deadline・partial・Problem・OpenAPI | §4 の deadline 伝播、truth table、RFC 9457 と OAS dialect、status/schema 対応 |
