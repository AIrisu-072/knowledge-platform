# P4-02 source-neutral registration / visible catalog 実装追補

Status: **実装入力 / P4 source-neutral code 未実装・独立 review 待ち**。本書は [P4 freeze](p4-remote-freeze.md) と [P4 plan](p4-remote-plan.md) の P4-02 だけを、受入済み [P5 API 改訂 2](p5-api-contract-revision-2.md) §1–2、[P7 共有 durable 改訂 1](p7-shared-durable-revision-1.md) §5 に合わせて差し替える。P4-03 以降の routing、remote transport、二つの lease、P5 HTTP 四 route、P7 実 SQL adapter の完了は主張しない。

固定入力 SHA-256: `p5-api-contract-revision-2.md` `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9`、`p7-shared-durable-revision-1.md` `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe`。確認した P4 freeze `7b38cae0e4057556970a36c9df4752348c0ad1d7c4cac916eb794275d35bd5c5`、P4 plan `a713a6f0d2f5f1d0066576dada09f4987e54bccf5c7df2330fa89f565bb31da0`、[P4 trust final review](p4-trust-code-final-review.md) `77c704b36a196ce61d98f270cbb0b1ed3508a40b47ac2d113efc73aa90877db5`。現行 `scoped.rs` / `remote_registration.rs` は Remote 専用であり、既存 `scoped_catalog_contract` 13 件の trust seam GO はこの改訂後の GO を意味しない。

## 1. 一つの型・mint・可視集合

実装場所は `search-application` とし、`source_registration.rs` が次の型と純粋な canonical codec を所有する。field は private。既存 `RemoteSourceRegistration` の endpoint、limits、authority、retention と server-only constructor の検査を弱めない。

```rust
enum SourceKind { Document, Remote }
enum RegistrationNamespace { Document, Remote }
struct DocumentSourceRegistration {
    tenant: TenantId,
    source_id: SourceId,
    document_adapter_ref: DocumentAdapterRef,
    allowed_resource_kinds: Vec<ResourceKind>,
    supported_modes: Vec<DiscoveryMode>,
    enumeration_semantics: EnumerationSemantics,
    retention_mode: RetentionMode,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}
enum SourceRegistration {
    Document(DocumentSourceRegistration),
    Remote(RemoteSourceRegistration),
}
struct SourceAuthorityDescriptor {
    tenant: TenantId,
    source_id: SourceId,
    kind: SourceKind,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}
struct VisibleSourceRegistration {
    scope: AuthorizedSourceScope,
    registration: SourceRegistration,
}
struct VisibleCatalogSnapshot {
    entries: Vec<VisibleSourceRegistration>,
    continuation_stamp: Option<VisibleSetStamp>,
}
```

`SourceRegistration::{tenant,source_id,kind,registration_revision,visibility_revision,discoverable_source}` は variant に委譲する。`authority_descriptor()` はその時点の variant の値から作る immutable view で、別に更新可能な authority record を保存しない。`VisibleSourceRegistration::new` は同じ `TrustedSearchScope` の tenant、SourceId、registration/visibility revision とこの descriptor を一致させる。activation は既存 `AuthorizedSourceScope` に保持し、後述 `is_current` で照合する。`Document` を endpoint 空の `Remote`、provider kind の特殊文字列、または公開 DTO の自己申告として表さない。`SourceId` は trusted server/host が割り当て、request と provider は割り当てられない。

Document constructor は trusted host config の非秘密 `DocumentAdapterRef` と、**実際に接続済みの Document adapter** が発行した capability witness を突き合わせる。adapter ref は binding ID であり DB URI、credential、provider URL を含まない。local mode は `LocalDirectory` / `LocalContentSearch` のみ、非空・重複なしで、実接続済み mode の部分集合とする。resource kind、enumeration semantics、retention mode も接続済み adapter の実能力と整合させ、正の両 revision、tenant/SourceId と非空 binding を検査する。能力未配線、incompatible な retention、実装のない content search、未検証の complete enumeration は constructor を失敗させる。`discoverable_source()` と後続 `SourcePage` の capability/coverage は検査済みの能力からだけ導出し、未接続 mode や positive coverage を公告しない。公開 `SourcePage` は P5 の安全な能力 field のみで、adapter ref、remote endpoint、authority scope、access model、retention、credential を含めない。

