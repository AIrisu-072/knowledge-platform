# P5 Common Search / Discovery API — 独立アーキテクチャレビュー

- 判定: **方向性は妥当。現稿のまま Design Freeze は NO-GO**。下記 P1/P2 を設計契約に反映し、P1〜P4 の凍結結果と突き合わせた後に再判定する。
- 対象: `p5-api-design.md` の早期設計、承認済み Search Discovery v0 design §§4, 41, 62–64、関連する `spec/`、現行 Search ports、別 ref `origin/feat/document-http-openapi-transport-v0-d@3f870a92525afb6741e1ee72ee6c932eac0f0511`。コード、OpenAPI、実 HTTP の実装完了判定ではない。
- 独立確認: Human / LLM / Agent が同一 Search Application を呼び、`InvocationKind` を権限にしない構成、HTTP 入力から principal / role / group を作らない方針、Search と Discovery の区別、live/current と history の分離、Graph 専用公開 API を作らない方針は既存契約に整合する（`p5-api-design.md:23-41,49-60,102-104`、`docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md:129-157,2102-2139`）。HTTP ref の `IdentityAdapter` は header を認証済み claim と扱わず、adapter 未設定の route 起動を拒否している。ただし別 ref の実装を P5 の接続済み証拠とは扱わない。

## Freeze 前に直す指摘

### [P1] trusted tenant と Source 存在可視性を四 operation 共通の入口へ入れる

`p5-api-design.md:37-41,56,68-74` は `SourceBrowseService` の可視性フィルタを明記する一方、Search / Discover の routing と `ResourceLocatorPort` の lookup を actor・tenant にどう限定するかを固定していない。現行 `DiscoveryService::discover` は actor を受けない `SourceRegistryPort::list_sources()` をそのまま `SourceRouter::plan` に渡し（`crates/search-application/src/discovery_service.rs:167-179`、`crates/search-application/src/ports.rs:39-42`）、route gap / trace に Source ID と状態を入れる（`discovery_service.rs:202-208,814-839`）。候補の current access だけでは、隠れた Source の routing、障害、件数、coverage の差を伏せられない。P4 draft は `(tenant_id, source_id)` を境界とし、P5 の trusted transport が `TenantId` を作る前提である（`p4-remote-design.md:39-43,83-89`）。P5 に tenant の供給元・型・lifetime がない。

**必要な修正:** 認証済み resolver から内部 `TrustedTenantContext` と `VerifiedActorContext` を一組で作り、request-scoped Source registry / visibility を **routing・pin・remote call より前**に適用する。`SearchQueryService` と `DiscoveryService` はこの actor-scoped Source 集合だけを受け、Source visibility と item/field authorization を開示直前に再評価する。`ResourceLocatorPort` は tenant-scoped lookup とし、ID の global uniqueness だけに依存しない。未知・別 tenant・不可視の Source/Resource は、`sourceIds`、GET、gap、`partial`、trace、timing-sensitive error class で識別させない。重複 mapping は内部 integrity incident として記録し、未認可 caller へだけ異なる 500 を出して存在 oracle にしない。対象 port が無ければ四 operation とも起動を拒否する。二 tenant、隠れた障害 Source、重複 locator、`sourceIds` を使う反例を受入試験に加える。

### [P1] 署名付き cursor の秘匿性と NO_RETENTION の境界が未確定

`p5-api-design.md:94` は「署名付き opaque token」に principal/subjects fingerprint、可視 Source 集合、generation、fusion last key を束縛する。payload を持つ token は署名だけでは秘匿されず、Source ID、candidate key、principal の派生情報を client に開示し得る。`p5-api-design.md:104` の禁止は body / snippet / evidence に限定されるが、P4 draft の NO_RETENTION は候補 ID、query/result trace、per-call telemetry まで call 後の保持を禁じる（`p4-remote-design.md:91-103`）。P4 の内部 remote cursor は外部 Search client へ渡さない前提でもある（同 `:83`）。

**必要な修正:** `opaque` の実体を「機密性を満たす暗号化 token」または「許可された retention だけを持つ server-side random handle」と定義し、署名のみの可読 payload を排除する。token/handle に入れる field の retention matrix を P4 と照合する。NO_RETENTION の snapshot/candidate state は request をまたぐ公開 cursor に載せたり保存したりしない。SESSION_ONLY の場合も、同一 session と期限へ束縛した RAM handle 以外へ保存しない。継続資格がない Source が一つでも結果の完全性に必要なら `nextCursor` を発行せず、`partial` と型付き理由を返す。decoded token、log、cache、trace、期限切れ後の state を検査する negative test を追加する。

### [P2] Claim ID の解決は generation pin だけでなく actor visibility が必要

`p5-api-design.md:58` は public `requiredClaimIds` を trusted `ClaimSelectorPort` から引くが、現行 port は `(generation, claim_id)` だけを受け、actor/tenant/current policy を受けない（`crates/search-application/src/ports.rs:53-68`）。Document catalog も pinned projection の存在と authoritative assertion を見るだけで caller visibility は判定しない（`crates/search-source-document/src/evidence.rs:122-169`）。候補認可後に evidence を落とす設計であっても、Claim 存在・selector、route、validation/gap の差が事前 oracle になり得る。

