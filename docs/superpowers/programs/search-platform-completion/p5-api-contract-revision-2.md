# P5 Full API — source-neutral catalog と HTTP wire 契約改訂 2

- 状態: **設計追補案 / 独立再照合待ち**。`p5-api-contract-review.md` の P1 2 件・P2 1 件を閉じる入力である。本書だけで P5 Design Freeze、P4-02 code GO、`spec/`/OpenAPI 反映、四 route の実装・公開資格を宣言しない。
- 優先順: 本書は `p5-api-contract-reconciliation.md` の source-neutral catalog、401、deadline/504、P1 Archive binding に関する記述を置き換える。それ以外の P5 照合追補と P1/P3/P4 の凍結意味論、P7 の未実装 gate は維持する。元文書は変更しない。
- 固定入力 SHA-256: P5 照合追補 `a17920c81928c638ea11204f6dce5e62a0c72d52a286431c70d743dd62d8c12e`、独立 NO-GO review `21df587285a0a7fb08d916ea0fd8524866a02272b4e34d932ac00fd4bb0fb3dc`、P1 Archive binding ruling `eadaa11a8c02953f48e533c53d549e9b26f346dc862231d3d7622a9ee615599c`、P4 freeze `7b38cae0e4057556970a36c9df4752348c0ad1d7c4cac916eb794275d35bd5c5`。P7 shared durable design `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` は **draft** として照合した。現行 P4 code の型・port は `crates/search-application/src/scoped.rs` と `remote_registration.rs` で確認したが、並行実装中の未 commit code を契約の正本にしない。

## 1. 一つの actor/Source scope で Document と Remote を扱う

P4 の `TrustedSearchScope`、`AuthorizedSourceScope`、`AccessContextAuthorityPort`、`CurrentSourceVisibilityPort`、`ScopedSourceRegistryPort` が四 operation の唯一の認証・可視性境界である。`CheckedAuthorityAdapter` と `CheckedSourceVisibilityAdapter` の mint を再利用し、P5 専用の actor/Source mint、`ActorVisibleSourceRegistryPort`、別の Discovery loop を作らない。`POST /v1/search`、`POST /v1/discover`、`GET /v1/resources/{resourceId}`、`GET /v1/sources` は同じ request-local `TrustedSearchScope` と下記の同じ可視 catalog snapshot を受ける。P4 の `prepare_visible_sources` は Discovery 固有の evaluation binding を保ち、四 route 共通の actor/current/catalog 検査だけを共有 helper に抽出する。他の三 route に架空の Discovery evaluation を発行しない。

次は **`search-application` の実装目標型**であり、現存型を示すものではない。Remote の source-specific 設定は既存 `RemoteSourceRegistration` に留める。Document を Remote の空 endpoint や疑似 provider として表さない。

```rust
enum SourceKind { Document, Remote }

struct DocumentSourceRegistration {
    tenant: TenantId,
    source_id: SourceId,
    document_adapter_ref: DocumentAdapterRef, // host 設定の非公開 binding。DB URI ではない
    allowed_resource_kinds: Vec<ResourceKind>,
    supported_modes: Vec<DiscoveryMode>,       // LocalDirectory/LocalContentSearch のみ
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

trait ScopedSourceRegistryPort: Send + Sync {
    fn visible_sources<'a>(&'a self, actor: &'a TrustedSearchScope)
        -> BoxFuture<'a, VisibleCatalogSnapshot>;
}
```

`SourceRegistration::authority_descriptor()` は variant 内の値から都度導出する immutable view であり、独立に書き換えられる第二の authority record ではない。Document constructor は trusted host config と Document adapter の接続済み能力だけを受け、SourceId を server が発行し、local mode、resource kind、enumeration、retention と revision の整合を検査する。Remote constructor の既存の endpoint、limits、authority、retention 検査は残す。両 variant の `discoverable_source()` は登録済み能力からだけ Core `DiscoverableSource` を作り、`SourcePage` には照合追補 §2 の安全な能力 field と **実接続済み** coverage code だけを射影する。`document_adapter_ref`、remote endpoint、authority scope、access model、retention、credential は公開しない。`VisibleSourceRegistration::new` は scope の actor tenant/SourceId/registration revision/visibility revision と導出 descriptor の一致を要求し、登録 activation は scope に閉じて後述 `is_current` で照合する。

