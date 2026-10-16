# Search Personal Memory Source v0：設計

- 状態: PROPOSED / 設計判断待ち。実装・設計凍結・依存の採用・配備の承認ではない。
- Capability: Search Platform / Personal Memory Source（agent-platform の Personal Memory を索引する Source）。
- 調査基点: `main@ead6830e28bd18b93cd71e2087818ebfdab37a2b`（2026-10-15）。branch `design/personal-memory-source-v0`。本文の file:line は、断りが無ければこの commit のもの。未 merge の branch を引くときは branch 名と commit を書く（手元の ref で読んだ。fetch はしていない）。
- 相手側の基点: agent-platform `development@30d10df347fccb5a824edc183984080c73cd760b`。設計「連想型 Personal Memory — agent-platform 統合設計 v2（レビュー案、2026-10-15）」（以下「設計 v2」）の §2・§6・§7・§9・§10・§11・§13。
- 実装状態: NOT STARTED。製品 code・`spec/`・migration は変更していない。build・試験・計測・配備先への接続は行っていない。
- 関連: [実装計画](../plans/2026-10-15-search-personal-memory-source-v0.md)、[状況](../execution/search-personal-memory-source-v0-status.md)。
- 改訂: 2026-10-15 の独立レビュー（p0-review、KP の基点 `7ba63b15`）の指摘を反映した。対応の一覧は §8。

依頼者の決定（2026-10-15、拘束）のうち本書に関わるもの:

1. Search は knowledge-platform（以下 KP）側を先に作る。永続取り込み、常駐する Search server、日本語対応、本人ごとの private graph、project・skill の絞り込み、変更列を KP に作ってから接続する。agent-platform（以下 AP）は独自の FTS・Vector・Graph エンジンを作らない。「関連 Memory と Search で照合せずに active Memory を確定しない」規則は残す。v9.0.0 はこのために遅れてよい。
2. 範囲は設計 v2 の P0〜P6 全段。別の PC からの利用を含む。P6 も後へ回さない。最適化の作業は、計測した予算を外したときだけ始める。
3. 重い負荷（benchmark を含む）は RTX 5090 の機械（Tailscale でつながる依頼者の Linux x86_64 の機械。vLLM も動いている）で ssh 経由で行う。ssh は Tailscale の確認を依頼者が承認するまで使わない。この Mac の 1 分 load は 20 以下に保つ。
4. 設計 v2 §13 の R4 は変更する。`attach` は Search を通さず、AP の memory core から現在値を列挙する。

lead decision 2026-10-15（レビューの owner への問いへの決定、拘束）のうち本書に関わるもの:

5. KP-2 の基準は計測の前に固定する。重複・矛盾の組について、L＋V の Recall@20 の 95% 区間の下限 0.90（§3.4）。
6. v0 の PostgreSQL は 1 つの database とし、本人ごとに分けない。v0 では Memory に GPU を使わない。Memory は pin の意味を変えない（切替の間に前の世代で答える方式を Memory のために入れない）。
7. 本文の上限は 4 KiB のまま（S3 の `body` は 4,096 byte 以下）。agent から来た忘却は AP で `disable` に落ち、取り消せない忘却は owner の確認と猶予の後にだけ起きる。KP には `remove`（`disabled`）が先に、猶予の後に `remove`（`forgotten`）が届く。KP は索引で 2 つを同じに扱うので、受け手の作りは変わらない（§3.3）。忘却した内容を覚え直すのは owner の console から tombstone を明示に解いたときだけで、AP は新しい memory_id で作る。KP の抑止の行はそのまま残る。

owner decisions 2026-10-15（第 2 回、拘束。前の版で owner decision pending としていたものを置き換える）のうち本書に関わるもの:

8. 配置: AP の Memory API、KP の Search server、PostgreSQL はすべて owner の Apple silicon の Mac（以下「この Mac」）に置く。RTX 5090 の機械には配備しない。別の PC は Tailscale 越しに AP の Memory API を使い、この Mac が起きている間だけ使える。KP は loopback だけで待ち受け、別の PC からは直接使わない（§3.9）。
9. 計測: 重い負荷と規模の計測（1k・10k・100k 件、同時 1・4・16）は、ssh の承認の後に RTX 5090 の機械で、機材を明記した数値として行う。この Mac の本番の予算は、この Mac での軽い qualification（1 つの session の主な経路、idle と peak の RSS、起動、binary と disk の大きさ）から決め、その計測でも 1 分 load を 20 より上げない。2 つの数値は分けて書く（§3.10）。
10. KP の Draft PR: #85・#99 の merge を待つ。判断日は 2026-10-29。それまでは、#85・#99 に依らないもの（KP-1、KP-3、KP-6 の手順書など）を先に進める。10-29 までに #85・#99 が動かなければ owner が決め直す。#86 と #99 の branch は他の作業が更新しているので、決して触らない。
11. ssh: RTX 5090 の機械への ssh は、引き続き owner の Tailscale の承認を待つ。AP の 02 が KP-7R（RTX 5090 の重い計測）を release の条件に入れたので、この承認は v9.0.0 の前に要る。

## 0. 要約

1. Personal Memory は Remote Source として登録しない。KP に新しい Source の種類 `PersonalMemory` を足し、AP の変更列を KP から pull して、本人ごとの永続世代（字句・Vector・Graph）を作る。Remote は評価 1 回の RAM 世代で永続化しない契約で凍結されている（§2 G1）。そこを変えるより、Document 用の永続 pipeline を新しい種類にも開くほうが変更が小さく、Remote の保証も弱めない。
2. 1 つの MemorySpace を KP の 1 つの Source とする。字句の統計（BM25 の df）や Vector の近傍が他の本人のデータに左右されず、本人の Graph が自然に分かれるからである。登録は KP の運用者入力だけから作る。
3. 問い合わせは新しい route 3 本（`/v1/memory/search`・`/v1/memory/expand`・`/v1/memory/status`）で受ける。結果には照合した世代と、AP の変更列のどこまで反映したか（`appliedThrough`）を必ず付け、`search` では retriever の lane ごとの反映位置（`lanes`）も付ける。空の結果（200、`items: []`）と利用不可（503）を別の応答にする。件数の合計は返さない。
4. 進行中の Search の作業（未 merge の Draft PR #85・#86・#99、§2.1）に乗る。日本語の字句は #85 の文字 bigram analyzer（所有者が当面 Lindera より選んだもの）を Memory にも使い、Memory の合成コーパスで保存前照合の基準を満たさないときだけ Lindera を再検討する。世代を変更のたびに全件作り直す問題は #99 の「不変の部品と一覧」で解く（承認されたのは 2026-10-07 の設計で、PR は作業中の Draft）。Vector は main で選定済みの multilingual-e5-small を CPU で使う。v0 では Memory に GPU を使わない（lead decision）。モデルの選定は KP 全体の判断に従う。
5. 常駐の `search_server` binary と、AP が発行する短命の署名 assertion（EdDSA）を検める資格情報検証器を作り、この Mac へ PostgreSQL とともに launchd で配備する。本番の予算はこの Mac での軽い計測で固定し、規模の計測は RTX 5090 の機械で別に行う（owner decision 第 2 回）。
6. AP との取り込みの契約は `personal-memory-source/2` として AP 側の担当と合意した（§4。正本は AP の文書。担当の間の合意で、所有者の承認はまだ）。
7. 忘却は 2 段で返す。`suppressed` は即時で、その時点から結果に出ない。`purged` は、忘却した記憶を含む字句の segment を書き直し（#99 の部品方式は削除の印で隠すだけで、本文を持つ file を後の世代と共有し続けるため）、古い世代と行を消し終えてから送る。期限は `suppressed` から 24 時間で、過ぎたら `overdue` を示し、終えた時点で遅れても送る（§3.3）。

## 1. AP が Search に求めること、Search がしないこと

### 1.1 求めること

| ID | 要求 | 設計 v2 の根拠 | 本書の答え |
|---|---|---|---|
| R-1 | 保存前の照合で、本人の Source の中から関連する Memory の候補を、照合した世代と反映位置に束縛して返す | §6 手順 2・4 | §3.7 `/v1/memory/search` |
| R-2 | 連想探索。1 回の呼び出しは件数・関係数・時間に上限があり、続きは次の呼び出しでたどれる。深さの上限は呼び出しをまたいで固定しない | §1.1-6、§7.2 | §3.6、§3.7 `/v1/memory/expand` |
| R-3 | 本人の隔離。他の本人の ID・本文・件数・Graph・trace・索引を、どの経路からも返さない | §9 末段、B01 | §3.2、§3.8 |
| R-4 | 適用条件での絞り込み（project_id、skill_ids） | §3 `applies_to`、§7.3 手順 3 | §3.7 の filter |
| R-5 | 変更の取り込み、削除・忘却の索引からの除去、除去の確認の返送 | §2 図の OUT、§4.1 忘却、A08 | §3.3、§4 |
| R-6 | `complete([])` と `unavailable` を混同しない。続きは版（世代）に束縛する。件数・error・trace で他者の存在を漏らさない | §7.1、§9 末段、B02 の Search 側 | §3.7、§3.8 |
| R-7 | 起動できる常駐の Search server と、本物の資格情報検証。別の PC から使える配備（別の PC は AP の Memory API を通す。この Mac が起きている間だけ） | §9、§11 P2 | §3.9 |
| R-8 | 資源（binary、常駐 RSS、起動時間、DB と索引の容量、遅延）を測ってから予算を固定する | §10.3 | §3.10 |

### 1.2 Search がしないこと

- **Memory 本文の正本を持たない。** 持つのは索引のための派生の写しである。#99 の方式では、Unit 部品の行（`search_unit_segment.payload`）と字句の Unit 索引の stored の欄（#99 の branch `b7b8cd89` の `crates/search-tantivy/src/body.rs:271-279`）が本文そのものを持ち、字句の語、Vector の値、Graph の行も本文や記憶から作る。検索結果に本文を返す経路は無い。本文は AP が core から版を指定して読む。写しを消す範囲と時期は §3.3 の忘却の物理的な消去が決める。
- **現在値・失効・削除・矛盾・優先順位を決めない。** 取り込むのは、AP が変更列で「現在の active 版」として渡した版だけである。`supersedes` などの関係を Search が解釈して結果から隠したり順位を変えたりしない。
- **`attach` の完全列挙をしない**（R4 の変更）。B02 の「反映待ちの新規 item も含めて完全に返す」は AP の core が満たす。
- **照合 receipt を発行せず、保存してよいかを判断しない。** Search は候補と、それを出した世代・反映位置を返すだけである。
- **`depends_on` の健全性を検査しない。** 厳密な依存の版鎖の検査は AP の core が行う（設計 v2 §5、§7.2 末尾）。Search の Graph は連想の候補を出すためだけに使う。
- **関係を推定して Graph に書かない。LLM を呼ばない。** Graph の辺は AP が送った `MemoryRelation` だけである。Vector の近さは検索結果の順位にだけ使い、辺として保存しない。
- **本人をまたぐ辺・統計・cache を作らない。**

## 2. 現状と不足（`main@ead6830e` で再確認）

監査 C（2026-10-15）の指摘を、同じ commit の file:line で確かめ直した。新しく見つけた不足には「新」と書く。§2.1 の進行中の作業で解かれる見込みのものは、対応の欄に PR 番号を書く。

