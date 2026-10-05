# P3-G01 登録参照・read lease interface 改訂 1 の独立再審査

**判定: GO — G01 の SQLx-free 公開型・port を pre-code 実装入力にできる。** 対象は `p3-g01-issuer-revision-1.md` と `p3-g01-test-revision-1.md` の interface/test 契約だけである。Graph backend は未選定で、P7 registration/READY/pin、DB role/trigger、G01 compile RED/GREEN、production runtime の実装・受入は未検証である。

## 先行 NO-GO の閉鎖

| 先行指摘 | 再審査結果 |
| --- | --- |
| F1: public host trait から偽 handle を発行できる | **閉鎖。** `TrustedGraphRegistrationHostPort` と issuer 群を production G01 API から外し、`from_identifiers` を誰でも呼べる識別値 constructor と明記した。`RegisteredFullBuildHandle` / `BuildGuardHandle` の private field は authority でなく、保存 P7 target・Graph parent・guard・実 DB role・DB clock が child DML admission になる (`p3-g01-issuer-revision-1.md:7-11,18-45`)。fake full/incremental ref と直接 SQL を実 role で拒む named case がある (`p3-g01-test-revision-1.md:25-33,43-44`)。 |
| F2: full/incremental/read 発行値と検証層が未確定 | **G01 interface として閉鎖。** full は key/token/fence、incremental は base/target key/token/fence、read は key/evaluation/lease ID を別型で固定し、P7 private handle と区別した (`p3-g01-issuer-revision-1.md:7,18-57`)。同一接続登録、毎 batch の保存 row/kind/snapshot/manifest/activation/guard/expiry 再検証、read 前・返却前の lease と現在 actor/Source 再検証を要求する (`:9-11,123-125`)。具体的な実 DB 証明は後続 gate に残る。 |
| F3: synthetic test が偽権限・誤分類を見逃す | **計画として閉鎖。** 外部 integration crate による全 port の positive compile、公開 constructor の偽造可能性、旧 authority API/自律 stage/READY の negative compile を分けた (`p3-g01-test-revision-1.md:5-20`)。full、incremental、read、retention、host error、role/trigger、期限境界を named 実 PG case に割り付け、fake adapter 成功を admission と数えない (`:23-62`)。これらは未実行。 |
| F4: mapping receipt と non-READY report が曖昧 | **閉鎖。** `GraphStageReport` と `GraphSourceMappingReceipt` は公開・偽造可能な比較データで、`validate_staged` は READY に遷移せず、`recover_ready` は保存 READY の読取契約になった (`p3-g01-issuer-revision-1.md:64-99,119-121`)。Source-owned mapping と保存 row の再計算、owner/kind/native ID、temporal/n-ary closure の負例を後続 gate にした (`p3-g01-test-revision-1.md:45-48`)。 |

## 境界の確認

- **書込みと expiry:** EVENT/MANUAL の登録、full/incremental guard と Graph parent は P7/P3 の一接続 transaction に属する。通常 builder に parent/guard INSERT を渡さず、各 batch と commit 直前に DB role・保存 binding・`expires_at > clock_timestamp()` を検査し、失敗時は子 DML を含め rollback する設計で一致する (`p3-g01-issuer-revision-1.md:9-11`; `p7-shared-durable-plan.md:125-132,152-170`)。G01 参照と stage report は READY/publish の証拠にならず、P7-08 が同じ接続で P1/P3 実体を再検証する (`p3-g01-issuer-revision-1.md:11`; `p7-shared-durable-plan.md:172-179`)。
- **読取りと Source scope:** `retrieve_pinned` は既存 `TrustedDiscoveryBinding` と `AuthorizedSourceScope` を受け、production wrapper が concrete verifier と現在 Source access port を固定する。保存 lease、tenant/actor/session/evaluation、revision/activation、ownership、P4/P5 current gate、Document current access を読取前と返却前に再判定し、失敗時は全結果を閉じる (`p3-g01-issuer-revision-1.md:101-115,123-125`; `p3-g01-test-revision-1.md:50-62`)。P4 の構造的不一致は ID-free `OperationFailed("trusted scope unavailable")` である (`p4-source-neutral-code-recheck.md:6-9`)。第二の actor/Source mint はない。
- **error 分類:** 公開 plan/budget/limit の形式だけ `InvalidRequest`、現在 Source/retention/access の不可用は `SourceUnavailable`、保存 target/guard/lease の stale・expiry は `FenceLost`、内部 row/role/shape/closure 不整合は `OperationFailed`、commit 応答不明は `CompletionUnknown` と分け、host の `SearchError` は variant を維持する (`p3-g01-issuer-revision-1.md:127-138`; `p3-g01-test-revision-1.md:29-37,62`)。現行 enum に各 variant が存在する (`crates/search-application/src/error.rs:4-14`)。
- **compile gate:** `BoxFuture<'a,T>` は既存 App alias と同じ `Result<T,SearchError>` 一重包みであり (`crates/search-application/src/ports.rs:32`)、test proposal は外部 crate から全 signature を型検査する。`search-application` に SQLx/PgPool/PgConnection を export しない gate と、実 PostgreSQL の authority gate は明確に別である (`p3-g01-test-revision-1.md:5-21,64-66`)。

