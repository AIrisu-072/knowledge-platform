# P7 Production Runtime — design/plan Freeze

Status: **FROZEN — design/plan only**（2026-10-01）。独立 architecture/security [revision 2 review](p7-runtime-architecture-review-revision-2.md) は、後述の exact input に対して **PASS / GO — design/plan Freeze に限る**と判定した。本書はその合成設計を固定する。production code、runtime 動作、P1〜P6 最終受入、P3 backend 採択、P2 Vector 採択、production readiness、SLO 保証、merge、本番 migration、live deployment の資格・実施を示さない。

## 1. Authority と exact provenance

`spec/` が規範正本であり、[P7 shared durable freeze](p7-shared-durable-freeze.md)、各 P1〜P6 の既存 freeze/owner 境界を維持する。以下は `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未 commit 作業木で SHA-256 を再照合した、今回の直接入力である。

| 直接入力 | SHA-256 | 役割 |
| --- | --- | --- |
| [p7-runtime-design-revision-2.md](p7-runtime-design-revision-2.md) | `415d5a1648702670eb5737eecfe16581b7565191f2e64409c143236a00e95461` | host publisher と typed Audit producer の最終設計差分 |
| [p7-runtime-plan-revision-2.md](p7-runtime-plan-revision-2.md) | `336894fcfaba7adf2e67474b56357ddfa7e9ad4502e1a0206616cb4743d9cb6b` | sole writer、順序、負試験の最終計画差分 |
| [p7-runtime-architecture-review-revision-2.md](p7-runtime-architecture-review-revision-2.md) | `1070a12609b408c655b03ef0da4e20b819093dca7befb8691941953b36a46172` | 独立 design/plan GO、W1/W2 |

合成元の内容同一性も固定する。元[設計](p7-runtime-design-completion.md) `df0f2ba4e91526a24567addfac3777a7d7219ae296d5a3dc4b0dcc0c51737f3e`、元[計画](p7-runtime-plan-proposal.md) `acecf896ad5481955b805728bb8c0d970e154657e1a4d5a89f7c100f3bc0ed3a`、初回[NO-GO review](p7-runtime-architecture-review.md) `0bf9ce336e664a3eb4a5e14c7f37ec9a351bc4edbfa9d6b8b6435f002088f793`、[設計 revision 1](p7-runtime-design-revision-1.md) `422e00dcb0c685feb2fc9f81755e25bdd555d2e4b7602ccadb85e4c281a9d035`、[計画 revision 1](p7-runtime-plan-revision-1.md) `d24cba464e40f9d07d2baf9095d4ece1bb099f0f8b06cfaca6eee2e2982e3d57`、[revision 1 review](p7-runtime-architecture-review-revision-1.md) `a6f91b67ecfd220191486aa2cdd6715967a8115f9e0b9dc6aee8ecaee6c19e29`。共有基盤の[freeze](p7-shared-durable-freeze.md) `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110` と[plan](p7-shared-durable-plan.md) `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517` を消費する。

同じ対象で抵触するときは revision 2 → revision 1 → 元設計/計画の順に適用する。revision 2 は二つの残存指摘の差分であり、revision 1 と元文書の非抵触条件を取り消さない。[bounded implementation plan](p7-runtime-plan.md) はこの Freeze の実行順・受入条件を記す。

## 2. Frozen runtime contract

1. **単一 composition と既存 owner。** 一つの Rust runtime root が P7-01〜12 の同一 PostgreSQL Source/current/READY/pin/guard と Search receipt、P1 Document/Unit/lexical、条件付き P3 Graph、P4 Remote RAM evaluation、P5 四 route/二 lease、P6 generic delivery を接続する。P7-Rxx は accepted producer を消費し、第二 Source pointer、Source/actor mint、route handler、generic Domain ack、Search receipt、Graph READY を再実装しない。P1〜P6 final receipt は R09 のみが fan-in する。
2. **Host authority と publisher。** R01 の trusted operator `HostRegistrationInputV1` は登録集合から独立した全 tenant roster（登録 0 件 tenant を含む）、Document/Remote の全 DTO、epoch、単調 revision、宣言件数を提供する唯一の authoring input とする。提供不能なら fail closed。R01P は Search `0004_host_registration_inventory_v1.sql` の独立 inventory と current head を唯一 publish する。immutable 全 row、tenant/namespace digest、旧 head 条件付き CAS、`host.registration.changed` の typed Audit row を一つの PostgreSQL transaction で確定する。partial、衝突、stale、Audit INSERT failure、unknown commit では公開・claim を閉じ、別接続の再読で確定する。credential 値は inventory に保存しない。
3. **Reader と起動 admission。** R02 は publisher の read-only adapter として同じ read transaction の head/row を読み、host authoring revision/roster と別接続で照合する。Document/Remote の `CompleteDesiredRegistrations::capture(...).await` を同一 authority に束ね、P7-02 唯一の `PgPool` ledger と `SourceRegistrationCatalog::try_new(...).await` を使用する。host head、ledger/owner/kind/activation/current、P7-12 scan の再照合が終わるまで P5 listener と P6 claim は閉じる。reload は全検証後の一回の切替に限る。
4. **Typed Audit と transaction owner。** 既存 Document `audit_outbox_events` の UUID/subject 制約を保ち、Domain `0010_audit_delivery_v0.sql` に別の append-only `search_audit_outbox_events` と versioned policy を追加する。R04A-S の `append_search_audit_on(&mut PgConnection, TypedSearchAuditEvent)` を、R01P host CAS、R04A-C policy command、R05 管理/admission/result・隔離、P5 denial、Document の各既存 owner が自分の transaction 境界で呼ぶ。Search/system subject を Document `AccessPolicy`/nil UUID に偽装しない。Audit INSERT failure は対応する mutation を rollback し、拒否記録 failure でも拒否を維持する。外部作用は durable admission＋Audit commit の後だけ実施し、結果記録失敗を成功扱いしない。R04A-D は Document/Search 両 Audit source を origin-tagged decoder と別 role/fence/retry/DLQ で別 PostgreSQL sink に配送する。P6 Domain `delivered_at`、Search receipt、各 Audit ack は独立である。
5. **保持、telemetry、測定。** revision 1 で設計上 CLOSED の三件を維持する。R04O は `SinkKind × RetentionMode × VisibilityClass` の closed allowlist と default deny、P4/P5 二 lease、実 socket 送出、全 sink/exporter/保持 handle の `NoRetention` sentinel を要求する。R04P は HTTP/protobuf 対 gRPC を隔離 PoC で比較し、独立採択・規範 exact pin 前に `POC REQUIRED` transport を production Cargo/R06 image に入れない。R07 は immutable workload manifest、bounded pilot、資源 pre-admission/途中停止、未測定 `NOT_ADMITTED` を要求し、R08 は対応する実測範囲だけの operational SLO *提案*を作る。
6. **資格境界。** P3-P04 backend selection は未採択であり、Graph/P7 READY・publish と R02/R09 production acceptance は閉じる。非 PostgreSQL Graph 採択時は同等 publication/fence の別設計と独立 review が先に要る。P2 Vector/runtime selection は実測・独立判定まで閉じる。`Disabled` を選ぶ場合も neutral core、lexical、Graph/current/final access の gate は省かない。host identity、実 socket、実 role、P6 unknown COMMIT/fenced ack、P7-12 別 DB restore、OTLP 採択、capacity/SLO は各実装・資格 receipt で別に証明する。

## 3. Review closure と実装時 gate

初回 review の五件のうち、sink 別 `NoRetention`、OTLP transport PoC、workload/budget の三件は revision 1 review で**設計上 CLOSED**。host inventory publisher と typed Search/system Audit producer の二件は revision 2 review で**設計上 CLOSED**。これは named test、実 role、runtime、実測の PASS を意味しない。

- **W1（実装受入）:** `spec/operations/observability-audit-requirements-v0.md` §14.2/§15 に従い、必須 `document.version.read_confirmed` を Document producer の class 表と R04A-S の legacy decoder/policy に明記する。既存 `crates/document-repository-postgres/src/read_state.rs` の同一 transaction INSERTを Document owner が実 DB で照合し、この class 固有の Audit INSERT failure で初回既読 state も rollback する負試験を実施する。R04A の配送試験だけで producer 済みと数えない。
- **W2（fresh bootstrap 受入）:** Domain `0009` → Search `0001`〜`0003` → Domain `0010` → Search `0004` の段階順、両 migration ledger の checksum と実接続 role/grant gate を、全 migration file が存在する fresh disposable DB で R03/R09 が検証する。現行 `search-runtime::migrate` の単一 `sqlx::migrate!("./migrations")` だけを順序の証拠にしない。欠落、逆順、checksum 不一致、権限過大では listener/claim/readiness を開かない。

W1/W2 はこの design/plan Freeze の blocker ではなく、実装と fresh-bootstrap の未達 gate である。次の exact action は [plan](p7-runtime-plan.md) の依存順に R04A-S と P7-03 accepted schema/role を満たし、R01P 実 publisher、R02 実 adapter、各 producer、R03/R09 fresh bootstrap を focused RED→GREEN と独立 read-only review で証明すること。Graph/Vector 選定と P1〜P6 最終 receipt を先取りせず、R09 の final gate に合流させる。