| # | 現状 | 不足 | 対応 |
|---|---|---|---|
| G1 | Remote の世代は「Source と評価 1 回ごとに 1 つの不変な RAM 世代」で、永続 manifest に変えない（`crates/search-application/src/remote_generation.rs:1-11`）。cache は `CACHE_WITH_EXPIRY` だけで、永続への変換を拒む（`remote_cache.rs:1-9`）。提供側のデータは selector にも concept relation にもならない（`remote_read_view.rs:1-9`）。P4 の凍結にも "one immutable generation per Source/evaluation" とある（`docs/superpowers/programs/search-platform-completion/p4-remote-freeze.md:7`） | Search が Memory を索引する仕組みが無い | §3.1 新しい種類 |
| G2 | 新: Remote の本文取得 `content/{id}` の wire は `{tenant, source, id, version, digest, provenance[]}` で、本文の欄が無い（`crates/search-source-http/src/protocol.rs:156-167`） | 字句・Vector の材料を受け取れない | §3.3、§4 |
| G3 | 新: Remote の本番 transport は HTTPS だけで（`crates/search-source-http/src/transport.rs:211-214`）、名前解決の結果に公開でない address が 1 つでもあれば拒む。`100.64.0.0/10`（Tailscale の address 帯）も拒む（同 `:408-419`） | Tailnet や同じ機械の AP に届かない | §3.3 の transport |
| G4 | 永続世代の登録と Graph の差分登録は Document だけを受け付ける（`crates/search-runtime/src/generation_registration.rs:167`、`graph_coordination.rs:47`）。Vector の保守も Document Source が対象（`vector_runtime.rs:1-8`） | 新しい種類の永続世代を作れない | §3.1、§3.3 |
| G5 | `SourceKind { Document, Remote }`（`crates/search-application/src/source_registration.rs:35-38`）、`ResourceKind` に Memory が無い（`crates/search-core/src/resource.rs:11-22`）、公開 DTO の `ResourceTypeDto` も同じ（`crates/search-api-http/src/dto.rs:38-49`） | Memory を表す型が無い | §3.1 |
| G6 | 製品の tantivy は `default` tokenizer（`crates/search-tantivy/src/schema.rs:60`、`body.rs:61`、query 側 `query.rs:227-231`）。Lindera 6.2.0（IPADIC 埋め込み）は PoC だけ（`experiments/search-discovery-poc/Cargo.toml:17`、`src/lexical.rs:96-98`）で、選定は POC REQUIRED のまま（`spec/selection/library-tool-selection-v0.md:572`、同 §13.3 の表 `:1276`）。MIRACL 日本語 dev の評価 100 問で nDCG@10 は、製品の字句（default tokenizer と字句の phrase 一致の組み合わせ）が 0.0348、文字 bigram の BM25 が 0.2486（`experiments/search-vector-model-poc/report.md:60`）。二つの差には tokenizer の違いと問い合わせ方式の違いが両方入っている | 日本語の字句検索がほぼ効かない | #85、§3.4 |
| G7 | Vector は Discover だけで使う（`docs/superpowers/execution/search-platform-production-program-status.md` 末尾「残り」）。新: `PgVectorIndex` は読み込んだ stage を RAM に置き続け、外すのは未公開 stage の破棄と scope の purge のときだけ（`crates/search-runtime/src/vector_store.rs:123-177, 179-183, 334-336, 350-362`） | Memory のように世代が頻繁に変わると、古い stage が RAM に溜まる | #99、§3.5 |
| G8 | 型付き n 項関係はある（`crates/search-core/src/relation.rs:47-58`）。走査の予算は hop・関係・分岐・seed・経路の数で（`graph.rs:63-69`、検査 `:85-110`）、時間と node 数の欄は無く、停止条件は受け付けない（`:107-108`）。結果は hit と経路だけで続き（frontier）を返さない（`crates/search-application/src/ports.rs:445-454`）。公開 Discover の `maxHops` は 1〜3（`crates/search-application/src/discover_route.rs:76`、`dto.rs:269-272`） | 本人の Memory の Graph が無い。続きを返す API が無い | §3.6、§3.7 |
| G9 | `/v1/search` の入力は query・resourceTypes・sourceIds・coverage・pageSize・cursor だけ（`dto.rs:95-108`） | 属性での絞り込みが無い | §3.7 |
| G10 | 新: 公開 `SearchItemDto` は resourceId・sourceId・resourceType・resourceVersionId・title・rank・matchedFields・provenance で、digest・世代・反映位置が無い（`dto.rs:364-378`） | 照合 receipt を世代と反映位置に束縛できない | §3.7 |
| G11 | 公開 cursor は RAM だけで、絶対 5 分・無操作 1 分で失効し、1 回使うと無効、再起動でも失効する（`crates/search-application/src/api_cursor.rs:1-8`） | LLM が考えている間に続きが失効する | §3.7（cursor を使わない） |
| G12 | 「ある token 以降に変わった item」を取る API が無い（Remote は `catalog?cursor=` の頁送りだけ、`transport.rs:124-160`）。Document は snapshot から世代を作り直す（`crates/search-source-document/src/outbox.rs:1-2`）。item 単位の tombstone 型は無い | 変更列・削除・確認の返送が無い | §3.3、§4 |
| G13 | `build_search_api_runtime`（`crates/search-runtime/src/api.rs:211`）を呼ぶのは同じ crate の `tests/` だけ。binary は `search_outbox_worker` だけ（`src/bin/`）。資格情報の検証は port（`crates/search-api-http/src/auth.rs:33-35`）があるだけで、実装は試験用の 2 つだけ（`crates/search-api-http/tests/support/backend.rs:334`、`crates/search-runtime/tests/support/api.rs:150`）。KP の既存 server は PoC の固定 identity（`crates/document-server/src/identity.rs:1-2`）。Dockerfile に Search の image は無い（`Dockerfile:21-34`） | 起動できる Search server と本物の検証器が無い | §3.9 |
| G14 | 新: durable read は、Source の pointer が動くたびに世代のすべての保存物（payload、封印した字句 directory、Graph 行、digest）を再検証し、projection を RAM の store へ載せ直す（`crates/search-runtime/src/durable_read.rs:1-16`）。字句 directory は全 byte の digest を取る（`lexical_artifact.rs:1-10`） | 小さな変更ごとに件数に比例した再検証と RAM の載せ替えが起きる | #99、§3.3 |
| G15 | Discover の trace に `count` がある（`dto.rs:492-502`）。数は、見えている Source ごとの適格件数と evidence の件数である（`crates/search-application/src/public_projection.rs:300-313, 322`） | Memory の route では件数をまったく返さない設計にする | §3.8 |

既にあって使えるもの: 永続の P7 bundle（字句 directory の fsync と封印、`lexical_artifact.rs:1-10`）、差分（INCREMENTAL）世代（`document_runtime.rs:14-19`）、guard 付き GC（`gc.rs:1-15`）、評価ごとの pin（`pin.rs:1-14`）、固定の Problem（`crates/search-api-http/src/problem.rs:1-7`）、本番の登録台帳と運用者入力の公開（`crates/search-runtime/src/host_inventory.rs:1-16`）、固定モデルの Vector（`crates/search-vector-adapter/src/lib.rs:1-10`）。

### 2.1 進行中の Search の作業（未 merge）

次の Draft PR は main にまだ入っていない。本書はこれらを置き換えず、その上に乗る。PR の状態は 2026-10-15 に `gh pr view <n> -R AIrisu-072/knowledge-platform --json state,isDraft,mergeable,baseRefName,updatedAt,title` で読んだ（読み取りだけ）。#86 と #99 は同じ日にも他の作業が更新しているので、表の先端の commit は読んだ時点のものである。本書の作業はこれらの branch に触らない。

| PR | branch（先端）と状態 | 内容 | 本書との関係 |
|---|---|---|---|
| #85 | `fix/search-validation-findings-20261006`（`18080d44`）。OPEN・Draft・mergeable `CONFLICTING`・base `main`・最終更新 2026-10-07 | 検証で見つかった不具合の修正。日本語の文字 bigram analyzer `tantivy-0.26.2-cjk-bigram-v1`（辞書なし、tantivy の組み込みだけ）と世代ごとの analyzer の選択、字句の「連続一致の後に語の 6 割一致」の段、Unit 一覧の分割保存（migration 0010）など | 選定記録の追記（branch 上の `spec/selection/library-tool-selection-v0.md:1297-1305`）に「所有者は当面 Lindera より文字 bigram を選んだ」とある（同 `:1299`）。Memory もこの analyzer を使う（§3.4） |
| #86 | `feat/search-validation-corpus-20261006`（`5a87a9af`）。OPEN・Draft・mergeable `CONFLICTING`・base `main`・最終更新 2026-10-15。題は「作業中: …」 | 大規模文書・検索精度の検証基盤。1,000・1 万文書での計測、埋め込みモデルの比較、所有者の GPU マシン上の vLLM による Agent 実験 | 計測の方法と数値（branch 上の `docs/superpowers/programs/search-validation-corpus/status.md`）を §3.5・§3.10 で参照する。KP-7R（RTX 5090 の重い計測）は同じ GPU マシンを使うので、計測の時間を調整する |
| #99 | `feat/search-generation-segments-20261007`（`b7b8cd89`）。OPEN・Draft・mergeable `MERGEABLE`・base は main でなく #85 の branch・最終更新 2026-10-15。題は「作業中: 検索世代を不変の部品と一覧で持つ（段1：Unit一覧）」のまま | 世代を「不変の部品と一覧」で持つ。所有者が承認したのは設計（2026-10-07、branch 上の `docs/superpowers/specs/2026-10-07-search-generation-segments-design.md:3`）で、PR は作業中の Draft である。Unit・字句・Vector・Graph の部品化、読込みの差分化、Vector の stage は最新だけを RAM に残す（branch 上の `crates/search-runtime/src/vector_store.rs:793-804`）、worker の jemalloc。migration 0010〜0013 を使う | G7・G14 を解く。Memory の世代もこの方式で作る（§3.3）。KP-4 は #99 の merge の後に始める（計画 §1）。2026-10-29 を判断日として待つ（owner decision 第 2 回）。この branch と #86 の branch には触らない |

#99 の計測（1 万文書・169 万 Unit、#86 の状況記録の `status.md:217`）では、1 件の更新の反映が 11〜16 秒、worker の最大 RSS が 2.7〜2.9 GB、新しい世代への切替で検索が 2〜3 秒止まる（p99、同 `:190-193`）。Memory の Source は本人ごとで、1 件はおおむね 1 Unit なので、1 万件でも Unit 数はこの計測の約 1/170 である（見込み、未計測）。

#99・#86 に残る未決と未達（Memory の計画の重みに関わるもの）:

- 所有者の判断待ち: 切替の間に前の世代で答える方式（pin の意味を変える。#86 の `status.md:193`）。GPU で埋め込む方法（#99 の計画 `docs/superpowers/plans/2026-10-07-search-generation-segments.md:168`）。割当て器の選び直し（同 `:94`）。Memory の側は lead decision 2026-10-15 で、pin の意味を変えず GPU を使わないと決めた（冒頭の 6）。
- 合格条件の未達: 1 万文書の worker のメモリは 2 GB 以内が条件（#99 の計画 `:274`）で、`b7b8cd89` の時点で最大 RSS 2.70〜2.88 GB（#86 の `status.md:217`）。Vector の差分構築中の OOM（2026-10-08、同 `:230`）は、manifest v2 の後の計測で Vector の差分の公開まで通った（同 `:231`）。
- 字句の統合の条件は未決（#99 の設計 `:76`、計画 `:124`）。計画の T8 は統合を止めると書く（計画 `:70`）が、先端 `b7b8cd89` には、10,000 文書未満の segment が 8 個を超えたらそれらを統合する処理がある（`crates/search-tantivy/src/body.rs:458-480`）。10,000 文書以上の segment は統合しない。差分の世代は前の世代の segment file を hard link し（同 `:402`）、消した Unit は削除の印で隠す（同 `:418-424`）。Unit 索引は本文を stored で持つ（同 `:271-279`）。したがって忘却した記憶の本文は、その segment を統合し直すまで後の世代の file に残る。Memory ではこれを §3.3 の強制の書き直しで扱う。

## 3. 目標設計

### 3.1 Source の種類: `PersonalMemory` を新しく足す

**選ぶもの**: `SourceKind::PersonalMemory` と `RegistrationNamespace::PersonalMemory`、`ResourceKind::Memory`（`ResourceBody::Memory`）を足す。取り込みは新しい adapter crate `search-source-memory` が行い、世代は Document と同じ永続 pipeline（#99 の部品方式）で作る。

**Remote を拡張しない理由**（すべて現在の code と凍結文書で確かめた）:

1. Remote の凍結された意味（評価ごとの RAM 世代、永続へ変えない、提供側データを relation にしない。G1）を 3 つとも変えることになる。Remote の既存の試験と保証を、種類の中の分岐で守り続ける必要が出る。
2. Remote の wire は本文を運ばない（G2）。取り込みには本文・属性・関係が要り、wire は別物になる。
3. Remote の transport は公開 address 専用で（G3）、これは SSRF に対する防御として正しい。Memory の提供者は運用者が固定した私的な相手（同じ機械か Tailnet）なので、規則が逆になる。
4. Remote には所有者の概念が無く、可視性は item ごとの `/authorize` に頼る（`crates/search-source-http/src/adapter.rs:376-389`）。Memory は Source 全体が 1 人のものなので、Source 単位の可視性で足りる。
5. 永続 pipeline は既にあり、Document だけに閉じている門が 2 か所と Vector の保守だけである（G4）。そこを「永続取り込みの種類」（Document と PersonalMemory）に開くほうが、Remote を作り変えるより小さい。