`VisibleCatalogSnapshot` の成功は、actor tenant に属する **Document と Remote の登録済み Source を共に列挙し、各 Source に同じ scope/current gate を適用し終えた**ことを意味する。`entries` の空配列も完全な列挙の結果だけに許す。`continuation_stamp` は authoritative visibility stamp または安定した registry epoch と可視集合の canonical digest から作る。安定性を証明できなければ `None` とし、初回の safe result は返せても Search/SourcePage cursor は発行しない。`SourcePage` は独立した部分成功を持たない。Source 固有 `Denied` / `Unknown`、`bind_source=None`、列挙中のその Source の revision/activation 競合はその Source の scope を除外し、他の許可済み Source を維持する。更新後の Source を旧 snapshot に混ぜない。actor/registry/ledger/visibility port error、全登録の列挙不能、重複 SourceId、tenant/kind の構造的不一致、snapshot の完全性を確認できない状態は `Err(SearchError)` として四 route とも generic `503 DEPENDENCY_UNAVAILABLE` に閉じる。途中まで作った一覧・gap・件数を 200 にしない。隠れた provider の稼働確認は可視集合確定前に実行しない。

## 2. ledger、current、名前空間の migration 契約

現行 `SourceRegistrationLedgerPort::reconcile(BTreeMap<SourceId, RemoteSourceRegistration>)` と `is_current(&RemoteSourceRegistration, ...)`、`RemoteRegistrationCatalog`、`TrustedVisibleRegistry`、`CheckedSourceVisibilityAdapter` は Remote 専用である。P5 実装前に **P4-02 の implementation amendment** を別ファイルで固定し、以下の source-neutral 型と port、既存 P4 regression の移行、独立 review を完了する。P5 HTTP だけで Remote-only port を包み、Document を別 port/別 visibility decision にすることを禁止する。`SourceRegistrationLedgerPort` の現行 method 名は `is_current` であり、照合追補の `current` 表記はここで訂正する。

