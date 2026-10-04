# P3 Durable HyperGraph — 改訂 1 独立 architecture 再審査

- 判定: **NO-GO for design freeze**。既報 P2-2〜5 は設計上閉じた。P2-1 の READY mutation / publish / evaluation pin / GC の競合は解消されたが、改訂が導入した incremental build の base / target と GC の競合が 1 件残る。下記の具体的な build guard を定義して再確認すればよい。追加の人手承認 gate ではない。
- 対象: `p3-graph-design-revision-1.md` SHA-256 `ba9e8616d9dd8fc569280f20956c841929155366fa30246dadee0f0c73e8aa59`、既報 `p3-graph-architecture-review.md`、Search v0 Approved Design §22/24/34/54/62、`spec/data/transaction-consistency-requirements-v0.md` SD-T1/SD-T6、関係する現行 port / Document adapter / memory oracle。確認時 HEAD は `f348466b79928c3c96e6349b4976e4e58d25a576`。静的な設計・コード照合であり、PoC・migration・実 DB 並行試験・backend 選定の結果ではない。

## 既報 5 件の再判定

| 指摘 | 再判定と根拠 |
| --- | --- |
| P2-1 READY / publish / pin / GC | **一部未了**。改訂 §4（44–48行）は子表 DML の親 generation lock、BUILDING 限定、同一 transaction の `validate_ready`、READY の DB 側不変 fence を定義した。§5（52–66行）は source control → generation → lease の lock 順、同一 DB transaction の pointer CAS / pin / GC、失効時の Query と bounded retry を定義した。PostgreSQL の [`FOR UPDATE` row lock](https://www.postgresql.org/docs/current/explicit-locking.html) と整合する。ただし下記の incremental build guard が未定義。 |
| P2-2 owner / current access | **閉鎖**。改訂 §3/§6（32、38、72–97行）は `GenerationScopedGraphAccessPort::evaluate` から caller owner を除き、保存済み row の kind / owner と Source snapshot の canonical mapping を stage / validate / recover で照合する。Document / FolderPlacement の一対一 owner、Document 固有の FolderPlacement ID、Knowledge/Version の直接評価が、現行 `validate_graph_ownership`（`crates/search-source-document/src/outbox.rs:549`）、`document_resource_id` / `folder_resource_id`（`relations.rs:147`）、`DocumentCurrentAccessAdapter`（`postgres.rs:424,444,532`）と一致する。cross-Source 同一裸 ID fixture では Source 別 evaluator を使う。 |
| P2-3 temporal roundtrip | **閉鎖**。改訂 §3（36–38行）は `TemporalProjection` の valid/effective 四境界、freshness anchor / basis の全 field、NULL、nanosecond、offset と半開区間を lossless な SQL 表現と canonical digest に含めた。対象型は `crates/search-core/src/projection.rs:100` / `temporal.rs:12`。実 roundtrip は PoC 待ち。 |
| P2-4 relation-only closure | **閉鎖**。改訂 §7（101–105行）は resource 不変の relation add / same-ID update / delete、ID 変更を旧 relation / participant 全削除→replacement 挿入で扱い、authoritative closure proof がない Source は full rebuild とした。SD-T1 / SD-T6 の full と incremental の論理等価を受入に残す。 |
| P2-5 output / timing | **閉鎖**。改訂 §8（109–119行）は正常な generation と現在 Source access の範囲で欠落 / 未認可 seed の公開 response class / count / path / evidence を揃え、hidden degree 起因の request-time cap / timeout が異なる公開値にならない制約を置いた。完全な latency distribution noninterference は主張せず、反復観測の残余リスクを PoC と security review へ送る。この限定は Approved Design §62 の「存在を漏らさない構成を可能にする」という契約を超えて保証を捏造しない。運用 timeout / 障害の latency / error 差も残余 side channel として明記した。 |

## 残る blocking finding

### [P2] `stage_incremental` の base READY と active BUILDING が GC から保護されない

改訂 §7（101–105行）は `base_key` の READY row を新 BUILDING generation へ複製し、旧 relation 集合を closure proof と照合する。一方 §4（44行）は batch ごとに **target** generation だけを lock し、§5（60行）の `retire_unpinned` / `discard_unpublished` は current と有効な **evaluation** lease だけで削除可否を決める。§4（48行）は BUILDING の削除も認める。`stage_incremental` の base と target を build 全期間保持する契約がない。

例: base A の複製を一部 commit → 別の publisher が C を current にする → A の evaluation lease がなく GC が A を削除 → 次 batch が A の残りを読めない。target BUILDING も batch 間に cleanup され得る。READY の不変性と各 batch の target lock だけでは、複製元の全行と old relation ID 集合を一つの base digest に固定できない。欠落を必ず検出する契約もなく、同一 snapshot の full / incremental 等価（SD-T1/SD-T6）を設計から保証できない。

**必要な修正 1 件:** P7 接続後に GC を有効にする場合、evaluation lease と別の durable **build guard** を `base_key` と target BUILDING key に束縛する。最初の copy batch 前に source control → generation key 順 → guard row の順で登録し、base の READY / digest / manifest を確認する。各 copy batch は guard の有効性を確認し、base row の `FOR SHARE` と target row の `FOR UPDATE` を key 順に transaction 終了まで保持する。copy 完了時に target の複製部分の count / content digest と base receipt を照合する。delta batch も target と guard の有効性を確認する。`retire_unpinned` / `discard_unpublished` / BUILDING cleanup は有効 guard の付いた両 key を削除しない。publish または明示 abort / discard 後に guard を解除し、crash 後は期限切れ guard を bounded recovery で処理する。guard を登録・更新できない base は incremental を中止して full rebuild に戻す。§4–5/§7 と `publish→GC` / active `BUILDING cleanup` の実 DB 故障注入にこの契約を追記する。P7 未接続の isolated P3 では現行 §5（62行）どおり GC を公開しない。

## 次の exact action と境界

上記 build guard を改訂設計に反映し、この一点を再審査する。その後に意味論と選定規則の freeze へ進む。PostgreSQL は暫定第一候補のまま。隔離 PoC、DB role / trigger / lock の実証、temporal / owner / relation parity、timing probe、製品選定、Document runtime 接続、P7 multi-index E2E、exact-head CI は未実施であり、ここでは達成を認定しない。