**却下した他の案**:

- **Memory を KP の Document として取り込む**: Document Platform が 2 つ目の本文正本になる。設計 v2 §2 と §1.2 の「Search は本文の正本を持たない」に反する。
- **全本人を 1 つの Source に入れて中で分ける**: BM25 の df・idf、Vector の近傍、処理時間が他の本人のデータに左右され、件数や順位から他者の内容を推測できる経路が残る。避けるには分割ごとに索引を分けることになり、結局 Source ごとの世代を作り直すことになる。

**影響の見積り**（src の、`tests/` を除いた箇所を数えた）:

- `SourceKind::` を含む行は search 系 crate で 20、そのうち種類で分ける箇所は 13（`match` 9、比較 4）。`RegistrationNamespace::` は 5 file で 42 か所（うち 14 は `#[cfg(test)]` の中、`crates/search-runtime/src/source_registration.rs:527` 以降）、`SourceRegistration::{Document, Remote}` は 6 file で 10 か所。永続化 DTO と digest の種類 tag（`source_registration.rs:388, 632-633, 728-729`、`generation_registration.rs:336`）にも値を足す。
- `match` は種類を足すと compile で漏れが分かるが、**次の箇所は compile が通ってしまう**ので、KP-3 で 1 か所ずつ意図を決めて試験で固定する。比較: `generation_registration.rs:167`、`graph_coordination.rs:47`、`discover_route.rs:249, 269`、`source_registration.rs:1208-1209`。namespace を 2 つに固定した引数: `SourceRegistrationCatalog::try_new`（`source_registration.rs:1203-1207`）。とくに `discover_route.rs:249, 269` は、何もしないと Memory Source が黙って Discover の対象から外れる。本書はそれを意図とし（§3.2 の面の分離）、試験で固定する。
- DB の制約も変える。`0002_search_source_ownership_v1.sql:130` の `CHECK (source_kind IN ('DOCUMENT','REMOTE'))`、`0007_host_registration_inventory_v1.sql:19-21, 29-33` の Document・Remote に固定した列と `schema_version='v1'`。新しい migration は追加だけでは済まない（計画 KP-3 の rollback）。

### 3.2 登録と所有者

- **1 つの MemorySpace を 1 つの Source とする。** KP の承認設計は「Source を初期の primary shard の境界とする」「Resource の存在自体が機密なら Source・Access の境界に沿った物理分離を許す」としている（`docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md:237, 239`）。本書は Source を分けることでこの境界を満たす。PostgreSQL の database まで本人ごとに分けることは v0 では行わない（lead decision 2026-10-15。§3.9）。
- **登録の項目**（`ServerPersonalMemoryRegistrationConfig`、仮称）: tenant、server が採番する `source_id`、所有者の principal（`owner_principal`）、AP 側の space の識別子（`provider_space_id`、不透明）、提供者の endpoint（§3.3 の transport 規則）、取り込みの上限（頁の件数、応答の byte、呼び出しの期限、batch の件数）、`registration_revision`、`visibility_revision`。保持は永続（`PersistentResource` 相当）、列挙は完全（`Complete` 相当）に固定し、運用者は変えられない。
- **登録は KP の運用者入力だけから作る。** Remote と同じく HTTP や提供者の応答から deserialize できない型にする（`crates/search-application/src/remote_registration.rs:174-175` と同じ規則）。本番の登録は、既存の `PgHostInventoryPublisher`（`host_inventory.rs:3-10`）の運用者入力に第 3 の namespace を足して公開する。本人が増えたら運用者が inventory を改訂して公開する。個人利用では本人が host の運用者であり、本人の space を作ったときに host の上で `search_server inventory publish` で登録を足す。手順は AP 側の配備の手順書（AP-7・O-3）と KP-6 の手順書に置く。toolbox は利用者の端末に配る道具で、KP の登録には触らない（計画 §3）。
- **可視性**: `VerifiedSourceVisibilityPort`（`crates/search-application/src/scoped.rs:536-542`）の Memory 用実装は、actor の (tenant, principal) が登録の (tenant, owner_principal) と一致し、かつ資格情報の scope が `memory.search` のときだけ grant を返す。運用者や他の本人には、API からはその Source の存在も見せない。DB を直接読める運用者を否定はしない（設計 v2 §1.2 と同じ立場）。
- **scope による面の分離**: `memory.search` の資格情報は `/v1/memory/*` だけで使え、既存の 4 route では 403（既存の `FORBIDDEN`）になる。Document 用の資格情報は `/v1/memory/*` で 403 になる。Memory Source は既存の 4 route（`/v1/sources` を含む）の対象に入れない。

### 3.3 取り込み（AP の変更列を KP が pull する）

**流れ**（Source ごとに 1 本の取り込み task が動く）:

1. KP が `changes?after=<applied_token>` を呼び、変更の記録を受け取る。記録には本文を入れない。
2. `remove`（AP の `reason` は `disabled` か `forgotten`）は、受け取った直後に抑止の表（Source、memory_id、seq、reason の区分。本文も digest も持たない）へ入れ、取り込み状態と同じ transaction で確定する。確定した時点から、その Source のどの世代（現在・保持中・pin 付き・構築中）の `/v1/memory/*` の結果にも、その memory_id は出ない。確定後に AP へ `suppressed` を返す。`suppressed` の時点では、本文・語・Vector・Graph の行はまだ KP の保存物に残っている。KP は索引の上では `disabled` と `forgotten` を同じに扱い、どちらも次の世代から外し、どちらも下の「忘却の物理的な消去」で消す。`disabled` の抑止は、より新しい seq の `upsert` が来たら解除する。`forgotten` の id に `upsert` が来たら整合性の誤り（Unknown）とする（AP は忘却した id を再利用しない。覚え直すときも新しい id で作る）。
3. `upsert` は `items:batchGet` で現在の item（本文、属性、関係）を取り、次を検める。space の一致、本文の digest（SHA-256）の一致、本文が UTF-8 で上限以下、版の seq が既知の値以上、抑止された seq より新しいこと。合わないものは「無い」とせず Unknown として扱い、再試行する（既存 adapter と同じ方針、`crates/search-source-http/src/adapter.rs:7-9`）。
4. 現在の世代を基準に新しい世代を作る。#99 の部品方式で、変わった Memory の部品（Unit、字句 segment、Vector の参照、Graph のレコード）だけを作り、変わらない部品は前の世代と共有する。ただし、抑止した記憶の文書を含む字句の segment は共有しない（下の「忘却の物理的な消去」の (b)）。Vector の部品は同じ構築の中で作り、字句と Vector の両方がそろってから公開する（埋め込みのモデルが使えないときだけ Vector なしで公開し、その世代の Vector の lane は使えない。§3.5）。世代の manifest に `applied_through`（反映した変更列の位置）を入れ、digest に含める。
5. pointer の CAS で公開する。公開後に AP へ `applied_through` と世代 ID を返す。
6. 抑止した記憶を、下の「忘却の物理的な消去」の (a)〜(c) がすべて成り立つまで消したら、AP へ `purged` を返す（`disabled` と `forgotten` の両方について送る。AP は `forgotten` の receipt にだけ使う）。`purged` の意味（含むものと含まないもの、期限）は protocol の意味として §4 にも書く。
7. 抑止の行（memory_id と seq だけ、本文なし）は消さずに残し、古い batchGet の応答や古い backup からの復活を防ぐ（設計 v2 §4.1 の最小 tombstone と同じ考え）。

**忘却の物理的な消去（purge）**

#99 の部品方式は、消した文書を削除の印で隠し、本文を持つ segment file を後の世代と共有し続ける（§2.1）。統合の条件は #99 で未決で、10,000 文書以上の segment は統合されない。そのため「どの世代からも参照されない部品を消した」時点が来ないか、Unit 部品の GC だけで `purged` を送ると、語と本文が file に残ったまま `purged` を名乗ることになる。Memory の Source では次のようにする。

- (a) 参照を切る: その Source のどの世代（現在・保持中・pin 付き・構築中）の一覧も、その記憶を含む部品（Unit 部品、字句の segment、Vector の部品と値、Graph 部品）を参照しない。Memory の Source では、新しい世代を公開した直後に、抑止した記憶を含む古い世代を時限で回収する。今の Document の回収は次の公開のときだけ行い、直前 1 世代を残す（#99 の branch の `crates/search-runtime/src/gc.rs:116-118`、`document_runtime.rs:128`）。それでは更新が止まると古い世代が残り続けるので、Memory では次の公開を待たない。Memory の route は pin を作らない（§3.7。Memory の Source は pin を作る既存の route に出ない、§3.2）。
- (b) 字句の segment を書き直す: その記憶の文書（Unit 索引と Resource 索引）を持っていた segment は、それを除いて書き直してから新しい世代に入れる。Tantivy の統合は削除の印の文書を写さず（vendored の `third_party/search/tantivy-0.26.2/src/indexer/merger.rs:258, 388-392, 439-440, 495-515`）、1 つの segment だけの統合で印の文書を除ける（同 `:984` の試験）。全文書が消えた segment は統合で無くなる（同 `segment_updater.rs:92-97`）。古い世代の directory を消し、その segment の file を KP のどの directory も参照せず（link 数 0）、KP の process も開いていない（search server が読み込んだ古い世代の reader を閉じた）。
- (c) 行を消す: その記憶の本文・語・Vector・digest・関係を持つ PostgreSQL の行（Unit 部品、Vector の部品と値、Graph 部品、取り込み状態の digest）を DELETE して commit した。Vector の値の鍵 `EmbeddingCacheKey` は `source_id` を含み（`crates/search-core/src/knowledge_unit.rs:396-405`）、本人をまたいで共有されないので、本人の Source の中の参照だけを見ればよい。Graph 部品は一覧から外れて 10 分たってから消える（#99 の branch の `gc.rs:159`）ので、その分を期限に入れる。
- 含まないもの: PostgreSQL の VACUUM 前の頁（TOAST を含む）と WAL、KP の backup、file system の空き block（unlink は 0 で上書きしない）、process の RAM の解放済みの領域。抑止の行（memory_id・seq・reason の区分）は消さない（手順 7）。
- 期限: protocol の上限 T は `suppressed` から 24 時間で、AP と KP が同じ T で期限超過を判断する（AP は `suppressed` を受けた時点から数える。§4）。KP は保守の窓を待たず、その記憶を外す次の世代の構築で segment を書き直す。他に変更が来なくても、KP の設定の目標時間（`purge_target`、既定 30 分。見込みの値で、KP-7 で `suppressed` から `purged` までの時間を測って予算に固定し直す。T より短くする）の中で、世代の構築と古い世代の回収を走らせる。`purged` は (a)〜(c) を終えるまで送らない。T を過ぎても終えていなければ `/v1/memory/status` に `purge: "overdue"` を出し（件数も id も出さない）、終えた時点で遅れても `purged` を送る。AP は T を過ぎた忘却の receipt を `purge_overdue` にして owner に見せ、`purged` が来たら `purged` に進める（AP の 03 §3.6・§5.4.5 の 5）。
- 実装の前: KP-4 が (a)〜(c) を実装して試験（計画 KP-4 の `purge_*`）が通るまで、KP は `purged` を送らない。AP の忘却の receipt は `suppressed` で止まり、それ以上を名乗らない。

**全件の再同期**: 初回の登録、AP が token の失効（410）を返したとき、digest の不一致が続いたとき、運用者の明示の指示（`search_server memory resync <source>`）のとき、KP は `items` で現在の item の完全な一覧と snapshot token を取り、全件を取り直して FULL の世代を作る。一覧に無い item は落とす。その後は snapshot token から差分に戻る。