`ScopedSourceRegistryPort::visible_sources(&TrustedSearchScope) -> BoxFuture<'_, VisibleCatalogSnapshot>` に変更し、**一つの union catalog** から actor tenant の Document と Remote を列挙する。`CheckedAuthorityAdapter`、`CheckedSourceVisibilityAdapter`、`TrustedSearchScope`、`AuthorizedSourceScope`、`CurrentSourceVisibilityPort` の既存 mint/current gate を両 variant に共通使用する。P5 専用 mint/registry、Document 専用 visibility decision、第二の Discovery loop は作らない。四 route 共通の actor/current/catalog 検査は helper に抽出し、`prepare_visible_sources` は Discovery 固有 binding の薄い adapter とする。Search/Resource/SourcePage 用に架空の evaluation は発行しない。

成功した `VisibleCatalogSnapshot` は両 kind の全登録列挙と各 Source の current 確認が済んだ結果である。空集合も完全列挙の結果に限る。actor current を列挙前後に確認し、Source ごとに bind → scope の actor/Source/descriptor 一致 → `current` → ledger activation/DTO current を確認する。Source 固有 `Denied` / `Unknown`、`bind_source=None`、列挙中の当該 Source revision/activation 競合はその Source だけを除外し、旧 snapshot に新 revision を混ぜず、無関係な Source を残す。structural actor/Source/tenant/kind mismatch、重複 ID、catalog/ledger/visibility error、全登録列挙または完全性の不証明は全体 `Err(SearchError)` として閉じ、部分一覧・gap・件数を成功値にしない。隠れた provider の稼働確認は可視集合確定後まで行わない。

`continuation_stamp` は host-authoritative visibility stamp、または安定した registry epoch と可視集合の canonical digest が同一 snapshot に束縛できる場合だけ `Some` とする。原子性を証明できない現段階では `None` を返す。`None` の snapshot は初回の安全な bounded 応答には使えるが、Search/SourcePage cursor を発行しない。SourcePage が一 response に収まらない場合は P5 freeze に従い generic 503 とし、truncate した 200 や無署名 cursor を出さない。

## 2. complete desired set と一つの ownership ledger

`RegistrationSetRevision` は正の単調 `u64`、`RegistrationSetDigest` は private 32-byte SHA-256 値。次の port は Remote-only 版を**置換**し、production に第二の Remote ledger を残さない。

```rust
struct CompleteDesiredRegistrations {
    namespace: RegistrationNamespace,
    deployment_revision: RegistrationSetRevision,
    set_digest: RegistrationSetDigest,
    registrations: BTreeMap<SourceId, SourceRegistration>,
}
trait SourceRegistrationLedgerPort: Send + Sync {
    fn reconcile<'a>(&'a self, desired: &'a CompleteDesiredRegistrations)
        -> BoxFuture<'a, BTreeMap<SourceId, RegistrationActivation>>;
    fn is_current<'a>(&'a self, registration: &'a SourceRegistration,
        activation: RegistrationActivation) -> BoxFuture<'a, bool>;
}
```

`CompleteDesiredRegistrations` の作成権限は trusted composition root が保持する **host-owned complete snapshot** に限定する。namespace ごとに全 tenant・全当該 kind の key/DTO を列挙してから revision を付ける。request、provider、tenant ごとの部分 map、`Vec<RemoteSourceRegistration>`、呼出し側が渡した map 自身を「完全」の証明にしない。empty set も host がその namespace の完全な空集合を確認した場合だけ作れる。namespace と全 entry kind、map key と DTO SourceId、重複・owner・revision・digest を作成時に検査する。reconcile 側も host authority の**同 revision の現在の全 snapshot**を独立に取り直し、全 key/variant/DTO 値の exact equality と双方の canonical digest を確認してから変更する。commit 直前にも host revision/digest が current か確認する。partial tenant map、extra/missing key、未確定 snapshot、異なる DTO は原子失敗にする。synthetic fixture も別の host snapshot 正本を持ち、この比較を通す。単なる map から作る便利な公開 constructor は置かない。

