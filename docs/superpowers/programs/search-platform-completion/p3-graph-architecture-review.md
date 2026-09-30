# P3 Durable HyperGraph — 独立 architecture review

- 判定: **NO-GO for design freeze**。P1 は見つからず、下記 P2 を設計本文へ反映して再確認する。これは人の追加承認 gate ではない。
- 対象: `p3-graph-design.md`（2026-09-30 DRAFT）、`spec/data/transaction-consistency-requirements-v0.md` SD-T1〜T6、Search v0 design §22〜27/34/54/62、現行 `search-core`／`search-graph-memory`／`search-source-document` の契約。作業時 HEAD は `a877f447dcdc767be2a2e328886420fb4e7e8337`。この review は読取り調査であり、PoC・DB 実測・実装 gate の結果ではない。

## Blocking findings

### P2-1 — READY 不変性と pin/GC の直列化契約が不足

設計 §5 [77–79](p3-graph-design.md#L77) は Graph READY の検証、pointer CAS、lease、GC を別操作として述べるが、`DurableGraphGenerationPort` [61–67](p3-graph-design.md#L61) は同一 PostgreSQL transaction に入れる境界、ロック順、失敗時 retry を定義していない。物理 schema [40–43](p3-graph-design.md#L40) にも BUILDING 行だけを書ける制約がない。`validate_ready` の count/digest 計算と READY 遷移の間に stage が書けば、検証済み digest と実 row がずれる。pointer と lease を読み取った後の GC、又は pin と退役が競合すると旧 generation を pin した Query が削除済み artifact を参照できる。PostgreSQL の既定 Read Committed では同一 transaction の連続 SELECT でも異なる snapshot を見得る（[公式 isolation](https://www.postgresql.org/docs/current/transaction-iso.html)）。

**修正条件:** P3 repository と P7 coordinator が共有する transaction/lock 契約を定義する。stage は BUILDING のみ変更可、READY の resource/relation/participant/owner は DB 境界で変更拒否、`validate_ready` は同じ generation lock 内で検証して状態遷移する。CAS は READY/digest を同一 transaction で確認する。`pin_current` の key 選択と lease 登録、`retire_unpinned` の pointer/lease 確認と削除を共通の Source/key lock 順で直列化し、失効時の Query と serialization retry を規定する。SQLx 型は Domain/Application へ出さず、実装 adapter 側で transaction を所有する。並行 stage/validate、pin/GC、CAS 敗北、crash の実 DB 試験を受入に追加する。

### P2-2 — current access の owner key が trusted row に束縛されない

提案 port [54–70](p3-graph-design.md#L54) は `access_subject_ref: Option<OpaqueSourceKey>` を呼出し側から `evaluate` へ渡す。一方、schema [41](p3-graph-design.md#L41) は ResourceKind/owner 必須条件を持たず、Document/FolderPlacement の owner 欠落・取り違えを READY 時に判定できる契約がない。現行 `validate_graph_ownership` は **Document と FolderPlacement の全件に一対一の owner** を要求し（`crates/search-source-document/src/outbox.rs:549–576`）、`DocumentGraphAccessReader` はその owner を現在の Document Read に渡す（同 `:202–225`）。異なる DocumentId を渡せば権限を持つ別文書の判断を誤用し得る。

**修正条件:** `GenerationScopedGraphAccessPort::evaluate` は `(source,generation,resource)` から backend が読み出した owner のみを使う形にするか、渡された owner と当該永続 row の一致を port 内で強制する。Document/FolderPlacement の種類と owner の必須性、Version の直接評価を stage/recover で検証し、Source snapshot の canonical ID 対応とも照合する。owner は graph digest に含め、欠落・不一致・Source adapter 不在は不可視として fail closed。複数 Source に同じ裸 ResourceId がある fixture では、現行 memory oracle の `CurrentAccessEvaluatorPort::evaluate(ResourceId, ...)` が Source を受けない点（`crates/search-graph-memory/src/traversal.rs:54–61`）を補う Source 別 access evaluator で比較する。

### P2-3 — resource temporal の port と物理表現が一致しない

`GraphResourceRecord.temporal: TemporalProjection` [54–55](p3-graph-design.md#L54) は `TemporalDiscoveryProfile` 全体を含み、`freshness_anchor_at` と `freshness_basis` も持つ（`crates/search-core/src/projection.rs:99–105`、`crates/search-core/src/temporal.rs:12–17`）。提案 `resource` row [41](p3-graph-design.md#L41) は valid/effective の四境界しか持たない。他方、§5 [73](p3-graph-design.md#L73) は「sorted resource temporal」を content digest に含める。このままでは port の値を再起動後に往復できず、full/incremental digest parity の対象も曖昧になる。

**修正条件:** Graph 用 port 型を oracle が実際に評価する valid/effective 四境界へ縮め、digest も同じ四境界と明記する、又は freshness の二 field を lossless に永続化する。`None`、時刻精度、半開区間、full/incremental/recover の roundtrip を同じ canonical encoding で試験する。

### P2-4 — incremental closure が relation-only 変更を定義していない

`GraphIncrementalDelta` [58–59](p3-graph-design.md#L58) と §5 [75](p3-graph-design.md#L75) は changed/retired resource に接する旧 relation の除去を定義するが、Resource 自体が同じまま relation の qualifier、authority、provenance、temporal、evidence、participant 集合が変わる場合を明示しない。旧 relation_id がコピーされ、同 ID の replacement と衝突するか、旧 row が残る。Source contract は full/incremental の論理等価を要求する（`spec/data/transaction-consistency-requirements-v0.md` SD-T1/SD-T6）。

**修正条件:** relation-only add/update/delete も影響 closure に含め、旧 relation ID の retirement と replacement の適用順、ID が同じ内容変更の扱いを定義する。Source が complete authoritative relation delta を証明できないときは full rebuild。resource 不変の relation 更新・削除を full rebuild と digest/path parity で試験する。

### P2-5 — timing 非漏洩の絶対保証に実装条件がない

§2 [18](p3-graph-design.md#L18) は unauthorized Resource の存在を timing からも漏らさないとするが、§4 [47](p3-graph-design.md#L47) の incidence scan と各 participant の current access 評価は、隠れた relation 数や high-degree に応じて処理時間が変わる。現行 oracle も relation ごと・participant ごとに認可を呼ぶ（`crates/search-graph-memory/src/traversal.rs:107–133`）。そのままでは絶対的な timing noninterference を満たす設計とは言えない。

**修正条件:** 公開出力・error・count で存在を漏らさない保証と、観測可能な timing の threat model／許容値を分ける。timing も厳密保証するなら bounded/padded execution 等の具体策と測定 gate を追加する。保証を限定するなら §2 の文言を検証可能な範囲に修正し、残る side channel を security review に記録する。branch/path budget は不可視 relation を可視件数として数えないまま維持する。

## 整合している境界

- `TypedRelationInstance` 一件と同一 generation の全 participant を一体で保持する方針は、`spec/data/logical-data-model-v0.md` の canonical n-ary 契約、`GraphTraversalPlan::allows`、memory oracle の incidence と整合する。relation_id をまたぐ合成を禁止し、現在 access を全 participant に掛ける点も妥当。
- `NO_RETENTION`/`SESSION_ONLY` を durable store から除外し、Graph path を Primary evidence に昇格しない境界は規範と整合する。
- PostgreSQL を暫定第一候補にし、redb/Neo4j と同一 fixture で測る順序は妥当。製品採用と SLO は未決定のままでよい。[redb 開発元](https://github.com/cberner/redb) は ACID/MVCC と MIT/Apache-2.0 を示し、[Neo4j 公式](https://neo4j.com/open-source-project/) は Community GPLv3、[同 backup guide](https://neo4j.com/docs/operations-manual/current/backup-restore/) は online backup を Enterprise 側に置く。これらは比較条件であり測定結果ではない。
- P3 単体の durable backend と P7 の multi-index publish/pin/recovery は区別されている。上記 transaction 境界を明文化した後も、P7 接続前に E2E 稼働を主張しない。

## 次の exact action

設計担当が P2-1〜5 の修正条件を `p3-graph-design.md` へ反映し、この review の再確認を受ける。その後、意味論と PoC 選定規則だけを freeze し、実測 PoC と backend 選定 receipt を経て production dependency/実装へ進む。人の中間承認は追加しない。