**backup からの復元**: 古い DB backup から KP を戻すと、その後に抑止した item が世代に戻りうる。KP は起動時に、AP が記録した最後の確認（`acks/last`、§4）と自分の `applied_through` を比べ、AP の記録のほうが新しければ、全件の再同期が終わるまでその Source を利用不可にする（A08 の「旧バックアップ復旧で勝手に復活しない」）。再同期で一覧に無い item を落とした後、上の「忘却の物理的な消去」で戻った写しも消す。KP の状態は AP から作り直せるので、Memory の Source の行と字句の file は KP の backup の対象から外すことを推奨する（外せば、忘却した記憶の写しが KP の backup に残らない。§3.9）。

**transport**（Remote の transport は使わない、G3）: 運用者が固定した 1 つの endpoint にだけ接続する。loopback（`127.0.0.1`・`::1`）なら HTTP を許し、それ以外は HTTPS で、運用者が置いた CA 証明書か公開 CA で検証する。名前解決の結果を使わず、固定した address に接続する。redirect と proxy は使わない。要求・応答の byte と期限に上限を置く。Remote と同じく、失敗は「無い」ではなく Unknown として扱う。

**頻度**: Source ごとに、変更を最大 W 秒（既定 2 秒、設定可）ためてから 1 つの世代を作る。#99 の部品方式では、1 世代の仕事は変わった item の数と一覧の大きさに比例する。#99 が merge される前の main の方式では、世代ごとに件数に比例した作り直しと再検証が起きる（G14）。Memory の取り込みは #99 の上で作る（計画 KP-4 の依存）。

**失敗時**: AP に届かない間、KP は最後の世代で答え続け、`/v1/memory/status` が `unreachable` を返す。すべての結果に世代の `appliedThrough` が付き、`search` では lane ごとの反映位置（`lanes`、§3.7）も付くので、AP は遅れを自分で判断できる（AP の未反映差分の補完、設計 v2 §6 手順 2）。

### 3.4 字句検索（日本語）

**使うもの**: #85 の文字 bigram analyzer `tantivy-0.26.2-cjk-bigram-v1`。CJK の連続は重なった 2 文字に、それ以外の英数字の連続は小文字にした 1 語にする。辞書を使わず、新しい依存も無い。世代の manifest の `analyzer_version` で、作るときと読むときの analyzer を選ぶ（#85 の選定記録の追記、branch 上の `spec/selection/library-tool-selection-v0.md:1297-1305`）。所有者は当面これを Lindera より選んでいる（同 `:1299`）。

**Memory 用に確かめること**（計画 KP-2。基準は lead decision 2026-10-15 で計測の前に固定した）:

- データ: 合成の日本語 Memory コーパス（新規、repository に入れる。1,000 件以上、問い 60 以上。言い換え、複合名詞、カタカナの揺れ、英数字の混在、短い問い、重複・矛盾の組を含む）と、既存の MIRACL 日本語 dev の公開レーン（40 問を校正、100 問を評価、`experiments/search-vector-model-poc/report.md:57-60`）。実データは使わない。
- 比べるもの: 字句の問い合わせ方式 2 つ（#85 の「連続一致の後に語の 6 割一致」、bigram の BM25 上位 k 件）と、Vector と組み合わせた結果（L＋V）。
- 指標: Recall@20（保存前照合では取りこぼしが重いので主指標）、nDCG@10、該当なしの誤検出、索引の byte、問い合わせの p95。
- 基準（lead decision 2026-10-15 で固定。計測の結果を見て変えない）: 重複・矛盾の組について、L＋V の Recall@20 の 95% 区間（問い単位の paired bootstrap 10,000 回）の下限が 0.90 以上なら bigram のまま進む。下回れば Lindera（下記）を候補に計測を続ける。KP-2 は最初の commit でこの基準を `gate.md` に書き、計測の commit をその後に置く（計画 KP-2）。

**Lindera を再検討する場合の扱い**（KP-2 の基準を満たさないときだけ）:

- binary に埋め込まない。Lindera 6.2.0 の `embed-ipadic` は build のときに `https://Lindera.dev/mecab-ipadic-2.7.0-20250920.tar.gz` を取りに行き MD5 で照合する（`lindera-ipadic-6.2.0/build.rs`、crates.io 配布物で確認）。埋め込むと binary が約 47.5 MB 増える（算出値、実測ではない）。代わりに build 済みの辞書 directory を運用者が置き、KP が SHA-256 で照合してから file として読む（Lindera 6.x は file からの読み込みと mmap に対応）。Vector のモデルと同じ扱いにする（`crates/search-vector-adapter/src/lib.rs:3-6`）。
- crate の表記は MIT だが、辞書データは IPADIC（NAIST と ICOT）の条件に従い、配布物に原文の COPYING を全文付ける。`lindera-ipadic` 6.2.0 の NOTICE.txt は NAIST の著作権表示の行を欠くので、tarball の COPYING を使う。
- `lindera-tantivy` の最新は lindera 5 系にしか対応しないので、KP の PoC と同じく `Segmenter` を直接呼ぶ自前の adapter を持つ。
- 既にある KP の計測（13 問の PoC、macOS arm64、debug build、他の build と競合しうる）では、default に比べて Lindera IPADIC は Recall@10 0.846→0.923、索引 5,035→5,939 byte、構築 80.4→93.3 ms、問い合わせ p50 0.085→0.198 ms だった（`docs/superpowers/execution/search-poc-segmenter-measurement-20261014.md` の表）。性能の合否には使わないと同じ文書が書いている。辞書そのものの build は release で 1.22〜2.73 秒という公開の値がある（計画 §5）。
- 選定は `spec/selection/library-tool-selection-v0.md` §13 を改訂し、決定記録を `docs/decisions/` に置く。出典は計画 §5 に置く。

**schema**: Memory の字句索引は、本文の field と、keyword の field（project_id、skill_ids、recall_mode、basis、topic_id）を持つ。AP の `summary`（256 byte 以下の 1 行、版の一部）を別の field にするか本文と同じ field に入れるか、Vector の passage の文に含めるかは、KP-2 の計測で決める（#86 では題名を足すと Vector の Recall@10 が上がった）。analyzer の識別子が問い合わせ側と一致しない世代は、字句について使えない（gap を返す）。analyzer を変えるときは世代を作り直す。Document Source の analyzer の扱いは #85 の範囲で、本書は変えない。

### 3.5 Vector

- **モデルと実行**: main で選定済みの `intfloat/multilingual-e5-small`（revision `614241f6…`、3 file を SHA-256 で照合、`crates/search-vector-adapter/src/lib.rs:28-60`）を Candle 0.11.0 の CPU で動かす（`spec/selection/library-tool-selection-v0.md` §13.4）。この Mac（macOS、Apple silicon）の CPU で動かす。macOS での動作と速さは未確認で、KP-7 の軽い qualification で測る。置き場所は設定で与える。既存 worker の既定 `/var/lib/knowledge-platform/models/multilingual-e5-small`（`crates/search-runtime/src/bin/search_outbox_worker.rs:88`）は Linux 向けなので、この Mac では §3.9 の案の path にする。自動ダウンロードはしない。
- **モデルの選定は KP 全体に従う**: #86 の比較（調整用 52 問、Vector だけ、Recall@10）では e5-small 0.740、ruri-v3-310m 0.817（題名つき 0.885）、EmbeddingGemma 2（768 次元、題名つき）0.827 で、#99 には EmbeddingGemma 2 を Candle に移した commit がある（`e52fd8dc`）。KP の選定が変われば Memory も同じモデルに移る。model identity が変わるので、埋め込みは作り直しになる。
- **Unit**: 1 件の Memory を 1 つの Resource とする。e5 は接頭辞の後 512 token で切る（`lib.rs:7-9`）ので、それを超える本文は文の境界で複数の Unit に分け、hit は memory_id にまとめる。具体的な分け方は KP-4 で決める。
- **接頭辞**: e5 の model card は、対称なタスク（似た文を探す）では両側に `query: ` を勧めている。保存前照合は対称なタスクに近い。索引側を `passage: ` にするか `query: ` にするかは、合成 Memory コーパスの Recall@20 で比べて決める。
- **類似度の下限 τ**: Document 用の 0.890 は MIRACL の文章で校正した値である（`report.md:57`）。Memory では、合成コーパスの校正用の問いだけで同じ手順を踏んで決め直す。
- **v0 では Memory に GPU を使わない**（lead decision 2026-10-15）。KP 全体が製品での GPU の使い方を決めても（所有者の判断待ち、#99 の branch 上の `docs/superpowers/plans/2026-10-07-search-generation-segments.md:168`）、Memory の v0 は CPU のままにする。定常の保存は 1 回に数件で CPU で足りる見込みであり、GPU が効くのは全件の作り直しと 100k の benchmark である（見込み、未計測）。CPU の埋め込みは KP の検証環境の CPU で約 46 Unit/秒だった（同 `:136`）ので、その速さなら 100k 件の全件の作り直しは 40 分前後かかる見込みで、KP-7R（RTX 5090 の機械）で測る。この Mac の CPU での速さは未確認で、この Mac の件数（公式記憶の取り込みの規模）での作り直しの時間を KP-7 で測る。KP は既に、Unit を書き出して所有者の GPU マシンで同じ重みの e5-small（float32）で埋め込み、鍵ごとに値の表へ戻す方法を検証用に使っている（169 万件で 218 秒、CPU の adapter との cosine の最小 0.9999995、同 `:173`）が、Memory の v0 では使わない。v0 の後に GPU を使うとき（この Mac なら Metal、RTX 5090 なら CUDA）は、CPU との一致を P2-02 と同じ基準（token ID が同一、成分の誤差 0.002 以内、cosine の差 0.0001 以内、`spec/selection/library-tool-selection-v0.md:1292`）で確かめてから使う。CUDA で build する場合の条件（CUDA 12.8 以上、ドライバ 570.26 以上、sm_120a 専用の成果物、Candle に sm_120 の公式な対応表明が無いこと）は計画 §5 に出典とともに置く。
- **lane の規則**（M5）: 問い合わせの Vector の lane は、応答に使った世代と同じ世代の Vector 部品だけを使う。今の Vector の検索も、同じ P1 の世代の READY な Vector 世代が無ければその lane を飛ばし、「不在」とは扱わない（main の `crates/search-application/src/vector.rs:415-422`）。古い世代の Vector で答えて、字句の世代の位置を名乗ることはしない。Memory では字句と Vector を同じ構築で作ってから公開する（§3.3 の手順 4）ので、Vector の lane が欠けるのは埋め込みのモデルが使えないときだけになる見込みである。#99 の Vector の構築は別の loop で今の世代に対して動き、更新が続くと追いつかない恐れがある（#99 の設計 `:59`）。公開の前の READY の世代に対して Vector を作れるかは KP-4 で確かめる（未確認）。作れないときは、Vector が遅れている間の応答は `partial` になり、lane の位置は `null` になる（§3.7）。
- **stage の cache**: G7 は #99 で「最新の stage だけを RAM に残す」形になる。KP-4 では、Memory の Source でも pointer の移動の後に古い stage が残らないことを試験で確かめる。

### 3.6 Graph

- **辺の材料**: AP が送る `MemoryRelation` だけ。種類は `depends_on`・`derived_from`・`exception_of`・`supersedes`・`conflicts_with`（設計 v2 §5）。KP では `relation_type = "memory.<kind>"`、`namespace = Semantic`、参加者は役割 `from` と `to` の 2 つ、qualifier に AP の `relation_id`・`from_version_id`・`to_version_id`・`relation_generation` を入れる（`TypedRelationInstance`、`crates/search-core/src/relation.rs:47-58`）。
- **本人ごと**: Graph の世代は Source ごとなので、本人の Graph は自然に分かれる。AP が送った関係は、出す側の item の宣言として保存する。`expand` は、相手の端が同じ世代の現在の item である辺だけを返す。端がまだ取り込まれていなければ、その辺は相手が取り込まれた世代から見えるようになる。端が他の Source にあるかを KP は調べない（他の本人のデータを見に行く処理を作らないため）。`conflicts_with` は AP が id の小さい側にだけ置くので、KP は向きの無い辺として扱い、`direction` にかかわらず両端から返す。関係の `condition`（本文に由来する文）は保存しない。
- **探索**: `/v1/memory/expand` は 1 回で 1 hop だけ広げる（§3.7）。どの枝を続けるかは呼び出す Agent が決め、呼び出しを重ねれば深さに上限は無い（設計 v2 §1.1-6）。1 回の呼び出しの関係数と seed 数は既存の `TraversalBudget`（`graph.rs:63-69`）で抑える。時間と node 数の欄はそこに無いので、Memory の問い合わせ層で期限と node 数を数えて止める。
- **してはいけないこと**: Search は `depends_on` の版鎖が健全かを判定しない。辺の qualifier の版と現在の版の違いを、Search が「失効」と表示しない。
- **辺の鮮度**（M10-2）: `expand` が返す辺は、応答に使った世代に取り込んだ時点の関係である。AP が関係を退役させると、出す側の記憶の `upsert` が届き、次の世代からその辺が消える。それまでの間、退役した関係が辺として出うる。AP は辺の `relationId` が今も `active` かを core で確かめ直してから使う（AP の 03 §5.5）。KP は辺を正本として扱わない。