canonical encoder は `Document` に `document-desired-set:v1`、`Remote` に `remote-desired-set:v1` の namespace/version domain separator を付ける。entry は SourceId の 16 UUID bytes 昇順で並べる。全 field を固定 tag・順序・長さの frame にする。UTF-8 は byte 長、enum は明示固定 tag、整数は固定幅 endian、`Option` は有無 tag、vector は個数と各 element の frame を入れる。DTO 上の vector 順を保持するか、意味上 set であれば constructor で一意に正規化することを codec と fixture で一つに固定し、hash-map 反復順、Debug 表記、JSON/JSONB 物理 byte に依存しない。共有 `sha2` は既に `search-application` の workspace dependency なので追加 dependency は不要。

各 entry に tenant、kind、SourceId、registration/visibility revision と **全 server-owned DTO field** を入れる。Remote は provider kind、endpoint の scheme/host/port/base path、supported modes、enumeration、authority predicates、allowed resource kinds、current access contract、retention、freshness、canonical lineage、limits の全欄を含む。Document は adapter ref、allowed kinds、local modes、enumeration、retention を含む。endpoint は非公開の operator registration 値であり digest 入力から省かない。credential、DB URI、secret はどちらの DTO にも入れず、digest 入力にもならない。set digest だけで DTO equality や current 判定を代替しない。version 未知、field 追加時の encoder 未改訂、overflow は fail closed とする。

ledger は全 tenant・両 kind の `SourceId → (tenant owner, kind, full DTO, registration/visibility revision, activation, ACTIVE/TOMBSTONED)` と既存 `(SourceId,generation)` owner を一つの正本で照合する。`SourceId`、owner、kind は tombstone 後も不変であり、反対 kind/tenant への再割当ては拒否する。更新では namespace ごとの revision/digest を同じ global serialization 境界で検査する。旧 revision、同 revision 異 digest、同 digest だが異なる map を拒否し、同 revision・同 digest・同 map だけを冪等にする。新 revision は当該 namespace の登録更新と**その kind に限る**不在 key の tombstone を一原子操作で行い、反対 namespace の登録、activation、visibility を触らない。DTO/visibility 変更、削除、同 owner/kind の明示再有効化では activation を単調に進め、旧 scope を失効させる。既存の per-Source registration/visibility revision rollback と同 revision の異なる定義の拒否も維持する。再有効化で同 revision を許す方向へ緩めない。activation overflow は失敗し、巻き戻さない。

`is_current` はその source が ACTIVE で、owner/kind/SourceId、保存した **whole DTO**、両 revision、activation が全て一致する場合だけ true を返す。tombstone、欠損、旧 activation/DTO、別 kind、別 owner は false、ledger 基盤障害は `Err`。catalog の `current_activation` と両 visibility adapter は同じ `SourceRegistration` とこの ledger を読む。ledger receipt の全 key/activation の検証後だけ local union projection を差し替え、更新中・unknown commit・projection 不一致は current gate を閉じて authoritative 再読を要求する。起動時は Document/Remote **両 namespace の complete reconcile** と接続済み adapter 検査が済むまで production 四 route を構成しない。

P7-02 の SQL adapter は後で**この同じ port**を実装する。P7 改訂 1 §5 の `search_registration_serial` 単一 global lock、namespace 別 deployment revision/digest、`source_kind` CHECK と不変 owner/kind、kind 限定 tombstone、commit 直前 host current 再照合、同 transaction の activation/fence を要する。本追補の synthetic ledger は再起動を模擬できても実 PostgreSQL durability、複数 replica、migration/role、production factory の接続証拠ではない。P7 の実 DB RED/GREEN と独立 gate を別に残す。

## 3. 一人の限定 writer と focused RED → GREEN