## 実装への引継ぎ条件と残余リスク

1. 旧 `p3-graph-plan.md:101,130,157` の自律 `stage_full` / `validate_ready` と caller が verifier を渡す reader signature は、G01 production 実装に使わない。改訂案はこれを明示的に置き換え (`p3-g01-issuer-revision-1.md:3`)、P7 計画も一接続登録・READY・固定 verifier を要求する (`p7-shared-durable-plan.md:167-179,190-197`)。G04/G07 の task handoff では旧 signature を改訂契約へ書き換えてから RED に入る。
2. `p3-g01-issuer-revision-1.md:11` の「現在の Source `build_fence_seq`」確認と Source-less batch の「後から Source lock を取らない」は、G06/P7-07 実装時に比較対象と競合時の意味を固定する必要がある。保存 target/guard の token/fence 照合と Source row の最新 seq との等値比較を混同しない。凍結 guard は pointer 前進後も有効であり、batch の実 DB 競合試験で証明する (`p3-graph-plan.md:148-151,175-178`; `p3-g01-test-revision-1.md:43-45`)。これは G01 公開型の GO を拡張するものではない。
3. `compile_fail` は無関係な構文・import error でも通り得る。G01 GREEN の receipt では positive compile と各 negative case の意図した E0432/E0599/E0451 等を個別に確認する。実 role・独立接続・DB clock の named case は backend 選定と schema 実装後に実測し、G01 compile 成功をその代用にしない (`p3-g01-test-revision-1.md:11-16,64-66`)。

## 入力固定と実行境界

`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の dirty worktree で、指定された次の **9 入力**を開始時と終了時に SHA-256 照合した。値は開始 = 終了。

| 入力 (`docs/superpowers/programs/search-platform-completion/` 以下) | SHA-256 |
| --- | --- |
| `p3-g01-issuer-revision-1.md` | `ece9eea1c87ba3377ff75fbb1cd57560afb8571cdb9784b3eb8b797c724fc5e1` |
| `p3-g01-test-revision-1.md` | `9ee1f18dbf280f34abc17ee26105ab7a75d3bf83a454c728dc0136e8d85978ea` |
| `p3-g01-issuer-review.md` | `9154017020fa1f47e91be0558bbdcf9ec2756abc1f0e7d6c576f0d373230f1d1` |
| `p3-graph-freeze.md` | `773be941a35a2984495614ea41db74f70d325f4c034fe576d4d106b4af0e6ca3` |
| `p3-graph-plan.md` | `cc6a6ddee5004c1da419f3d95965a98f71a4b8ea2e081a5d9a36267203e9c86f` |
| `p7-shared-durable-freeze.md` | `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110` |
| `p7-shared-durable-plan.md` | `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517` |
| `p4-source-neutral-code-fix.md` | `7295deb04113581562a386893b07c76c63aa183119b9479cf344660a99e80ef5` |
| `p4-source-neutral-code-recheck.md` | `6daaec0c018bb7776525fb0292848f1d51f7917639af114b722b07486f033665` |

この監査は文書・現行型の read-only 照合だけを行った。`graph_generation.rs` / `graph_generation_port.rs` は存在せず、G01 の compile RED/GREEN は未実行。空き容量は `df -h .` で約 1.2 GiB、記録された 1.5 GiB STOP floor 未満のため Cargo/build、DB fixture、PoC は実行していない (`docs/superpowers/execution/active.md:9`; `search-platform-completion-program-status.md:174`)。GO は設計入力の判定であり、compile gate は G01 実装後に必須である。