### 3.7 問い合わせ API

既存の 4 route（`crates/search-api-http/src/router.rs:167-170`）の意味は変えず、3 route を足す。OAS（`spec/api/search-openapi.yaml`）と error 台帳（`spec/errors/search-api-error-registry.yaml`）には追加だけを行い、契約試験を足す（B7 と同じ進め方）。owner と Source は資格情報から決まり、要求には書けない（`crates/search-api-http/src/auth.rs:1-7`）。

綴りの正本（3 route、要求と応答の DTO の欄の名前と値、誤りの code）は次の印の間に置き、§4 の固定値で固定する。各 route の規則は印の後に書く。

<!-- personal-memory-source/2 query-api begin -->

**`POST /v1/memory/search`**

```text
入力 {
  query: 文字列（1〜8,192 byte）
  filter?: {                         // 閉じた演算子だけ。意味（null が「本人全体」など）は AP が決める
    projectId?:  { in: [文字列 | null, ...] }            // 1〜16 個
    skillIds?:   { anyOf: [...] } | { emptyOrAnyOf: [...] } | { empty: true }
    recallMode?: { in: ["attach" | "search", ...] }
    topicId?:    { in: [...] }
    basis?:      { in: ["user_stated" | "agent_observed" | "agent_inferred", ...] }
  }
  retrievers?: ["lexical" | "vector", ...]   // 既定は両方
  limit?: 1〜50（既定 20）
  generationId?: 世代 ID                      // あれば、その世代が現在でないとき 409
}
出力 200 {
  status: "complete" | "partial"
  generation: { id, appliedThrough }          // 応答に使った世代（字句・Graph・Unit の世代）
  lanes: [ { retriever: "lexical" | "vector", appliedThrough: token | null } ]   // 要求した retriever ごとに 1 つ
  items: [ { memoryId, versionId, digest, rank, retrievers: ["lexical" | "vector"] } ]   // 最大 limit 件
  gaps: [ { reasonCode, blocking } ]          // 既存の公開 reasonCode だけ、最大 64
  traceId
}
```

**`POST /v1/memory/expand`**

```text
入力 {
  seeds: [memoryId, ...]                     // 1〜32
  relationKinds: [...]                       // 5 種の部分集合、空は不可
  direction: "out" | "in" | "both"
  exclude?: [memoryId, ...]                  // 0〜4,096。既に訪れたもの
  after?: [ { memoryId, relationId } ]       // 前回途中で切れた seed の続き
  budget?: { maxNodes: 1〜256（既定 64）, maxEdges: 1〜1,024（既定 256）, deadlineMs: 1〜2,000（既定 500） }
  generationId?: 世代 ID
}
出力 200 {
  status: "complete" | "budget_exhausted"
  generation: { id, appliedThrough }
  edges: [ { relationId, kind, from: { memoryId, versionId? }, to: { memoryId, versionId? } } ]
  nodes: [ { memoryId, versionId, digest } ]
  unexpandedSeeds: [memoryId, ...]           // 予算で手を付けなかった seed
  truncatedSeeds: [ { memoryId, afterRelationId } ]   // 途中で切れた seed と続きの位置
  traceId
}
```

**`GET /v1/memory/status`**

```text
入力 なし                                    // owner と Source は資格情報から決まる
出力 200 {
  generation: { id, appliedThrough, publishedAt }   // 現在の世代
  lanes: [ { retriever: "lexical" | "vector", appliedThrough: token | null } ]   // その世代の lane ごと。search と同じ形
  ingest: "current" | "lagging" | "resyncing" | "unreachable"
  purge: "current" | "overdue"
  lastSucceededAt                             // 最後に成功した取り込みの時刻
  traceId
}                                            // 件数も memoryId も返さない
```

**誤り**（新しい code は作らない）

| HTTP | 既存の code | Memory の route での意味 |
|---|---|---|
| 400・422 | `MALFORMED_REQUEST`・`VALIDATION_FAILED` | 入力の形・範囲の誤り |
| 401 | `AUTHENTICATION_REQUIRED` | 資格情報が無い・検証できない |
| 403 | `FORBIDDEN` | scope が合わない |
| 404 | `RESOURCE_NOT_FOUND`（`causeDisclosure: none`） | actor に見える Memory Source が無い。他の本人の Source があっても同じ応答 |
| 409 | `CURSOR_STALE`（`causeDisclosure: none`） | `generationId` が現在の世代でない |
| 503 | `SERVICE_UNAVAILABLE`・`DEPENDENCY_UNAVAILABLE` | 使える世代が無い（未構築、再検証の失敗、backup 復元後の再同期中）。200 の空とは別の応答で、`items` を持たない |
| 504 | `UPSTREAM_TIMEOUT` | 既存の意味のまま |

<!-- personal-memory-source/2 query-api end -->

**`POST /v1/memory/search` の規則**（保存前照合と、必要時の想起）

- filter は順位付けの前にかける（後から絞ると上位 k 件の取りこぼしが出るため）。字句は tantivy の filter 付き問い合わせ、Vector は走査の前に Unit の属性で絞る。
- 設計 v2 の `applies_to` を「Project P と Skill 集合 S の文脈で使える」と読む AP は、`projectId: {in: [null, P]}` と `skillIds: {emptyOrAnyOf: S}` を送る。保存前照合では filter を付けず、本人の全 Memory から探す。
- 順位は既存の S1（retriever の順の優先連結、生の score を足さない、`spec/selection/library-tool-selection-v0.md` §13.3）に従う。score は返さない。
- `lanes`（M5）: 要求した retriever ごとに、その lane が実際に走査したデータの変更列の位置を返す。`lexical` は応答に使った世代の `appliedThrough` と同じ値である。`vector` は、同じ世代の Vector 部品で走査したときだけ同じ値で、走査できなかったときは `null` である（§3.5 の lane の規則）。v2 では lane が古い世代で答えることは無い。将来それを許すときは、その lane の古い位置を返し、gap `FRESHNESS`（blocking=false）を付ける。AP は `null` でない lane の位置の最小を「ここまで見た」として未反映の差分を数える（AP の 03 §5.5）。
- `status` は、要求した retriever がすべて応答に使った世代の全体を走査し終え、`lanes` の位置がすべて `generation.appliedThrough` と同じなら `complete`。一部の lane が使えなかったら `partial` で、その lane は `null`、gap は既存の公開 reasonCode `AVAILABILITY`（blocking=false。OAS の enum は `spec/api/search-openapi.yaml:730-742`）。lane の位置が古いときは `FRESHNESS`（blocking=false）。`complete` で `items: []` なら「その世代に該当が無い」という意味になる。Vector の lane が欠けた `partial` を保存の照合に使ってよいかは AP が決める（AP の 03 §3.4）。
- 本文・`summary`・`digest`・問いの文を log・trace・metrics・誤りの本文に出さない（§3.8、M10-4）。`digest` は塩の無い本文の SHA-256 なので、短い記憶は推測して当てられる。
- Memory の route は pin（評価ごとの lease）を作らない。`generationId` は「その世代が今の世代か」を確かめるためだけに使う。lease を作るのは評価の pin（`crates/search-runtime/src/pin.rs:252`。`search_evaluation_lease` への INSERT は `:326`）で、Memory の route はこれを呼ばない。Memory の route は、その Source の現在の世代を読み、その世代の鍵で字句・Graph・Vector を引く。今の Vector の道（`VectorRetriever::retrieve`、`crates/search-application/src/vector.rs:402-403`）は呼び出し側が pin した鍵を受け取る前提で書かれているが、中の `pin_current`（`:415-418`）の PostgreSQL の実装は、その鍵の Vector の manifest を読むだけで lease の行を作らない（`crates/search-runtime/src/vector_store.rs:469-489`）。KP-5 は Vector の lane にこの道を、評価の pin を作らずに使う。読む間に世代が回収されて manifest が無ければ、この道は unavailable を返す（`vector.rs:419-421`）ので、その lane は `null`、`status` は `partial`、gap は `AVAILABILITY` になる（上の `status` の規則）。KP-5 の試験 `memory_routes_create_no_evaluation_lease` で確かめる。将来 pin を使うときも、寿命の上限は `PinTtl` の 300 秒（`pin.rs:42`）で、消去の目標時間（§3.3 の `purge_target`、既定 30 分）に収まる（p0-review r2 LOW 12）。

**`POST /v1/memory/expand` の規則**（連想探索の 1 歩）

- 状態を server に持たない。続きは AP が `seeds`・`exclude`・`after` を渡して呼び直す。辺の順は (kind の固定順, relation_id) で決まるので、`after` は同じ世代の上で決定的に続く。これで RAM cursor の失効（G11）を避ける。
- 本人の Source に無い ID を seed にしても、辺が無いだけで error にしない。他の本人の ID かどうかで応答が変わらない。
- Graph の部品は字句と同じ世代の一覧に入る（#99 の設計 §3.3）ので、`expand` の反映位置は `generation.appliedThrough` 1 つで足りる。辺の鮮度は §3.6。

**`GET /v1/memory/status` の規則**: 本人の Source について、現在の世代（id、`appliedThrough`、公開時刻）、その世代の lane ごとの反映位置（`lanes`、`search` と同じ形）、取り込みの状態（`current` | `lagging` | `resyncing` | `unreachable`）、忘却の消去の状態（`purge`: `current` | `overdue`。`suppressed` から 24 時間を過ぎても §3.3 の (a)〜(c) を終えていない記憶があれば `overdue`）、最後に成功した時刻を返す。件数も memory_id も返さない。

**error**: 新しい code は作らず、既存の code の `operations` に Memory の 3 route を足す（固定 detail・`no-store`、`problem.rs:1-7`）。 403 の scope は §3.2 の面の分離による。

**cursor を使わない理由**: `search` は 1 頁（最大 50 件）で足り、`expand` は呼び出し側が続きの位置を持つ。どちらも RAM cursor（G11）を要らない。将来深い頁が要るときは、世代と要求の digest に束縛し server の鍵で署名した keyset の位置を使い、RAM には持たない。

### 3.8 漏洩を防ぐ仕組み

| 経路 | 仕組み | 試験（担当 PR） |
|---|---|---|
| 結果・本文 | Source が本人ごと。本文は返さない | `memory_api::other_owner_sees_not_found`（KP-5） |
| 件数 | `total`・`count` を返さない。`items` は `limit` 以下 | `memory_api::no_totals_in_any_route`（KP-5） |
| 順位・時間 | 字句の統計と Vector の走査は本人の Source の中だけ | `memory_api::scores_independent_of_other_owner`（KP-5。他の本人の Memory を増減しても順位が同じ） |
| error | 他の本人の Source があっても無くても同じ 404。固定 detail・`no-store` | `memory_api::problem_codes_fixed`（KP-5） |
| trace | stage と結果 code だけで、数を入れない（G15 の Discover とは別） | `memory_api::trace_has_no_counts`（KP-5） |
| 既存の 4 route | Memory Source を対象に入れない。`memory.search` の資格情報は 403 | `server_e2e::memory_source_absent_from_existing_routes`（KP-3） |
| ログ・metrics | 問い・本文・`summary`・`digest`・memory_id を書かない（M10-4。塩の無い digest は短い記憶を推測して当てる材料になる）。trace_id、route、status、遅延の区分だけ | `memory_api::logs_have_no_query_body_or_digest`（KP-5）、`memory_ingest::logs_have_no_body_or_digest`（KP-4）。どちらも log を捕捉し、canary の本文・summary・digest の 16 進が出ないことを見る。同じ捕捉で canary を意図して書いた行が見つかる対照を置き、捕捉が働いていることを先に確かめる |
| 古い世代 | 抑止は pin 付きの古い世代にも効く | `memory_ingest::suppression_applies_to_pinned_generation`（KP-4） |
| 忘却の後の写し | `purged` の前に、抑止した記憶を含む字句の segment を書き直し、古い世代と行を消す（§3.3 の忘却の物理的な消去） | `memory_ingest::purge_rewrites_segments_holding_suppressed`、`purged_leaves_no_canary`、`purge_completes_without_further_updates`、`purge_overdue_reported`（KP-4）。canary の検査は、Source の lexical root の全 segment を削除の印を無視して開き（stored の本文と語の辞書）、PostgreSQL の行も引く。忘却の前に同じ検査が canary を見つけることを対照として先に確かめる（doc store は既定で lz4 圧縮なので、file の byte 列をそのまま探すだけでは見落とす。`third_party/search/tantivy-0.26.2/Cargo.toml:47-53`、`src/store/compressors.rs:135-143`） |