**必要な修正:** Claim catalog の公開 ID に tenant/source scope と actor-visible 解決を定義し、selector の subject、predicate、expected value を current access 前に外へ出さない。未知・不可視・別 tenant Claim ID の外部結果を同一にし、必要 Claim が不可視のときは `Sufficient` を返さない。P1/P4 の claim catalog freeze 後に `validation` と `Unresolved` のどちらへ統一するか決め、認可失効中の Claim を negative test にする。

### [P2] 最終開示時の再認可と Source 一覧 cursor の取消 semantics を確定する

`p5-api-design.md:64-72,94-96,120` は current access の再評価を求めるが、title/snippet/evidence を取得して safe DTO に写す **後**の field-specific check と、認可失効時にその DTO 全体を破棄する境界が曖昧である。現行 Document adapter は Version と access revision を check の前後で比較する（`crates/search-source-document/src/postgres.rs:532-582`）。この保証を HTTP の出力組立まで維持する必要がある。また SourcePage cursor は actor fingerprint と registry revision に束縛するだけなので、registry は不変でも Source visibility が変わると、新たに見える小さい key を飛ばして完了と誤認し得る。

**必要な修正:** Search / Discover / GET の各公開 item と evidence/citation/snippet に、Source が承認した field、同一 generation、current Version / Source policy / actor validity を出力確定直前に検査する gate を置く。check 中に失効を観測したら item とそこから派生した evidence、rank、count、trace をまとめて除外する。SourcePage cursor は visibility revision または可視集合 digest に束縛し、変化を検知したら `CURSOR_STALE` で再開始させる。検知不能なら完全な継続を主張しない。取消と再付与が page 間・snippet/evidence read 中に起きる試験で確認する。チェック後の将来の取消まで原子的に保証する、という実現不能な主張は置かない。

### [P2] HTTP deadline / partial と Problem・OpenAPI の規範表現を一つにする

`p5-api-design.md:96,108,115-118` は高価な fan-out と timeout を認識するが、operation deadline を Source call / Probe / DTO 組立へ伝播させる契約、途中失敗で 200 partial と 503/504 を分ける条件が未確定である。P4 draft は remote call と evaluation の deadline / cancellation を要求する（`p4-remote-design.md:122`）。HTTP ref の `limits.rs` は ordinary operation 30 秒を定義するが、その ref は P5 に未統合である。

**必要な修正:** P5 の server-owned deadline、Source ごとの timeout、cancel/cleanup、optional/required failure と `evidenceSufficiency` / `evaluationCompleteness` / `partial` の組合せを truth table にする。上限で切れた結果を `partial=false` や無条件 `Sufficient` にしない。Problem の `type` と `code` は一対一に安定化する。[RFC 9457 §3.1.1](https://www.rfc-editor.org/rfc/rfc9457.html#section-3.1.1) は `type` を primary identifier とするため、「機械判定は code と status のみ」という記述は RFC 準拠の client 規則と衝突する。`instance` は個別発生の識別子であり、route path を使うか省略するかを決める。OpenAPI 3.2.1 の default schema dialect は Draft 2020-12 を拡張した [OAS dialect](https://spec.openapis.org/oas/v3.2.1.html#schema-object) である。埋込 schema と standalone JSON Schema の `$schema` を区別し、400/422/413/429/503/504 と Problem `type/code`、security、no-store を `spec/api/openapi.yaml` に明記して handler parity を検査する。

## P1〜P4 freeze 後の照合と GO 境界

| 凍結入力 | P5 の確定事項 |
| --- | --- |
| P1 | KnowledgeUnit と parent Resource/version/generation、field-level snippet/citation 権限、`bodyRequired` の supported/partial/unsupported の表示。Body hit を title hit で代用しない。 |
| P2 | vector mode の接続資格、score calibration、stable fusion/page order。未接続 mode や raw score を成功 DTO に入れない。 |
| P3 | Graph participant/path の current access と citation provenance。内部 path/locator を JSON/trace に直列化しない。 |
| P4 | trusted tenant/source scope、stable/ephemeral identity と GET 可否、Source visibility、remote continuation の公開可否、五つの retention mode と field disclosure。 |
| P6 | outbox delivery / index freshness の既知状態を readiness、coverage、gap に変換し、未接続を「最新」としない。 |

上記指摘を修正し、`spec/` の規範差分、OpenAPI 四 operation、閉じた request schema、Problem/error matrix、実 HTTP と handler の一致を確認すれば、共通 API 構成は freeze 対象にできる。P5 の公開資格には、同一 trusted resolver を Search/Document composition root に実配線し、実 DB + HTTP で actor、Source、candidate、field/evidence、history、cursor、retention の反例を通すことが必要。現時点の別 ref の HTTP stack、`paths: {}` の OpenAPI、設計文書だけではその資格を示さない。

## このレビューで行った確認

- 現行 code/規範と別 ref の `identity.rs`、`router.rs`、`error.rs`、`limits.rs`、`trace.rs` を read-only 確認。
- P5 設計案の四つの inline JSON 例は `json.loads` で parse 成功。schema/handler parity は schema・handler が未作成のため未検証。
- build、CI、実 HTTP 試験は実施していない。設計レビュー以外のファイルは変更していない。