```rust
enum RegistrationNamespace { Document, Remote }

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

`CompleteDesiredRegistrations` は trusted composition root だけが作る **その namespace の全 tenant にわたる完全な desired set**で、request や tenant 別 catalog の一部を入力にしない。Document/Remote それぞれの不在 key だけを tombstone にし、Remote reconcile が無関係な Document 行を、Document reconcile が Remote 行を削除・無効化しない。更新ごとの単調な `deployment_revision` と canonical `set_digest` を ledger が検査し、同 revision・異 digest と旧 revision の再適用を拒否する。二 namespace の初期 reconcile と catalog/adapter 確認の全てが成功するまで production factory は四 route を起動しない。稼働中の更新も ledger の正常 receipt 後にだけ local projection を差し替え、commit 応答不明時は成功を推測せず、再読・再照合まで current gate を閉じる。

ledger は **全 tenant、Document/Remote 共通の SourceId owner 正本**である。`(SourceId, tenant owner, kind)` の過去所有、全既存 `(SourceId,generation)` owner、versioned registration DTO と digest、registration/visibility revision、単調な activation epoch、ACTIVE/tombstone を durable・原子的に照合する。SourceId の tenant/kind の再割当ては tombstone 後も拒否し、同じ tenant/kind の明示的再有効化は新 activation と新 generation key を要する。registration/visibility の更新、削除・再有効化は old scope を失効させ、同 revision の削除後再登録でも old scope は戻らない。`is_current` は ACTIVE、tenant/kind/SourceId、**登録全体**、両 revision と activation を同じ ledger に照合し、process-local map や digest 単独の一致では true にしない。P7 draft の `search_source_ownership` / SQLx adapter はこの物理候補であり、現時点で実装・接続済みとは扱わない。source-neutral durable ledger がない production factory は起動拒否する。

`CheckedSourceVisibilityAdapter::{bind_source,current}` は同じ `SourceRegistration` catalog と ledger `is_current` を参照し、Document にも Remote にも既存 `AuthorizedSourceScope` を発行・再検査する。`ScopedSourceRegistryPort::visible_sources` は一つの union catalog を列挙し、`check_actor_current` を前後に行う。P4-07 の read/write、P5 の routing/pin/locator と item/field/Graph participant final gate はこの scope を継続使用し、Document Source の Version/Part/raw/current `Read` は Source 正本で別途再判定する。`GET /v1/resources/{resourceId}` の locator は可視 snapshot の Source に限り、裸の global ID から Document/Remote を推定しない。二つ目の actor/Source 許可集合も別 Discovery loop も追加しない。

## 3. 401 は配線済み scheme の challenge と一体

P5 の `401 AUTHENTICATION_REQUIRED` は、四 route の実 transport に適用できる `WWW-Authenticate` challenge が少なくとも一つある場合だけ生成する。[RFC 9110 §15.5.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.2) が header を必須とするためである。bearer、cookie、mTLS のいずれかを推測して固定しない。transport composition root に次の **型付き** binding を置き、認証 adapter と OpenAPI security scheme の同じ登録 ID に結び付ける。

```rust
struct SearchAuthSchemeBinding {
    scheme_id: RegisteredAuthSchemeId,
    security_scheme: OpenApiSecurityScheme,
    challenge_port: Arc<dyn SearchAuthChallengePort>,
}
trait SearchAuthChallengePort: Send + Sync {
    fn challenges(&self, operation: SearchOperation)
        -> Result<NonEmptyVec<ValidatedChallenge>, AuthConfigurationError>;
}
```

`ValidatedChallenge` は HTTP challenge grammar に適合する server-configured `WWW-Authenticate` field value で、CR/LF、principal、tenant、session、token、動的な resource/Source 情報を含めない。実 request の credential は transport adapter が検証して opaque session/access handle だけを `VerifiedActorResolverPort` に渡す。request body/header の raw self-principal、tenant、role、group、Source grant から scope を作らない。credential 欠落・無効時の 401 writer は binding から対象 operation の challenge を取り、Problem と `WWW-Authenticate` を同一 response に置く。認証済みだが operation 不許可は 403、trusted resolver の必須基盤障害は既存 `503 IDENTITY_UNAVAILABLE` とする。challenge を提供できない実 scheme を採る場合は、401 を出さずに認証失敗の status、Problem、OpenAPI、試験を明示的に再改訂するまでその route を起動しない。challenge port が未配線・空・不正でも 401 を header なしで送らず、composition を拒否する。

独立した trusted local server fixture は、設定済み scheme の test credential を server 側で opaque session に解決し、同じ handle で四 route を通す。未認証 request の 401 には実 fixture scheme に対応する challenge があり、`securitySchemes`/operation security/401 response header と byte-level に整合することを確認する。合成 self-principal header を渡すだけの HTTP canary を資格証拠にしない。production の scheme が決まった後もその実配線で同じ parity を確認する。

## 4. 内部 deadline と upstream wait の wire を分ける

[RFC 9110 §15.6.5](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.6.5) の 504 は gateway/proxy が必要な upstream 応答を時間内に受け取れない場合である。P5 の内部 final gate、local read、projection、serialization の operation deadline は 504 にしない。[同 §15.6.4](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.6.4) の `503 Service Unavailable` を、期限内に安全な final DTO を作れない内部処理の公開結果とする。Application/transport 間で原因を次の型に分け、自由文字列、Source 名、provider URL から status を推測しない。

```rust
enum DeadlineFailure {
    InternalOperationDeadline,
    GatewayUpstreamWaitTimeout, // 実際に gateway/proxy として必要 upstream を待った場合のみ
}
```

| 条件 | HTTP status / `about:blank` title | P5 `code` | 固定 `detail` |
| --- | --- | --- | --- |
| internal operation/final gate/serialization deadline、または gateway 条件を証明できない timeout | `503 Service Unavailable` | `SERVICE_UNAVAILABLE` | `The service could not complete the operation.` |
| 実 gateway/proxy の必須 upstream wait timeout | `504 Gateway Timeout` | `UPSTREAM_TIMEOUT` | `An upstream service did not respond in time.` |

旧照合追補の一律 `504 TIMEOUT` は P5 Search 四 route で撤回する。非 timeout の必須 identity/registry/ledger/visibility 障害には既存の `503 IDENTITY_UNAVAILABLE` / `503 DEPENDENCY_UNAVAILABLE` を使う。optional/independent Source の失敗や budget は、可視 Source のみに基づく typed gap と安全な独立結果を期限内に確定し、全 final gate を通せた場合だけ既存の `200 partial` とする。required evidence が未評価なら `sufficient` にしない。安全な DTO が確定しなければ全体を上表の Problem にし、途中 buffer を送らない。headers 送出後の write/deadline/disconnect は status を差し替えられないため接続を中断し、`TransientDisclosureLease` と buffer を閉じる。`NO_RETENTION` の再保持はしない。

全 P5 Problem は照合追補 §5 の `Content-Type: application/problem+json`、`type: "about:blank"`、実 HTTP status と body `status` の一致、固定 `title`/`detail`、`Cache-Control: private, no-store`、`X-Content-Type-Options: nosniff` を継承する。401 に限り適用可能な `WWW-Authenticate` を **必須**とし、retry policy が安全に確定した時だけ `Retry-After`/`retryable` を付ける。[RFC 9457 §4.2.1](https://www.rfc-editor.org/rfc/rfc9457.html#section-4.2.1) に従い 503/504 の title は上表の reason phrase とする。`code` は P5 の extension registry であり Problem `type` の代用ではない。Document HTTP の既存 URN 契約は変更しない。

## 5. P1 Archive ruling と後続の exact action

P1 Archive binding ruling の上記 exact SHA を、P5 の body claim/Resource detail の検証入力として明示的に継承する。Archive part の `UnitAuthorityBinding.archive_inner_format=None` は part 全体に単一 leaf format を課さない。各 Unit は `archive_inner_format=Some(leaf)`、正確な member chain、trusted `ArchiveProfilePlan` の leaf format/inner locator/UnitKind と一致する。binding の `Some(leaf)` は追加の単一形式制約であり、非 Archive は None を維持する。共通 composed profile と outer reader node の `UnitProvenance.parser_build_id` の照合、親 Version/Part/raw/current Read、verified Unit と coverage/negative proof の条件は省かない。Text と CSV の異種 leaf が同じ part にある場合も part-wide scalar を要求しない。

1. 専任 P4 implementation writer が別 amendment に §1–2 の型・ledger 名前空間・union catalog の移行、Source-specific race と infra error の決定的試験を記す。Remote-only 現行 code や P7 draft を production 接続済みと見なさず、P4-02 の独立 code review を再実施する。これが P5 code 着手の必須前提である。
2. 独立 P5 reviewer が本書を P5 照合追補、NO-GO review、P1 Archive ruling、P4 freeze/plan、P7 draft、現行 P4 code、RFC 9110/9457 に再照合する。GO 後に限り P5 design precedence と freeze に本書の exact SHA を記す。人手承認待ちは置かない。
3. Search 専用 `spec/operations/error-handling-resilience-requirements-v0.md` §15、`spec/data/logical-data-model-v0.md`、`spec/api/openapi.yaml` の四 operation と `p5-api-plan.md` に §1–4 の型、401 header、503/504 registry、実 security scheme、全 response/header/schema を反映する。Document HTTP の契約変更が必要なら別の明示差分にする。
4. 焦点を絞った gate で、Document と Remote の共通可視 catalog、tenant/kind collision と tombstone、namespace reconcile の非干渉、activation/revision 取消、個別 Denied/Unknown と登録競合で他 Source を保つこと、infra error の四 route 503、SourcePage stamp、実 trusted identity と challenge、internal 503/upstream 504/partial 200、Problem/OpenAPI/handler parity を確認する。P1 full body/Archive 異種 leaf、P3 全 participant、P4 二 lease と `NO_RETENTION`、Document 実 DB と remote local TCP の縦断・exact-head hosted gate は別の公開資格証拠として残す。