### 3.9 Search server・資格情報検証・配備

**binary**: `crates/search-runtime/src/bin/search_server.rs`（既存の `search_outbox_worker` と同じ crate に置き、新しい crate を増やさない）。

- 命令: `serve`、`migrate`、`check`（設定・schema・モデル・辞書の照合だけをして終わる）、`inventory publish <file>`、`memory resync <source_id>`。
- `serve` は 1 process で API と Memory の取り込み task を動かす（常駐の process を増やさない、設計 v2 §1.1-10）。取り込み task の panic が API を止めないよう、task を分けて監視し、再起動する。
- 組み立て: `PgHostRegistrationSource`（本番の登録、`host_inventory.rs:12-16`）、`build_search_api_runtime`（`api.rs:211`）に Memory の route と backend を足したもの。Document Source が 0 件でも起動できる形にする（Memory だけの配備で Document Platform に依存しないため）。具体的な port の組み方は KP-1 で決める。
- 既存 worker は秘密を環境変数から読む（`search_outbox_worker.rs:1-8`）。この Mac の配備では launchd の plist に秘密を書かない（下の「配備」）ので、DB の接続情報と KP indexer の署名鍵は、専用の利用者だけが読める file から読む形を KP-1・KP-4 で足す。既存の search 系 code には file から秘密を読む処理は無い。秘密はログに出さない。

**資格情報検証器**（`SearchCredentialVerifierPort` の本番実装、`auth.rs:33-35`）:

- Bearer token は AP が本人ごとに発行する compact JWS とする。header `{alg: "EdDSA", kid}`、claim `{iss, aud, sub（owner principal）, tenant, scope: "memory.search", iat, exp, jti}`。`exp - iat` は 300 秒以下。
- KP は設定 file に `kid` ごとの AP の公開鍵（Ed25519）を置く（公開鍵なので秘密ではない）。検証は `alg` の固定、`kid` の既知、署名、`aud` の一致、`exp`・`nbf`・`iat`（時計のずれ 30 秒以内）、`scope` の値。どれかが合わなければ 401（固定の challenge `Bearer realm="search"`、`auth.rs:19-20`）。network には出ない。
- 検証に通ると server が不透明な session handle（`AccessContextHandle`）を作る。要求の本文に書かれた identity は使わない（`auth.rs:1-7` の規則のまま）。
- 署名の検証には `ring` 0.17.14 を使う候補が強い。rustls 経由で既に lock graph にある（`Cargo.lock:4762-4763`）。直接依存にするには選定記録が要る（KP-1）。
- AP は本人の認証と Agent の委任を確かめてから assertion を発行する（設計 v2 §9）。AP の鍵が漏れれば任意の本人を名乗れるので、KP は登録済みの owner principal を持つ Source にしか grant を返さず、`memory.search` の scope では Document Source を見せない。鍵は `kid` で入れ替えられるようにする。
- KP→AP（変更列の取得）は逆向きの資格情報で、KP indexer の service principal が自分の Ed25519 鍵で要求ごとに JWS を作る（§4 の合意: `aud = "personal-memory-source"`、method と path と `space_id` に束縛、有効 60 秒以内、`jti` は使い捨て）。scope の claim は持たず、AP が `source_grants`（購読者 × space）で許可を決める。KP の公開鍵を AP の `source_subscriber_keys` に入れ、`source_grants` を足す手順は、KP-6 の手順書と AP 側の配備の手順書に置く。

**通信**: KP は AP の Memory API と同じこの Mac に置き、`127.0.0.1` だけで待ち受ける。Tailnet にも LAN にも出さない。別の PC の Agent は KP に直接つながず、Tailscale 越しに AP の Memory API を通す（設計 v2 §9「原則閉じたネットワーク・TLS」は AP の側で満たす）。

**配備（この Mac。owner decision 第 2 回）**。以下は設計の案で、macOS で動くことは未確認である。KP の CI は macOS では `cargo check --workspace` だけを回し（`.github/workflows/ci.yml:148-162`、`mise.toml:290-296`）、search 系の試験は Linux の container で回る（`ci.yml:111` の `postgres:18.6-bookworm`）。KP-6 で確かめる。

- process: `search_server serve` と PostgreSQL を launchd で常駐させる。どちらも login の前から動く LaunchDaemon とし、専用の macOS の利用者（例: `_kpsearch`）で動かす（`UserName`）。owner の利用者で動く Agent が、KP の写し（本文を持つ Unit 部品と字句の file）を直接読めないようにするためである。Document Source が無い配備では `search_outbox_worker` は要らない。
- PostgreSQL: 推奨は macOS の素の PostgreSQL を launchd で動かすこと（login の前から動き、VM の memory を使わない）。版は KP の試験と同じ 18.6 にしたいが、macOS の配布で 18.6 を使えるかは未確認で、使えなければ 18 の最新の minor にして差を記録する。KP の migration は拡張を使わない（Vector も `BYTEA`、#99 の branch の `crates/search-runtime/migrations/0012_search_vector_segments_v1.sql:17`）ので、拡張の build は要らない。代わりの案は OrbStack の container で試験と同じ image を動かすことだが、OrbStack が owner の login の後にしか動かず、Linux の VM の memory を使う。接続は Unix socket と `peer` 認証にして DB の password を置かない案を KP-6 で確かめる（sqlx での可否は未確認）。
- DB: v0 は 1 つの database とし、本人ごとに分けない（lead decision 2026-10-15）。本人の隔離は Source の分離（§3.2）と K1 の鍵で行う。KP の判断は「当面 Document と同じ database に同居させ、将来 Search を別 server へ分けられる形で作る」である（`docs/decisions/2026-10-05-search-graph-store-and-db-placement.md:6`）。この Mac には Document Platform を置かない見込みで、その場合は Search 専用の database になる。判断が挙げる唯一の結合点（同 `:28`）は Memory には無いので、分離の手順（同 `:29-31`）は要らない。
- file: 既存 worker の既定の path（`/var/lib/knowledge-platform/…`、`crates/search-runtime/src/bin/search_outbox_worker.rs:88`）は Linux 向けなので使わず、設定で与える。案: `/Library/Application Support/knowledge-platform/` の下に `models/multilingual-e5-small`・`search/lexical`・`search-server.json` を置き、専用の利用者の所有で directory は 0700 にする。Lindera を使うことになった場合だけ `dictionaries/<analyzer-id>`。
- 秘密: launchd の plist は root の所有で、他の利用者からも読める権限で置くのが普通なので、DB の password や KP indexer の署名鍵を plist の `EnvironmentVariables` に書かない。専用の利用者だけが読める 0600 の file に置く（KP-4 の「環境変数か file か」は、この Mac では file にする）。権限の既定は KP-6 で確かめる。
- CPU と memory: この Mac の 1 分 load は本番の間も 20 以下に保つ。macOS には systemd の `MemoryMax`・`CPUQuota` に当たる cgroups が無く、launchd の資源制限で常駐の memory を確実に絞れるかは未確認なので、KP の設定で抑える: 埋め込みと字句の構築の thread 数の上限（案 2）、同時に作る世代は 1 つ、全件の再同期は少しずつ進める、部品の cache の上限、process の `Nice`。上限の案（未計測。KP-7 の軽い qualification で確かめて固定する）: `search_server` の idle の RSS は 1 GiB 以下（e5-small の重みだけで約 444 MiB、#99 の計画 `:264`）、PostgreSQL は 512 MiB 以下、保存 1 件の取り込みが load へ足す分は 2 以下。
- GPU: Metal を含め使わない（v0 の lead decision）。CPU の既定の build で動かし、Accelerate などの最適化は予算を外したときだけ検討する（依頼者の決定 2）。
- 眠り: この Mac が眠ると、KP・PostgreSQL・AP の Memory API が全部止まり、別の PC からは使えない。起きたら取り込み task は保存した `applied_through` から続け、S2 の long poll は呼び直す。AP と KP が同じ機械で一緒に止まるので、片方だけが進んで食い違うことは無い。忘却の期限 T（24 時間）は壁時計で数えるので、`suppressed` の後に `purged` の前で 24 時間を超えて眠れば `overdue` になり、そのとおり報告する（KP は次の世代の構築で書き直すので、起きている間の忘却では起きにくい見込み）。別の PC から使うために眠りを止めるかは owner の設定の判断である。
- backup: Search の状態は AP から作り直せるので、KP の正しさのための backup は要らない。KP の data（PostgreSQL の data directory と `/Library/Application Support/knowledge-platform/`）は Time Machine などの backup の対象から外すことを推奨する（外せば、忘却した記憶の写しが backup に残らない）。APFS の local snapshot に除外が効くかは未確認である。外せない場合、その backup と snapshot に写しが残り、`purged` はそれを含まない（§3.3）。この Mac を backup から戻したときや別の Mac へ移したときは、§3.3 の復元の規則（S5）で再同期する。AP の data の控え（外の disk など別の場所へ）は AP の設計が決める。disk の暗号化（FileVault）が有効なら、空き block に残る写しを disk を外して読むことはできない（owner の Mac の設定は未確認）。
- build: この Mac で release の build を回すと 1 分 load 20 を超えうる。推奨は、hosted CI の `macos-15`（arm64）で `search_server` の release binary を作り SHA-256 とともに成果物にする job を KP-6 で足すこと。この Mac で build するときは `-p search-runtime --bin search_server` に絞り、`CARGO_BUILD_JOBS=2`（KP の macOS の CI の job と同じ値、`ci.yml:172`）で、load を見ながら回す。

### 3.10 資源の計測契約

数値はまだ無い。計測は 2 つに分け、数値を混ぜない（owner decision 第 2 回）。

- **この Mac の軽い qualification**（計画 KP-7）: 本番の予算はこの数値だけから決め、計画に固定して PR の門に入れる（設計 v2 §10.3）。1 つの session の主な経路だけを測り、同時の負荷はかけない。計測の間、1 分 load を一定の間隔で読み、18 を超えたら止める（20 より上げないための余裕）。最大の load を報告に書く。本番の `search_server` とは別の process・別の port・別の database・別の字句の root で、合成データだけを使う。本番の Memory の Source には触らない。
- **RTX 5090 の重い計測**（計画 KP-7R、ssh の承認の後）: 規模と同時の負荷を、機材（CPU・RAM・disk・OS と kernel・vLLM の状態）を明記した数値として測る。この Mac の予算には使わない。#86 も同じ機械を使うので時間を調整し、vLLM が動いている状態と止めた状態を分けて記録する。

計り方は #86・#99 の方法（工程ごとの RSS、切替の止まり、冷えた起動）に合わせ、比べられるようにする。