P4 source-neutral 実装は一つの bounded writer window に直列化する。書込範囲は `crates/search-application/src/source_registration.rs`（新規）、`scoped.rs`、`remote_registration.rs`、`lib.rs`、既存 `tests/scoped_catalog_contract.rs`、新規 `tests/source_neutral_catalog_contract.rs` に限る。`remote_registration.rs` は Remote DTO と必要な互換 facade に絞り、union catalog/ledger の唯一の定義を `source_registration.rs` に置く。`lib.rs` は export のみ。`Cargo.toml`、`Cargo.lock`、P5 HTTP/`spec/`、P7 SQL、他の進行中 writer のファイルには触らない。現行 Remote-only `try_new(ledger, Vec<RemoteSourceRegistration>)` / `replace_checked(Vec<_>)` を production の desired-set 入力として残さない。既存 13 試験の名前と意味は維持し、必要な setup だけ host-owned complete snapshot を使う synthetic helper に移す。互換 facade を残すなら synthetic/test-only と明示し、production 経路が部分 desired を渡せない型にする。

1. **RED — 型と constructor。** 新規 test に `document_requires_connected_capability_witness`、`document_rejects_remote_or_unwired_local_mode`、`document_descriptor_and_safe_capability_are_derived`、`canonical_set_digest_covers_every_remote_and_document_field`、`digest_is_stable_across_map_insertion_order` を先に追加する。未実装の union 型/codec、実接続 witness 拒否で失敗することを記録する。Remote endpoint 等の field ごとの mutation table と、empty/None/vector/Unicode/framing 衝突反例を含める。
2. **GREEN — 型と canonical codec。** `source_registration.rs` と `lib.rs` のみで constructor/descriptor/codec を実装し、同じ focused test を通す。固定 golden vector を test に残し、将来 P7 SQL adapter が同じ codec を呼ぶ。未公開 capability を positive としない。
3. **RED — 完全集合と ledger。** 新規 test に `remote_reconcile_never_tombstones_document`、`partial_tenant_map_cannot_tombstone_foreign_remote`、`document_remote_same_source_id_is_rejected_after_tombstone`、`partial_or_stale_remote_desired_set_is_atomic_failure`、`same_revision_same_digest_is_idempotent`、`old_activation_and_whole_dto_fail_current`、`unknown_commit_blocks_local_projection` を追加する。host 正本を二 tenant/二 namespace で保持し、candidate だけを欠落・改竄する。失敗時に両 namespace の entry/activation/revision/digest が全て不変であることを確認する。
4. **GREEN — ledger/catalog。** 既存 synthetic ledger と `RemoteRegistrationCatalog` を source-neutral な一 ledger/union catalog に移し、両 namespace の complete snapshot を初期化する。legacy 13 試験の asserted behavior を保ったまま setup を移す。新旧 scoped test を同時に通し、production durable adapter と混同しない。
5. **RED/GREEN — visible snapshot。** `union_catalog_includes_document_and_remote_with_one_scope`、`visibility_denied_or_unknown_keeps_other_source`、`catalog_revision_race_omits_only_changed_source`、`structural_mismatch_or_infrastructure_error_fails_whole_snapshot`、`unstamped_snapshot_has_no_cursor_authority` を追加してから `scoped.rs` の port/adapter/helper と `TrustedVisibleRegistry` を更新する。全体 error では成功 entries が返らず、個別 denial では他 Source が返ることを外部可視値で検査する。13 既存 case を全件再実行する。

Focused command: `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test source_neutral_catalog_contract` と `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test scoped_catalog_contract`。各 RED は狙った欠落/反例による失敗を記録し、同じ command で GREEN を取る。変更ファイルの差分、`cargo fmt --all -- --check`、`cargo clippy -p search-application --locked --all-targets -- -D warnings` を確認し、必要な既存 application port contract だけ追加で実行する。全 CI を各 step で繰り返さない。

**P5 code 着手前 gate:** 別の read-only reviewer が本追補、固定 P5/P7 SHA、writer の exact source/test hash、13 件と新規 named test の fresh GREEN、scope mint 一個、complete snapshot、host authority/digest、kind tombstone、current/activation、error/race、Document 実能力公告を照合し、P4 source-neutral seam に限定して GO/NO-GO を記録する。NO-GO なら同じ限定 writer が修正して再 review する。この GO は P7 production SQL durability や P5 四 route の資格に昇格しない。独立 GO の後に P5 code writer と P7-02 port adapter writer をそれぞれ直列の専用 window で開始する。merge/deploy はしない。