| 区分 | この Mac の軽い qualification（予算を決める） | RTX 5090 の重い計測（機材つきの参考値） |
|---|---|---|
| 容量 | aarch64-apple-darwin の release binary の byte（strip の有無）、動的依存、モデル（と辞書）の byte。固定の toolchain（Rust 1.98.1）と feature | x86_64 Linux の同じ項目 |
| 常駐 | idle の RSS と CPU、取り込み中の peak RSS（`search_server` と PostgreSQL を分けて）。合成 1k 件と、公式記憶の取り込みで見込む件数 | 1k・10k・100k 件を読み込んだ後の RSS、全件構築中の peak RSS。本人の Source 1・4・16 個 |
| 起動 | cold・warm の起動時間（モデルの読み込み、世代の検証を分けて）。OS のキャッシュあり・なし | 1k・10k・100k 件で同じ項目 |
| DB と索引 | 1k 件あたりの PostgreSQL の表と索引の byte（Unit、Vector、Graph、抑止）、字句のファイルの byte、WAL の増え方。Memory 本文は 4 KiB 以下の合成分布と日本語の長文の境界例 | 10k・100k 件で同じ項目 |
| 取り込み | 1 件の保存から検索できるまでの時間（字句と Vector を同じ構築で作る形で）、そのときの load の増え方 | 変更 1・10・100 件の batch、関係の平均 0・2・10、全件の再同期時間、CPU での埋め込みの件数/秒 |
| 忘却 | 1 件の `remove` から `suppressed`・`purged` まで（protocol の T = 24 時間の中で実際にかかる時間）、書き直した segment の byte | 更新が続く場合と止まった場合、本人の Source 1・4・16 個 |
| 問い合わせ | 1 つの session の `search`（字句だけ、Vector だけ、両方）と `expand` の p50・p95 | 同時 1・4・16、各 1,000 回以上の p50・p95・p99、世代の切替の直後の止まり（Memory は pin の意味を変えないので、止まりはそのまま測る） |
| 品質 | 測らない（品質は KP-2 が hosted CI で測る。§3.4 の Recall@20・nDCG@10、L＋V の Recall@20） | 同左 |
| 隔離 | §3.8 の試験が同じ build で通ること。速さのために試験を削らない（設計 v2 §10.3 末尾） | 同左 |

この Mac の軽い qualification が本番の予算を外したときだけ、最適化の作業を始める（依頼者の決定 2）。予算を外していない段階で、見込みだけで最適化の PR を作らない。RTX 5090 の数値は規模の見通しに使い、この Mac の予算の合否には使わない。

## 4. Source protocol `personal-memory-source/2`（AP と合意、2026-10-15）

- **正本**: agent-platform の `docs/design/personal-memory/03-core-schema-api.md` §5.4（取り込みの契約）と §5.5（AP が §3.7 の route をどう使うか）。branch `design/personal-memory-p0`、commit `ee5dea94e507f4cbbb9edf1539210ff50d5b2438`。版は、03 の契約の範囲（§5.4 と §5.5。行 `<!-- personal-memory-source/2 begin -->` と `<!-- personal-memory-source/2 end -->` の間の byte 列で、印の行は含まない）を次の command で抜いた byte 列の SHA-256 で固定する: `awk '/^<!-- personal-memory-source\/2 begin -->$/{f=1;next} /^<!-- personal-memory-source\/2 end -->$/{f=0} f' docs/design/personal-memory/03-core-schema-api.md | shasum -a 256` → `2415f631fc87c0686d032b786d65f696ed5c0a7649fb4be371e4c40896e31b31`（fix-kp がその commit の file から計算し直して一致を確かめた）。その後の AP の commit（印の外だけを直した `a6578732197ddab526e9755dbe02b2e947686944`・`5b4ceea44e950b9e2071fc0022628500b875dbfb`・`94fc0d81ab58ed8ddb758454c5a34bfa1e1dc79b`）でも、印は 1 組で、同じ command の値は同じだった（fix-kp が 2026-10-15 に確かめた）。契約は commit の id ではなくこの値で固定する。固定値が同じなら、本書が引く AP の commit は最初の記録（`ee5dea94`）のままでよい。AP の後の commit で印の中が変わっていないことは、この値を計算し直して確かめる（p0-review r3 LOW 2）。commit の id とこの固定値は、AP の 03 では印の外の §5.6 に置く（印の中に置くと、両方の記録が互いの値を待つ輪になる）。03 の file 全体の SHA-256 では固定しない（契約でない行を直すたびに値が変わるため。レビューの LOW 17）。KP はこの版を実装する。契約の範囲を変えるときは版を上げる。KP-4 で JSON 例と schema の写しを `spec/api/personal-memory-source-v2/` に置き、正本の commit と上の SHA-256 を冒頭に書いて conformance 試験に使う。
- **版の履歴**: `/1` は agent-platform `302fdffa402a4702f280247e6ab14290741eeeba`（その commit の 03 の file の SHA-256 `36fa10d0447cc073f599dd32b3cedea390e2444405b1006c49db9b89def03f2f`）で合意した版で、実装していない。`/2` で変えたもの: (1) `suppressed` と `purged` の意味と期限 T = 24 時間（§3.3。レビューの H3）、`/v1/memory/status` の `purge`。(2) `/v1/memory/search` と `/v1/memory/status` の `lanes`（§3.7。M5）。(3) K8 に `digest` と問いの文を足した（§3.8。M10-4）。(4) 保存時の照合で、AP が core で確かめ直して落とした hit の件数だけ `limit` を足して 1 回だけ問い直す（上限 40。KP の `limit` の上限 50 の内。M10-3）。KP の `search` に `exclude` は足さない。(5) `expand` の辺は、`relationId` が core で今も `active` で両端が合うものだけを AP が使う（§3.6。M10-2）。(6) Item に AP の `attach_grant`（attach を付けた根拠）は入れない。KP は `recall_mode` だけで絞る。S1〜S5 の要求と応答の欄は `/1` から変えていない（AP の 03 §5.6）。KP は `/1` の応答を受けない（計画 KP-4 の `protocol::rejects_protocol_v1`）。
- **合意**: 2026-10-15 に fix-core（AP 側の正本の担当）と fix-kp（本書の担当）が `/2` で合意した。`/1` は ap-core と kp-plan が合意した。どちらも担当の間の合意で、経緯は AP の 03 §5.6 にある（AP 側の log の規則は同 §5.7）。所有者の承認はまだで、本書と AP の設計の承認と一緒に求める。
- **問い合わせ API の綴り**の正本は本書 §3.7 である（AP の 03 §5.5 は使い方だけを書く）。`personal-memory-source/2` の一部として、§3.7 の行 `<!-- personal-memory-source/2 query-api begin -->` と `<!-- personal-memory-source/2 query-api end -->` の間の byte 列（印の行は含まない。3 route、要求と応答の DTO の欄の名前と値（`search`・`status` の `lanes` を含む）、誤りの code）を、knowledge-platform の repository の根で次の command で抜いた SHA-256 で固定する: `awk '/^<!-- personal-memory-source\/2 query-api begin -->$/{f=1;next} /^<!-- personal-memory-source\/2 query-api end -->$/{f=0} f' docs/superpowers/specs/2026-10-15-search-personal-memory-source-v0-design.md | shasum -a 256` → `c05acd4a78ad422459c268310ab2d059ec6b2f6f4d5115fff0464c0fa3c23c04`。印の中には commit の id・固定値・日付を置かない（固定値を印の中に置くと、値が自分を待つ輪になる）。AP の 03 §5.6 はこの値を引く。印の外（§3.7 の各 route の規則、§3.2・§3.3・§3.5・§3.6 など）を直しても値は変わらない。そこを直して route の意味が変わるときは、値が同じでも版を上げる（p0-review r2 LOW 6）。

KP が実装する経路（根は `/source/v1/spaces/{space_id}`）:

| # | 経路 | KP の使い方 |
|---|---|---|
| S1 | `GET items?cursor=&limit=`（1〜500） | 初回と再同期の完全な一覧。`active` だけで、行は `{memory_id, version_id, revision, relation_generation, item_seq, digest}`。`snapshot_token` は走査を始めた時点の位置で全頁同じ値。走査中の変更は必ず S2 に出る |
| S2 | `GET changes?after=&limit=&wait_ms=`（limit 1〜500、wait_ms 0〜25,000） | 増分と long poll。記録は `{seq, memory_id, kind, version_id?, revision, relation_generation, digest?, reason?}`。`kind` は `upsert` か `remove`、`reason` は `disabled` か `forgotten`。本文なし。同じ memory_id の記録は最後の 1 つだけが意味を持つ |
| S3 | `POST items:batchGet`（`{ids}`、100 件まで） | 本文つきの現在値。`active` でなければ `{memory_id, state: "removed"}`（理由なし）。Item は `body`（4,096 byte 以下）・`summary`（256 byte 以下、版の一部）・`topic_id`・`applies_to`・`recall_mode`・`basis`・時刻・出ていく `active` な関係（`condition` は来ない）。`item_seq` が記録の `seq` より大きければ新しい方を使う |
| S4 | `POST acks`（`{applied_through, generation_id, suppressed[], purged[]}`）→ 204 | 世代を公開したら `applied_through`、抑止を確定したら `suppressed`、§3.3 の「忘却の物理的な消去」の (a)〜(c) を終えたら `purged`（`disabled`・`forgotten` の両方。AP は `forgotten` の分だけを使う）。期限は `suppressed` から 24 時間で、過ぎても終えていなければ status に `overdue` を出し、終えた時点で遅れても送る。冪等で、`applied_through` は単調 |
| S5 | `GET acks/last` | 起動時に読み、AP の記録より古い世代で答えない（§3.3 の backup からの復元） |

- 全応答に `protocol: "personal-memory-source/2"` と `space_id` が入る。KP は登録と違えば拒む。`Cache-Control: no-store`。
- token は不透明・単調・1,024 byte 以下（AP が MAC を付けて発行）。410 `token_expired`（刈り込み、または AP の backup からの復元による epoch の変化）のとき、KP はその Source の索引を捨てて S1 から取り直す。
- AP の誤りは本文が `{"code"}` だけ: 400 `bad_request`、401 `unauthenticated`、404 `not_found`（知らない space と許していない space を区別しない）、410 `token_expired`、413 `too_large`、429 `rate_limited` と 503 `unavailable`（`Retry-After` 付き）。KP は登録済み Source の 404 を「0 件」や「消えた」とせず、その Source を使えない状態として扱う。
- 認証は §3.9。KP→AP は KP indexer の EdDSA JWS で、許可は AP の `source_grants`。AP→KP は本人ごとの EdDSA JWS で、Source は `(tenant, sub)` から決まる。
- 置き場: AP の Memory API・KP・KP の PostgreSQL を同じ host（この Mac、owner decision 第 2 回）に置き、AP と KP の間は loopback。置き場は契約の範囲の外で、AP の 03 では §5.6 に書く。
- **AP の要求 K1〜K9**（AP の 03 §5.4.8）と本書の対応: K1（全置き場の鍵に Source、問い合わせの Source は資格情報から）→ §3.2・§3.7（Vector の値の鍵も `source_id` を含む、§3.3。#99 の Unit 部品の主キーは内容の digest だけだが、その digest は `ResourceVersionRef` を覆い、それが `source_id` を持つので、本人をまたいで共有されない。#99 の branch の `crates/search-source-document/src/body_manifest.rs:516-527`、`crates/search-core/src/knowledge_unit.rs:43-47`）、K2（件数を返さない、統計は Source の中だけ）→ §3.8、K3（即時の抑止、字句の segment の書き直しと古い世代・行の消去を終えてから purge、期限 24 時間を過ぎたら overdue で終えた時点で遅れても purge、抑止の行に digest なし）→ §3.3 の手順 2・6・7 と「忘却の物理的な消去」、K4（世代 ID と `appliedThrough`、lane ごとの位置）→ §3.7、K5（版・`relation_generation`・`item_seq` を持ち、古い取得結果を捨てる）→ §3.3 の手順 3、K6（日本語、順位付けの前の絞り込み）→ §3.4・§3.7、K7（空の 200 と 503 を分ける）→ §3.7、K8（本文・summary・digest をログ等に出さない）→ §3.8、K9（起動時に S5）→ §3.3 の backup からの復元。
- AP の使い方（AP の 03 §5.5）: 保存時の照合は filter なし・`limit` 20（KP-2 の主指標 Recall@20 と同じ）で、確かめ直しで落とした hit の件数だけ `limit` を足して 1 回だけ問い直す（上限 40）、`lanes` の `null` でない位置の最小で未反映の差分を数える、想起は `projectId: {in: [null, P]}` と `skillIds: {emptyOrAnyOf: S}`、連想の関係の段は `expand`（辺は `relationId` が core で今も `active` で両端が合うものだけを使う）、`partial` は gap が全て blocking=false のときだけ受ける（Vector の lane が欠けた `partial` の扱いは AP が決める）、409 は `generationId` を付けずに 1 回だけ問い直す、404 と 503 は使えない扱い。

## 5. 試験と受入シナリオの対応

| 設計 v2 | KP 側で確かめること | 試験（担当 PR） |
|---|---|---|
| A06（他人・削除済み・循環・未知の型の関係を拒む） | KP は同じ Source の現在の item 同士の辺だけを作る。未知の kind は batch を Unknown にする | `memory_ingest::relation_endpoints_same_source`、`memory_ingest::unknown_relation_kind_is_unknown`（KP-4） |
| A08（忘却が索引と cache に届く） | 抑止の即時性、字句の segment の書き直しと古い世代・行の消去の後にだけ `purged`、期限、backup 復元後の再同期 | `memory_ingest::{remove_suppresses_immediately, purge_rewrites_segments_holding_suppressed, purged_leaves_no_canary, purge_completes_without_further_updates, purge_overdue_reported, restored_backup_blocks_until_resync}`（KP-4） |
| 設計 v2 §6-2（旧い索引を全件取得済みと偽らない） | lane ごとの反映位置。Vector の lane が欠ければ `complete` にしない | `memory_api::{lanes_report_each_retriever_position, vector_lane_absent_is_partial_not_complete}`（KP-5）、`memory_ingest::generation_publishes_lexical_and_vector_together`（KP-4） |
| A09（別 PC から同じ ID） | 別の PC は AP 経由。KP は資格情報の owner だけを見る | `memory_api::assertion_owner_binds_source`（KP-5） |
| B01（他の本人の情報を全経路で返さない） | §3.8 の表 | §3.8 の試験一式 |
| B02（empty と unavailable） | 200 の `items: []` と 503 の別 | `memory_api::complete_empty_vs_unavailable`（KP-5） |
| B05（有限の頁を繰り返しても無限 loop にならない） | `expand` の予算・`exclude`・`after` | `memory_api::{expand_budget_frontier, expand_stateless_continuation}`（KP-5） |
| D03（1 件の変更で全員の索引を作り直さない） | 世代は Source ごと | `memory_ingest::change_rebuilds_only_its_source`（KP-4） |
| D04（障害を成功や 0 件と混同しない） | 提供者の失敗は Unknown、利用不可は 503 | `memory_ingest::provider_failure_is_unknown_not_absence`（KP-4） |

## 6. 決めていないこと

決めていないこと:

1. Unit の分け方（1 件 1 Unit を基本に、512 token を超える分の扱い）。KP-4 で決める。
2. Memory の字句の問い合わせ方式と L＋V の組み方（KP-2 の計測の後。基準は固定済み、§3.4）。
3. `suppressed` から `purged` までに実際にかかる時間の予算（protocol の上限 T = 24 時間の中で、KP-7 で測って固定する、§3.3）。
4. 公開の前の READY の世代に対して Vector 部品を作れるか（KP-4 で確かめる、§3.5）。
5. この Mac での配備の細部（PostgreSQL 18.6 を macOS で使えるか、Unix socket と `peer` 認証、launchd の plist の権限、APFS の local snapshot への除外、§3.9）。KP-6 で確かめる。
6. 2026-10-29 に #85・#99 が動いていなかったときの進め方（owner が決め直す。冒頭の 10）。
7. RTX 5090 の機械への ssh の承認の時期（owner の Tailscale の承認待ち。待つのは KP-7R だけだが、KP-7R は AP の 02 で release の条件なので、v9.0.0 の前に要る）。

前の版で未決としていて、決まったもの:

- 保存前照合の問い合わせに本文全体を使うか要約を使うか: AP が本文全体と決めた（AP の 03 §3.4 の段 3、§5.5）。KP は 8,192 byte まで受ける。
- 本人ごとに PostgreSQL の database まで分けるか: 分けない（lead decision 2026-10-15）。
- GPU の使い方: v0 では Memory に GPU を使わない（lead decision 2026-10-15）。モデルの変更は KP 全体の選定に従う（§3.5）。
- 世代の切替の間に前の世代で答えるか: Memory は pin の意味を変えない（lead decision 2026-10-15）。切替の止まりは KP-7R（同時の負荷）で測る。
- #85・#99 を待つか: 2026-10-29 を判断日として待つ（owner decision 第 2 回）。
- 配置・控え・計測の窓: すべてこの Mac に置く。KP の data は backup から外す推奨。本番の予算はこの Mac の軽い qualification、規模は RTX 5090 の重い計測（owner decision 第 2 回、§3.9・§3.10）。

## 7. 参照

- 監査 C（2026-10-15、KP Search の適合）とそこで引いた file:line。本書の §2 は同じ commit で読み直した。
- 進行中の Draft PR #85・#86・#99 の branch（§2.1 の commit）。
- 外部資料（2026-10-15 に確認）は計画 §5 に URL ごとに置く。

## 8. レビュー対応（p0-review 2026-10-15）

p0-review（VERDICT changes-needed、KP の基点 `7ba63b15`）の指摘のうち、KP の 3 文書（本書・計画・状況）に掛かるものの扱い。「本書」は本設計、「計画」は実装計画を指す。

| 指摘 | 扱い | どこを直したか・却下の根拠 |
|---|---|---|
| H3 忘却の `purged` が #99 の部品方式で成り立たない | 直した | 本書 §1.2（KP が持つ写し）、§2.1（#99 の Unit 索引は本文を stored で持ち、hard link と削除の印で共有する事実）、§3.3 の手順 2・4・6 と「忘却の物理的な消去」（強制の書き直し、時限の回収、行の消去、含まないもの、期限、実装までは `purged` を送らない）、§3.8、§4、§5。計画 KP-4 の範囲と試験 `purge_*`・`purged_leaves_no_canary`（対照つき）。`suppressed`・`purged` の文言は fix-core と合わせた（AP の 03 §3.6・§5.4.5） |
| M5 `appliedThrough` が 1 つで Vector の遅れを隠す | 直した | 本書 §3.3 の手順 4（字句と Vector を同じ構築で公開）、§3.5 の lane の規則（同じ世代の Vector だけを使う。main の `vector.rs:415-422`）、§3.7 の `lanes` と `status` の規則、`/v1/memory/status` の `lanes`、§5。計画 KP-4 `generation_publishes_lexical_and_vector_together`、KP-5 `lanes_report_each_retriever_position`・`vector_lane_absent_is_partial_not_complete` |
| M10 Source 単位の可視性への置き換えの評価 | 評価を受け入れ、KP に掛かる穴を直した | 機密性は per-item authorize と同等以上という評価に同意する。M10-1（03:111 の authority の note）は AP 側。M10-2 は本書 §3.6「辺の鮮度」。M10-3 は AP が `limit` を上げて問い直す（AP の 03 §5.5。KP の `search` に `exclude` は足さない）。M10-4 は本書 §3.7・§3.8 と §4 の K8（`digest` を log 等に出さない）、計画の log の試験に対照を置いた。M10-5 は H3 |
| M13 KP の Draft PR への依存と共有の機械 | 直した | 本書 §2.1（2026-10-15 の `gh pr view` の読み取り: #85・#86 は `CONFLICTING`、#99 は base が #85 の branch で題は「作業中」。承認されたのは #99 の設計で PR ではない。#99・#86 に残る所有者の判断と未達の合格条件）、冒頭の owner decisions 第 2 回（2026-10-29 を判断日として待つ、#86・#99 の branch に触らない）、§3.9（共有の機械には置かず、この Mac に置く）、§3.10（本番の予算はこの Mac の軽い qualification、規模は RTX 5090 で機材つき、数値を分ける）。計画の冒頭、critical path、§4 の 1（main の方式で先に作る代価を含む）、KP-6・KP-7・KP-7R |
| owner の問い 16（KP-2 の基準） | lead decision 2026-10-15 として記録した | 本書冒頭の 5、§3.4。計画 KP-2 |
| owner の問い 17（database・GPU・pin） | lead decision 2026-10-15 として記録した | 本書冒頭の 6、§3.2、§3.5、§3.9、§6。計画の冒頭と §4 |
| owner の問い 15・3・18（#85・#99 を待つか、配置と控え、ssh） | owner decisions 2026-10-15（第 2 回）として記録した。ssh の承認だけは引き続き待つ | 本書冒頭の 8〜11、§3.9（この Mac への配備: launchd、専用の利用者、macOS の PostgreSQL、CPU の e5-small、load 20 を守る上限の案、眠ったときの扱い、backup からの除外）、§3.10、§6。計画の冒頭、§1、§4 の 1、KP-6・KP-7・KP-7R |
| LOW 1（本書:64 の `discover_route.rs:76`） | 却下 | `ead6830e` で `MAX_GRAPH_HOPS` は 76 行目に在る（`git show ead6830e:crates/search-application/src/discover_route.rs` の 75 行目は `MAX_GRAPH_SEEDS`、76 行目が `MAX_GRAPH_HOPS = 3`）。本書の引用は正しい |
| LOW 3（計画:20・:24 を引く 02 の依存図） | KP の 3 文書は変えない | KP-3 の依存は KP-0 の承認だけで、KP-2 → KP-5 の辺は無い（計画 §1 の表）。直すのは AP の 02 で、fix-plan に伝えた |
| LOW 9（本書 §6 の 1「本文全体か要約か」） | 直した | AP が本文全体と決めている（AP の 03 §3.4 の段 3、§5.5）。本書 §6 の「決まったもの」へ移した |
| LOW 12（「合意」の主語） | 直した | 本書 §0 の 6・§4、計画 KP-3 の行と §3 の表に「担当の間の合意で、所有者の承認はまだ」と書いた |
| LOW 17（protocol を 03 の file 全体の SHA-256 で固定） | 直した | 本書 §4 は、AP の 03 の契約の範囲だけを決まった command で抜いた byte 列の SHA-256 で固定する。計画 KP-4 の写しも同じ値を書く |
| H1・H2・H4、M1〜M4・M6〜M9・M11・M12、LOW 2・4〜8・10・11・13〜16・18・19 | KP の 3 文書には掛からない | AP の 01〜04 で扱う（03 は fix-core、01・02・04 は fix-plan）。KP の受け手に影響するのは次の 3 つで、KP の作りは変わらない: 問い 7（agent の忘却は `disable` に落ち、確認と猶予の後に `forgotten` が来る。KP は 2 つを索引で同じに扱う）、問い 8（覚え直しは新しい id）、問い 5（本文 4 KiB のまま） |
| r2 LOW 6（問い合わせ API の綴りが固定値に入っていない） | 直した | 本書 §3.7 の 3 route の要求と応答の DTO（`search`・`status` の `lanes` を含む）と誤りの表を印 `query-api begin`・`end` の間に集め、`status` の欄の名前（`generation`・`lanes`・`ingest`・`purge`・`lastSucceededAt`）を決めた。§4 にその byte 列の SHA-256 と command を書き、印の外を直しても値は変わらないことと、そのときの版の上げ方も書いた。状況文書にも値を書いた |
| r2 LOW 12（Vector の lane の pin） | 直した | 本書 §3.7 の `search` の規則の pin の箇条、計画 KP-5 の試験 `memory_routes_create_no_evaluation_lease`。lease を作るのは評価の pin（`crates/search-runtime/src/pin.rs:252`、INSERT は `:326`）で、Vector の道の `pin_current`（`crates/search-application/src/vector.rs:415-418`）の PostgreSQL の実装は manifest を読むだけで lease を作らない（`crates/search-runtime/src/vector_store.rs:469-489`）。Memory の route は評価の pin を呼ばずにこの道を使う。pin を使うときも寿命の上限は 300 秒（`pin.rs:42`）で、消去の目標 30 分（§3.3）に収まる |
| r2 MEDIUM 1・2、LOW 1〜5・7〜11・13・14 | KP の 3 文書には掛からない | どれも AP の 01〜04 だけを引く（03 は fix-core、01・02・04 は fix-plan）。r2 の §1 の表で KP を引く行（H3・M5・M13）と §3 の release の条件の行は「直った」「そろっていた」の確かめで、直しは要らない |
| r2 §4 の owner に残る判断（2026-10-29 の選び直し、眠りを止めるか） | 変更なし | owner の判断待ちとして既に書いてある: 眠りは本書 §3.9 と計画 §4 の 5、2026-10-29 の選び直しは本書の冒頭と計画 §4 |
| r3 LOW 2（KP の記録が引く AP の commit） | 直した | 本書 §4 と状況文書に、固定値が同じなら AP の commit は最初の記録（`ee5dea94`）のままでよいと書いた |
| r3 M-1・LOW 1・LOW 3 | KP の 3 文書には掛からない | AP の 02・03 で直した |
