<a id="p7-shared-durable-generation--production-implementation-plan"></a>
# P7 共有永続化世代の本番実装計画

[翻訳元の固定原文（commit 0ecf486719e3c9d71242e289a7564ad6d1032b3c）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-plan.md)

本書は意味保存の日本語訳であり、原設計の再承認・資格追加ではありません。既存承認hashは当時の原文・証拠を指し、訳文hashではありません。以下の状態・次の作業は当時の記録で、現在の実行指示ではありません。本文中の元の行番号も当時の原文を参照しており、訳文の行番号ではありません。旧見出しへのリンクは明示的なIDで維持しています。

> **当時の実行担当への指示：** Toolbox v8の`orchestrate` → `toolbox-context`を使い、各タスクを一つの責務・限定した書き込み範囲を持つ管理下ワーカーに渡します。各タスクのRED、GREEN、独立した読み取り専用レビューを記録します。native `spawn_agent`へ無断で切り替えません。この指示は当時の計画の引用であり、現在の実行許可ではありません。

**目標：** P1の完全な世代バンドル、P3で選定済みのGraph、P6のフェンス付き配送処理が、同一PostgreSQL上のSource、current（現在の世代）、READY（準備完了状態）、pin（世代の固定）、guard（構築中の保護）、receiptと、復元可能な索引の実体を安全に共有することです。

**構成：** P6 Search `0001`を、一つのSource行と移行台帳の正本とします。P7 `0002`で全テナント・全Source種別の所有権を、`0003`で世代・guard・pinの制約を追加します。SQLxを公開しないアプリケーションポートの背後で、`search-runtime`が一接続の短いトランザクションを所有します。純粋な正規形式のエンコード・デコード処理、DBに保存した全データ、実際の字句検索ファイル、条件付きのP3 Graphを別々に検証します。P3の本番資格が成立するまで、Graphに依存しないスキーマ・ポートだけを局所的な受入対象とし、READYと公開は閉じたままにします。

**技術構成：** 既存のRustワークスペース、SQLx `0.9.0`、PostgreSQL `18.6`、`testcontainers`、`sha2`、`serde_json`、既存のTantivyを使います。資格未取得の新しい依存関係は導入しません。

**仕様：** `spec/data/logical-data-model-v0.md`のP1-L1〜L3、`spec/data/transaction-consistency-requirements-v0.md`のSD-T11〜T13/P6 outbox、`spec/selection/library-tool-selection-v0.md`に従います。実装入力は、[元設計の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-design.md)のSHA-256 `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b`と、[改訂1の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-revision-1.md)のSHA-256 `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe`を統合したものです。抵触する五件は改訂を優先します。現在の日本語版は[元設計](p7-shared-durable-design.md)と[改訂1](p7-shared-durable-revision-1.md)を参照してください。[独立再審査のGO](p7-shared-durable-recheck.md)と[凍結記録](p7-shared-durable-freeze.md)は、**設計の判定だけ**です。P1のExtraction/KnowledgeUnit、P3のGraph・構築guard・外部キー制約を守る削除処理、P5のSource種別に依存しない改訂2、P6のOutboxについても、それぞれの凍結記録・計画に拘束されます。

本文中の「Searchイベント処理記録（Search receipt）」はDBに保存するイベント処理記録です。バンドルや成果物のreceiptは整合性を示すメタデータ、各工程のreceiptは検証記録・証拠を指します。これらを混同しません。

<a id="global-constraints-and-gates"></a>
## 全体の制約と必須ゲート

- P6-I03が、`crates/search-runtime/migrations/0001_search_source_coordination_v0.sql`と`search_runtime_sqlx_migrations`の唯一の書き込み担当です。P7は`0001`を改名・再採番せず、Search `0002`以降だけを書き、`search-runtime::migrate(&PgPool)`の既存の単一入口を使います。順序はDomain `0009` → Search `0001` → P7 `0002+`です。Graphは別の`search_graph`スキーマ・台帳を使います。SQLx `Migrator::dangerous_set_table_name`の固定名称を途中で変更しません。
- `outbox_events`、Source、所有権、世代、guard、pin、Searchイベント処理記録は同一DBに置きます。P3の`source_control`は`search_source_coordination`の別名であり、第二のポインター・エポック、別プールによる見かけ上の原子操作、Graph側のSource発行処理を作りません。Searchイベント処理記録は履歴メタデータであり、世代への外部キーやGCを防ぐpinにしません。
- P3-P04の実バックエンド選定・独立再判定と、P1の構築入力・P3の内容を**別々に**表す`GraphReceiptMappingV1`の正規形式エンコーダー・固定の期待値を持つ参照データ（golden vector）のGOより前は、Graphの本番データベース移行、Graph READY、P7 READY、公開、永続pinの成功を許しません。PostgreSQL以外が選ばれた場合は、同等の原子性・フェンス手順の設計と独立審査が済むまで、Graph接続タスクを停止します。隔離PoCのPASSは資格取得ではありません。
- P5改訂2の`SourceRegistration::{Document,Remote}`、`RegistrationNamespace`、`CompleteDesiredRegistrations`と、既存P4の`TrustedSearchScope` / `AuthorizedSourceScope`を採用します。ポートの差替え前に、P4-02のSource種別に依存しない実装改訂と、独立したコードレビューを満たします。二つ目のactor/Source発行処理や、テナントごとにRemoteの登録状態を突き合わせる正本は作りません。
- P1の`ProjectionGenerationManifest.digest`は、投影だけを対象にしたprojection-only v1のままです。P1の複合ダイジェスト形式composite v1は、別の32-byteダイジェストと`bundle_version`を持ち、SourceのcurrentとSearchイベント処理記録にある`sha256:`に続く小文字16進数表現に一致させます。Vectorを採用する場合はv1へ暗黙に追加せず、新しいバンドル版と参照データを先に凍結します。`Retryable`、欠損、未知のDTO・版・フィールド、保持条件不明の場合はREADYを拒否します。
- NoRetention/SessionOnlyのRemote由来のバイト列、Unit、Graph、索引、整合性メタデータ、バックアップを永続化しません。`PersistentDiscoveryMetadata`は、本文Unitや埋め込みの保持許可ではありません。P4のRAM評価リースとPostgreSQLのpinは、別のライフサイクルを持ちます。
- 共有ルートの`Cargo.toml` / `Cargo.lock`は、P6-I01・P1-I01・P3の書き込み担当と直列化した、一つの担当期間だけで変更します。本計画では既存のワークスペース依存関係を再利用します。P6の汎用outboxクレートはSearch/P7クレートをインポートせず、`delivered_at`は汎用ワーカーの別トランザクションが所有します。P6-S02/S03、P3-C01/C02/C04、P1-B01/B02、P4-02で同じファイルを書く担当者の順序を固定します。
- `40001`、`40P01`、ロックのタイムアウトでは、同じ期待状態・冪等性キーを使い、トランザクション全体を回数を限って再試行し、その都度DB条件を再評価します。カウンターのオーバーフロー、DBまたはコミットの応答不明、正規形式のエンコード・デコード未対応の場合は、安全側に倒して拒否します。長時間のSource読み取り、解析処理、ファイル同期・走査、Graphコピーを、Source/outboxのロック中に行いません。
- SQLロールは、限定した管理者がクラスタ側で`sql/roles.sql`により作成・権限付与します。Search移行の後に権限を適用し、起動処理がロール・スキーマ・トリガーと実接続ロールを検査します。欠落や過大な権限がある場合はAPI/claim（処理権の取得）を有効にしません。P6のSearch完了処理がoutboxを`FOR UPDATE`するための権限は、P6-I04の`UPDATE(lease_token)`限定に合わせ、`delivered_at`・Audit・DocumentのUPDATE権限は与えません。

<a id="file-and-sole-writer-map"></a>
## ファイルと唯一の書き込み担当の対応

| 所有タスク | ファイルと責務 |
| --- | --- |
| P7-01 | `crates/search-runtime/migrations/0002_search_source_ownership_v1.sql`, `tests/source_ownership_migration.rs`：全体の所有権・名前空間、既存行の監査、ロール設定の第一段階。P6-I03の`0001`書き込み担当と直列化します。 |
| P7-02 | `crates/search-runtime/src/{source_registration,source_lease}.rs`, `tests/source_registration.rs`：P5のSource種別に依存しない望ましい登録状態全体を、原子的に突き合わせます。P6-S02リースの有効状態・有効化条件も扱います。`crates/search-application/src/{remote_registration,scoped,ports}.rs`の型変更は、P4-02/P6-S01の書き込み担当の後に設けた専用期間で行います。 |
| P7-03 | `crates/search-runtime/migrations/0003_search_generation_v1.sql`, `sql/roles.sql`, `tests/generation_schema.rs`：識別情報・世代・保存データ・整合性メタデータ・字句検索・guard・pin、複合外部キー、変更不能性、実ロール。Graph DDLは含めません。 |
| P7-04 | `crates/search-core/{Cargo.toml,src/{projection_bundle,lib}.rs}`, `crates/search-application/src/ports.rs`でのCore型の再公開、`crates/search-projection-memory/src/store.rs`, `crates/search-runtime/src/{payload,bundle_codec}.rs`, `tests/bundle_durability.rs`：純粋な正規形式の計算と、型付きDTOの保存・復元。P1-B01/B02の型・エンコード担当と直列化します。 |
| P7-05 | `crates/search-runtime/{Cargo.toml,src/lexical_artifact.rs}`, `tests/lexical_artifact.rs`：仮置き・確定・再オープン・整合性封印・ファイルツリーのダイジェストと、DB外のバイト列。P1-E01が列挙する実際の検索対象Unit文書を利用します。 |
| P7-06 | `crates/search-runtime/src/{generation_registration,full_guard}.rs`, `tests/full_guard.rs`：非公開のEVENT/MANUAL登録、恒久的な識別情報、完全構築のトークン・フェンスと失効時の拒否。同じSourceを書くP6-S03/P3-C01と直列化します。 |
| P7-07 | `crates/search-runtime/{Cargo.toml,src/graph_registration.rs}`, `crates/search-graph/src/{repository,stage}.rs`, `tests/graph_registration.rs`：**P3の条件付き**で、一接続によるGraph親行の登録と子行のバッチ用ポートを実装します。Graph DDLが未適用ならP3-G03の`0001`の単独担当、適用済みならGraph台帳に追加する新しい移行の単独担当が扱い、適用済みファイルは書き換えません。P3-G01/G03/G04/C01の担当と同時編集せず、その専用期間で改訂を反映します。 |
| P7-08 | `crates/search-runtime/src/ready.rs`, `crates/search-graph/src/{canonical,recovery}.rs`, `tests/ready_bundle.rs`：**P3の条件付き**で、二つのエンコーダーの対応付けと、Graph・字句検索・全保存データのREADYを実装します。P1-B02/P3-G02/G08の担当と直列化します。 |
| P7-09 | `crates/search-application/src/{ports,indexing_service}.rs`, `crates/search-runtime/src/{event_completion,manual_publication}.rs`, `tests/event_completion.rs`：**P3/P6の条件付き**で、イベント由来の確認と、ポインター＋Searchイベント処理記録のコミットを実装します。P6-S01/S03/S04の型・接続処理の担当と直列化します。 |
| P7-10 | `crates/search-runtime/src/pin.rs`, `tests/pin_scope.rs`：**P3の条件付き**で、actorの権限範囲に結び付いたpinの取得・更新・結果返却・解放を実装します。P3-C04の担当と直列化します。 |
| P7-11 | `crates/search-runtime/src/gc.rs`, `tests/gc_races.rs`：**P3の条件付き**で、current・pin・guardを保護し、外部キー制約を守るGCを実装します。P3-C02/C03の担当と直列化します。 |
| P7-12 | `crates/search-runtime/src/recovery.rs`, `tests/process_restore.rs`：**全成果の集約後**に、再起動・強制終了・別DBへの復元・破損を扱います。ルート・Cargoと他の実装範囲には触れません。 |

`crates/search-runtime/src/lib.rs`のモジュール登録と、`Cargo.toml`の既存ワークスペースへの依存接続の追加は、P7-01→12の当該担当が順に一回ずつ行います。P7は`crates/search-source-document/src/outbox.rs`を単独で上書きしません。P1-B02、P6-S04、P3-D01が合流した後、P7-09の専用統合期間で本番の処理経路を接続します。`search-runtime/src/lib.rs`には当時`migrate`しかなく、READYを実装済みとは扱いません。

<a id="interface-contract-to-reconcile-before-code"></a>
## コード作成前に整合させるインターフェース契約

`search-application::ports::BoxFuture<'a,T>`は`Result<T,SearchError>`を内包します。以下は本番実装の目標であり、現行の単一`CompleteEventRequest.candidate`やRemote専用の`SourceRegistrationLedgerPort`が実装済みという意味ではありません。P4-02/P6-S01の既存試験と呼び出し元を同じ書き込み担当期間で移行し、公開ポートにSQLx/PgPoolを露出させません。コードブロックは識別子・契約を保存するため原文のままです。

```rust
// P5 改訂 2 の SourceRegistration/CompleteDesiredRegistrations を使う。
trait SourceRegistrationLedgerPort: Send + Sync {
    fn reconcile<'a>(&'a self, desired: &'a CompleteDesiredRegistrations)
        -> BoxFuture<'a, BTreeMap<SourceId, RegistrationActivation>>;
    fn is_current<'a>(&'a self, registration: &'a SourceRegistration,
        activation: RegistrationActivation) -> BoxFuture<'a, bool>;
}
enum CompleteEventRequest {
    PublishCandidate { fence: SearchDeliveryFence,
        expected_current: CurrentGenerationSnapshot,
        event_candidate: EventCandidateHandle, verified_bundle: VerifiedBundle },
    ReuseCurrent { fence: SearchDeliveryFence,
        expected_current: CurrentGenerationSnapshot,
        expected_manifest_digest: String, expected_bundle_digest: String },
}
trait DurableGenerationCoordinator: Send + Sync {
    fn begin_event_full<'a>(&'a self, fence: SearchDeliveryFence,
        manifest: PersistableGenerationManifest, ttl: BoundedTtl)
        -> BoxFuture<'a, EventCandidateHandle>;
    fn begin_manual_full<'a>(&'a self, source: SourceFence,
        manifest: PersistableGenerationManifest, ttl: BoundedTtl)
        -> BoxFuture<'a, ManualBuildHandle>;
    fn renew_full_guard<'a>(&'a self, handle: &'a FullBuildHandle,
        ttl: BoundedTtl) -> BoxFuture<'a, ()>;
    fn pin_current<'a>(&'a self, binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope, ttl: BoundedTtl)
        -> BoxFuture<'a, PinnedBundleLease>;
    fn renew_pin<'a>(&'a self, pin: &'a PinnedBundleLease,
        binding: &'a TrustedDiscoveryBinding, scope: &'a AuthorizedSourceScope,
        ttl: BoundedTtl) -> BoxFuture<'a, ()>;
    fn verify_pin_before_return<'a>(&'a self, pin: &'a PinnedBundleLease,
        binding: &'a TrustedDiscoveryBinding, scope: &'a AuthorizedSourceScope)
        -> BoxFuture<'a, ()>;
    fn release_pin<'a>(&'a self, pin: PinnedBundleLease,
        binding: &'a TrustedDiscoveryBinding, scope: &'a AuthorizedSourceScope)
        -> BoxFuture<'a, ()>;
}
trait HostScopeReferencePort: Send + Sync {
    fn issue<'a>(&'a self, binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, ActorScopeRef>;
    fn current<'a>(&'a self, reference: &'a ActorScopeRef,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessBindingState>;
}
```

`BoundedTtl`はホストポリシーで最小・最大を検査済みの時間長、`BuildHandle`は`Full(FullBuildHandle)|Incremental(BuildGuardHandle)`、`ActorScopeRef`は秘密情報ではなく、空ではなく、長さ上限があり、再起動後もホストが再解決できる参照とします。`HostScopeReferencePort`は、既存の信頼されたactor・Source・評価の保存用参照を発行して再照合するアダプターであり、新たなactor発行処理ではありません。`EventCandidateHandle`、`ManualBuildHandle`、`FullBuildHandle`、`VerifiedBundle`、`PinnedBundleLease`のフィールドとコンストラクターは非公開です。P7アダプターはDB行とホストの現在状態を必ず再読し、ハンドルの所持、外部からの世代・評価ID、JSON DTO、事前検査の`ReadyEvidence`を権限として扱いません。手動構築用のハンドルはイベント用ハンドルに変換できません。差分構築はP3 `BuildGuardHandle`の基底・対象の双方と同じSource `build_fence_seq`を使い、P7の対象識別情報の登録だけをP7トランザクションに合流させます。

P1-B01の`BodyUnitManifest` / `BodyCoverageArtifact` / `ArtifactReceipt` / `GenerationBundleReceipt`は、**未実装なら実装を待ち**、P1の担当とともに次の一つの形へ揃えます。`search-core::projection_bundle::{SemanticRegistrySnapshot, ProjectionPayloadV1, BundleComponentsV1, composite_digest_v1}`が、提供元に依存しない純粋なエンコード・デコード処理を所有します。既存の`search-projection-memory::generation_digest`は、同じ正規形式のprojection-only v1関数を呼ぶ互換ラッパーにします。型は`ProjectionPayloadV1 { resources: Vec<CompiledResourceProjection>, registry: SemanticRegistrySnapshot }`、`ArtifactDigestCount { digest:[u8;32], count:u64 }`、`BundleComponentsV1 { source_id:SourceId, projection_digest:[u8;32], unit_manifest:ArtifactDigestCount, body_coverage:ArtifactDigestCount, lexical:ArtifactDigestCount, graph:ArtifactDigestCount, profile_set_digest:[u8;32], lexical_schema_version:String }`とし、`composite_digest_v1(&BundleComponentsV1)->Result<[u8;32],BundleCodecError>`はP1の厳密な計算式と参照データを使います。`SemanticRegistrySnapshot`は現行アプリケーションの定義を一つだけCoreへ移して再公開します。P1のUnit・網羅範囲の正規形式エンコーダーは、P1-B01が提供する型付きメソッドを再利用し、P7が同名の第二実装を作りません。P1-B01の実装順やモジュールパスが異なる場合は、その時点で本契約とP1計画を同一の型に明示的に揃え直してから、P7-04のREDに入ります。未加工の`serde_json::Value`、JSONBのバイト列ハッシュ、既存メモリーストアの成功値だけを、永続READYの証明に流用しません。

P7-04の`StoredBundleV1 { key, source_snapshot, manifest: ProjectionGenerationManifest, projection: ProjectionPayloadV1, unit_manifest: BodyUnitManifest, coverage: BodyCoverageArtifact, receipt: GenerationBundleReceipt }`は、`dto_version='v1'`のタグ付き・`deny_unknown_fields`付きDTOとして全内容を復元します。`validate_stored_bundle_v1(&StoredBundleV1)->Result<ValidatedPayloadV1,BundleError>`は、定義データ・Source・キー・件数、全Unit.textのSHA、Unit・網羅範囲・プロファイル・投影・複合ダイジェストの独立した再計算を行いますが、**Graphと字句検索の実体を検証する前にREADYハンドルを発行しません**。`LexicalSealV1 { key, schema_version, analyzer_version, logical_digest, searchable_doc_count, unit_seal_digest, unit_seal_count, tree_digest, index_relpath }`は、実索引の再オープンと列挙から得ます。P7-08は、`GraphReceiptMappingV1::validate(p1_staged_input, p3_rows, p1_graph_receipt, p3_graph_receipt)`により、一つの正規形式Graphレコード集合から二方式で再計算し、P3 `GraphGenerationReceipt`のキー・スナップショット・対応付け・スキーマ・リソース件数・関係件数と照合します。P1の入力ダイジェストに、P3の内容ダイジェストをコピーしません。

<a id="common-task-protocol"></a>
### 全タスク共通の進め方

各タスクでは指定された試験を**先に**追加し、実PostgreSQL `postgres:18.6-bookworm`、独立した`PgPool`と必要な別プロセス、実際のbuilder/coordinator/reader/GCロールで、意図したREDを保存します。最小実装後に、同じ対象限定コマンドと当該クレートの`cargo clippy --locked -p <crate> --all-targets -- -D warnings`、`cargo fmt --all -- --check`を通します。独立した読み取り専用レビュアーが、SQL制約の迂回、型、ロック順、失効・権限範囲・保持条件を判定します。1タスクごとに全CIを回しません。対象全体が揃った後に`mise run verify:fast`と、必要な最後のホスト側の当該headに対するゲートを一度行い、失敗した範囲だけを追加検証します。各検証記録には、ブランチ・head、移行チェックサム、ロール、テストデータ・索引のハッシュ、試験コマンド・結果、既知の未接続ゲートを明記します。マージ・デプロイ・本番データベース移行は、別指示で行う操作です。

<a id="tasks--graph-非依存の局所-substrate"></a>
## Graphに依存しない共有基盤の局所的な実装タスク

<a id="p7-01--global-ownership-と-legacy-移行拒否"></a>
### P7-01 全体の所有権と既存データの移行拒否

**対象ファイル：** `0002_search_source_ownership_v1.sql`; `tests/source_ownership_migration.rs`。`0001`は変更しません。

**インターフェース：** `search_source_ownership(source_id PK,tenant_owner_key,source_kind DOCUMENT|REMOTE,registration_revision,visibility_revision,activation_epoch,state ACTIVE|TOMBSTONED,registration_dto,registration_digest,created_at,updated_at)`は、`(source_id,tenant_owner_key)`にもUNIQUE制約を持ちます。所有者・種別・SourceIdは、削除済みを示すtombstoneになった後も変更できません。`search_registration_serial`は、全体をロックする一行と、`document_deployment_revision/document_desired_set_digest`、`remote_deployment_revision/remote_desired_set_digest`の二組を持ちます。各組は初回の突合せ前だけ両方NULL、それ以降は正数の改訂番号とダイジェストが両方NOT NULLとなる完全なCHECK制約を設けます。本番用の生成処理は、両組が確定するまで起動しません。既存Sourceに、NULLを許す所有者・改訂番号・有効化状態と`registration_active=false`を追加し、Searchイベント処理記録に`bundle_version`を追加します。既存の処理記録の版をダイジェストから推測しません。所有権だけを差し替える、またはSource側の有効状態・改訂番号・有効化状態だけを変える直接DMLは、トリガーとロールで拒否します。同じ登録トランザクション内で両行が一致する場合だけを許します。

- [ ] RED：`cargo test -p search-runtime --locked --test source_ownership_migration -- --test-threads=1`で、Domain `0009`とSearch `0001`の独立台帳、`0002`の順序、二つの名前空間の改訂番号、種別のCHECK、所有者・種別のUPDATE/DELETE拒否、旧currentや所有権不明の行を黙って再割当てしないことを試します。`tenant_owner_key`は既存の`TenantId`と同じく、非空・前後の空白除去済み・制御文字なし・UTF-8最大256 bytesとします。登録DTOは許可された版のみで最大65536 bytes、ダイジェストは`sha256:`に続く小文字16進数に限定します。既存の所有者・種別・改訂番号・全世代の対応付けを、ホストの永続的な正本から証明できない場合は、起動・既存行の補完に失敗させます。DBの一部だけの更新や、ポインターのNULL化を許しません。
- [ ] GREEN：移行と事前走査、明示的な既存行補完トランザクションを実装します。ホスト設定による所有権の証明がない本番構成は起動を拒否し、全既存Source・キーの照合と、外部キー・トリガーの検査が済むまで、取得・読み取り・公開を有効にしません。Search `_sqlx_migrations`とDomain台帳のチェックサム・順序を実DBで再確認します。

<a id="p7-02--complete-desired-全集合と-source-lease-current-gate"></a>
### P7-02 望ましい登録状態の完全集合とSourceリースの現在状態確認

**対象ファイル：** `src/source_registration.rs`, `src/source_lease.rs`, `tests/source_registration.rs`。アプリケーション型はP4-02の専用担当期間で扱います。

**インターフェース：** P5改訂2の`reconcile(&CompleteDesiredRegistrations)` / `is_current(&SourceRegistration,RegistrationActivation)`を使います。信頼された構成起点が持つホスト所有の、**同じ改訂番号・全テナント・当該名前空間の完全なスナップショット**と、キー・DTOの厳密な一致を二度照合します。`RegistrationSetDigest`は、名前空間・版の区切り（Remoteは`remote-desired-set:v1`）、UUID昇順、固定タグ・順序・長さフレーム、サーバー所有の全DTOフィールドから計算します。DBの直列化行を最初にロックし、SourceId順のSource行、SourceId順の所有権行へ進み、コミット直前にホストの改訂番号・ダイジェストを再確認します。Document/Remoteは同じ直列化行・台帳を使い、tombstoneの対象は名前空間で限定します。DTO・可視性・改訂番号の変更、削除、再有効化では、有効化状態と`fence_epoch`をオーバーフロー検査付きで進め、リーストークンを失効させますが、currentのキー・二つのダイジェスト・改訂番号は変えません。P6-S02の取得処理は、ACTIVEで、所有者・有効化状態の結び付けが成立する行だけを条件付きUPDATEします。

- [ ] RED：`remote_reconcile_never_tombstones_document`, `partial_tenant_map_cannot_tombstone_foreign_remote`, `document_remote_same_source_id_is_rejected_after_tombstone`, `partial_or_stale_remote_desired_set_is_atomic_failure`を実施します。同じ改訂番号で異なるダイジェスト、古い改訂番号、同じ改訂番号・同じ対応表での冪等性、別テナント・種別による再利用、二つのプロセスによる同時突合せ、リース失効、取得・更新のコミット応答不明時にoutboxをclaimしないこと、カウンター・BIGINTのオーバーフロー、所有権証明の欠落を、同じコマンドで試します。`cargo test -p search-runtime --locked --test source_registration -- --test-threads=1`で、まずREDを確認します。
- [ ] GREEN：一トランザクションの台帳アダプターと、`is_current`による全DTO・現在状態の確認を実装します。P4のactorに見えるレジストリーの既存発行処理を再利用し、Document/Remoteの初期突合せが完了するまで四つのルートを起動しません。独立した接続でGREENを確認します。

<a id="p7-03--generationguardpin-schema-と実-role"></a>
### P7-03 世代・guard・pinのスキーマと実ロール

**対象ファイル：** `0003_search_generation_v1.sql`, `sql/roles.sql`, `tests/generation_schema.rs`。

**インターフェース：** `search_generation_identity`は、`(source_id,generation_id)`を恒久的な主キーとし、所有者・有効化状態とともに変更不能にしてGC後も残します。所有権の`(source_id,tenant_owner_key)`への複合外部キーと、Source行との一致を検査するトリガーで、旧所有者の混入を拒否します。有効化状態は過去値を保存し、再登録後の現行値を外部キーで上書きしません。

`search_generation`は、同じ複合キー、`BUILDING|READY|FAILED|DELETING`、`build_kind FULL|INCREMENTAL`、変更不能な`stage_origin EVENT|MANUAL`、全項目が揃うか全項目NULLとなるイベントID・Sourceエポックを持ちます。FULLのトークン・フェンスも全項目が揃うかNULLとなり、反対側のINCREMENTALではNULLとします。Sourceスナップショット、版付きの`projection_manifest`・ダイジェスト・resource_count、有効化状態、バンドル版、ready_atも持ちます。FULL対象の4列にUNIQUE、guardトークンと`(source_id,build_fence)`にもUNIQUEを設け、完全構築guardから`(source_id,target_generation_id,guard_token,build_fence)`へ複合`ON DELETE RESTRICT`外部キーを張ります。

`search_generation_payload`は、キー・種別ごとに`projection|unit_manifest|body_coverage`の各一行を持ち、`dto_version,payload JSONB,logical_digest,logical_count`を保存します。`search_generation_receipt`は、P1の投影・Unit・網羅範囲・字句検索・Graph入力・プロファイル・複合値のダイジェストと件数、字句検索のスキーマ・アナライザー、Graphのバックエンド・スキーマ・対応付け・内容ダイジェストとリソース件数・関係件数、版付きの整合性メタデータDTO、任意のVector整合性メタデータを保持します。`search_lexical_artifact`は、キーから導く相対パス、形式・スキーマ、ファイルツリー・実文書・Unit封印のダイジェストと件数、finalized_atを持ちます。`search_evaluation_lease`は、`(source_id,lease_id)`主キー、世代への外部キー、評価ID・二つのダイジェスト・DB上の失効時刻に加え、所有テナント、`actor_scope_ref`、登録・可視性・アクセスの改訂番号を保存します。全子行にはキーの複合外部キー`RESTRICT`を設けます。Sourceのcurrentは`(source_id,current_generation_id)`外部キーを持ちますが、過去のSearchイベント処理記録には世代への外部キーを付けません。

- [ ] RED：`cargo test -p search-runtime --locked --test generation_schema -- --test-threads=1`で、キー・テナント・有効化状態・種別・生成元・トークンの完全な制約を検査します。誤ったキーへの外部キー、参照先のないcurrentの移行、READY→BUILDINGやREADYの子行DML、guardの直接再発行、pinの権限範囲フィールドのUPDATE、恒久識別情報のDELETE・再利用を拒否します。`actor_scope_ref`は非空・前後の空白除去済み・制御文字なし・UTF-8最大256 bytes、改訂番号は正数かつBIGINT範囲内、二つのダイジェストは`sha256:`に続く小文字16進数とします。実際の`search_registration`、`search_builder`、`search_coordinator`、`search_reader`、`search_gc`ロールを別接続で使い、登録側のポインター直接DML、構築側の親・guard・ポインター直接DML、読み取り側の全DML、GC以外のDELETEを拒否します。PostgreSQLの行ロックに必要なUPDATE権限は、限定したcoordinatorと`SECURITY DEFINER`トリガー（固定`search_path`、PUBLIC EXECUTEの取消）で満たし、builderへ親行の無制限UPDATE権限を与えません。
- [ ] GREEN：NULLを許す既存ポインター・処理記録の監査と、必要な明示的補完を先に完了させます。証明できない旧ポインターがある場合は、移行・起動を停止します。CHECK・外部キー・変更不能性のトリガー・実権限を実装し、builderによる子行DMLは、親がBUILDINGであること、完全構築guardのトークン・フェンス、`clock_timestamp()`で未失効であることを、トリガーでも再検査します。例外はGCロールによるDELETINGの子行DELETEだけです。

<a id="p7-04--typed-full-payload-と純粋-canonical-再計算"></a>
### P7-04 型付きの全保存データと純粋な正規形式の再計算

**対象ファイル：** ファイル対応表のP7-04行にあるCore・メモリー・ランタイムのエンコード処理と、`tests/bundle_durability.rs`。P1-B01の型付きDTOと参照データが存在することが前提です。

**インターフェース：** `validate_stored_bundle_v1(&StoredBundleV1)->Result<ValidatedPayloadV1,BundleError>`を使います。投影データには全`CompiledResourceProjection`と`SemanticRegistrySnapshot`、Unit一覧データには全`BodyItemEntry`とUnit.text、網羅範囲データには同じ正本の項目集合を格納します。版付きの型付きDTOをSQL JSONBに保存・復元し、`deny_unknown_fields`とサイズ・件数の上限を設け、重複・余分・欠落・非正規値を拒否します。投影だけのダイジェストは、既存`search-projection-memory::generation_digest`とバイト単位で同じ結果になる純粋なCore処理で再計算します。Unit・網羅範囲・プロファイル・複合値は、P1エンコーダーで**復元したDTOを入力にして**再計算します。`ArtifactReceipt.key`、一覧のSource・キー・スナップショット・件数、全Unit.textのSHA、Supported/PartialのUnitと網羅範囲の欠落、Unsupported/FailedのUnitが0であることを照合します。Archive Partの共通の結び付けは、複数の末端形式を許す`archive_inner_format=None`を維持し、各Unitの`Some(leaf)`・メンバーチェーン・位置情報・プロファイルを検証します。Partialの肯定的な根拠となるUnitは保存できますが、網羅範囲の欠落は残し、否定結果の完全性を主張しません。

- [ ] RED：`cargo test -p search-runtime --locked --test bundle_durability -- --test-threads=1`で、P1 composite v1の参照データ、本文だけの変更で投影専用ダイジェストは不変・複合ダイジェストは変化すること、全データを保存して新プロセス・別接続から同じキー・ダイジェストを得ること、JSONBの物理バイト順に依存しないことを確認します。未知の版・フィールド、欠落、余分、重複、不正な件数・Unit.textは安全側に倒して拒否します。メモリー側のprojection v1との同値性と、`Retryable`・保持条件による拒否も確認します。
- [ ] GREEN：純粋なエンコード・デコード処理と、型付きデータのランタイムでの保存・復元を実装します。`search-generation`のREADY・公開はまだ呼ばず、`ValidatedPayloadV1`だけを返します。レビュアーがP1-B01/B02との型の一致を確認します。

<a id="p7-05--file-backed-lexical-の-durability-と双方向-unit-seal"></a>
### P7-05 ファイル型字句検索索引の永続性とUnitの双方向整合性封印

**対象ファイル：** `src/lexical_artifact.rs`, `tests/lexical_artifact.rs`。P1-E01が、実際に検索可能なUnit文書を列挙できることが前提です。

**インターフェース：** `finalize_lexical(key, staged_dir, expected_p1_lexical_receipt, unit_manifest)->Result<LexicalSealV1,...>`と、`reopen_and_validate_lexical(key, saved_row, unit_manifest)->Result<LexicalSealV1,...>`を使います。最終ディレクトリは、信頼された索引ルートとキーだけから導きます。仮置きファイルとディレクトリをfsyncし、変更不能な最終パスへ原子的に名前変更した後、親ディレクトリをfsyncします。再オープンした実索引の全Resource/Unit文書から、P1の論理的な字句検索ダイジェスト・件数・スキーマ・アナライザーと、整列済みのファイル名＋サイズ＋SHA-256によるファイルツリーのダイジェストを算出します。全Supported/Partial Unitと、実際に検索可能なUnit文書の親・Part・raw・位置情報・本文を双方向に一対一で照合します。Unsupported/Failedの文書は0とします。

- [ ] RED：`cargo test -p search-runtime --locked --test lexical_artifact -- --test-threads=1`で、構築側の自己申告だけの一致、Unit文書の欠落・余分・重複・本文差替え、別キーのパス、名前変更前後の異常終了、ツリー内ファイルの破損を拒否します。P1の実索引APIを使い、メタデータだけの模擬実装で整合性封印を成功扱いしません。
- [ ] GREEN：変更不能なファイルのライフサイクルと、DB上の成果物行を実装します。ファイルとDBは原子的にコミットできないため、READY前・CAS前・pin後かつ返却前に再確認し、コミット前にはファイルを削除しません。参照を失ったファイルの回収は、DBの状態・キー・guardを確認し、コミット後だけに行います。

<a id="p7-06--eventmanual-target-登録と-full-guard-fence"></a>
### P7-06 EVENT/MANUALの対象登録と完全構築guardのフェンス

**対象ファイル：** `src/{generation_registration,full_guard}.rs`, `tests/full_guard.rs`。

**インターフェース：** EVENTは、ロックしたoutbox行→Source行から、ルート・イベントID・リーストークン・Sourceエポック・失効時刻を取得します。MANUALはSourceから開始します。所有権がACTIVEであることと、有効化状態・保持条件・スナップショットを照合し、恒久識別情報のINSERT → P7のBUILDING対象のINSERT → 保存済み対象のトークン・フェンスに一致する完全構築guardのINSERTを、一トランザクションでコミットします。`build_fence_seq`はSource行の下で一度だけ増やし、オーバーフローを拒否します。非公開ハンドルのフィールドは外部へ発行しません。`renew_full_guard`、子行バッチ、READYは、保存済みの結び付けとDB時計上の未失効を毎回再検査し、失効したguardを同じ対象へ再発行しません。差分構築では、最初のコピーより前に、対象識別情報とP3の基底・対象guardを同じ登録トランザクションに結び付けます。

- [ ] RED：`expired_full_guard_rejects_late_child_write_and_ready`, `full_target_guard_cannot_be_reissued`、誤ったイベント・ルート、MANUAL→EVENTハンドルの偽装、guard INSERT失敗・キー衝突・フェンスのオーバーフロー時の全ロールバックを試します。`cargo test -p search-runtime --locked --test full_guard -- --test-threads=1`を使い、独立接続と期限境界の同期バリアでREDを確認します。
- [ ] GREEN：Graph未接続の場合も、対象＋guardの登録・失効・削除候補の検査までは実装しますが、Graph READYと公開は閉じたままにします。Graph接続時の、一コミットでの親行登録はP7-07で拡張します。

<a id="tasks--p3-native-qualification-後だけ着手"></a>
## P3の実バックエンド資格取得後だけに着手するタスク

<a id="p7-07--graph-parent-の唯一の一接続登録入口"></a>
### P7-07 Graph親行を一接続で登録する唯一の入口

**対象ファイル：** ファイル対応表のP7-07行。P3-P04のGO、選定済みPostgreSQLバックエンド、P3-G01/G03/G04/C01の本番用シグネチャ・ロール改訂が前提です。別のバックエンドなら本タスクは実行せず、手順を再設計して独立レビューします。

**インターフェース：** 非公開の`GraphRepository::register_full_on(&mut PgConnection, &RegisteredTarget)->Result<RegisteredFullBuildHandle,GraphError>`は、P7 coordinatorのEVENTまたはMANUALと**同じ**トランザクション内だけで、GraphのBUILDING親行を登録します。Source・キー・スナップショット・有効化状態・トークン・フェンスをP7の保存済み対象と一致させ、内部構造を公開しないハンドルを返します。Graph親行→P7対象の複合外部キー、変更不能な結び付け、子行変更のトリガーは、P3-G03の未適用移行で一度に確定します。適用済みの場合は、新しいGraphの追加移行で加えます。`DurableGraphGenerationPort::stage_full_registered(handle,resources,relations)->BoxFuture<GraphStage>`は、登録済み親行の**子行バッチだけ**を書き、各バッチがP7/Graphの親行・guard・失効時刻を再読します。旧`stage_full(manifest,...)->GraphStage`による親行の自律INSERTは、隔離PoC・テスト用データに限定し、本番の構成起点・ロールには公開しません。差分構築は、P3で凍結した基底・対象guardと同じ対象登録・接続を使います。

- [ ] RED：`full_registration_is_one_commit_with_guard_and_graph_key`, `graph_stage_failure_leaves_no_visible_unprotected_target`, `isolated_stage_full_cannot_publish_without_p7_guard`を、`cargo test -p search-runtime --locked --test graph_registration -- --test-threads=1`で、実ロール・別接続・障害注入を使って試します。Graph INSERT・guard INSERTの障害後に、P7/Graph/恒久識別情報の可視状態を照合します。トランザクションがロールバックした場合、恒久識別情報は未作成です。
- [ ] GREEN：一接続API、Graph親行の複合外部キー・変更不能な結び付けとDMLトリガー、本番ロールの親行INSERT禁止を実装します。ハンドルの所持だけによる認可を拒否し、GREENを確認します。

<a id="p7-08--実-payloadlexicalgraph-から-ready-を一-commit"></a>
### P7-08 実際の保存データ・字句検索・GraphからREADYを一コミットで確定

**対象ファイル：** ファイル対応表のP7-08行。P1-B01/B02、P7-04/05、P3-G02/G08の整合性メタデータ・エンコーダーが前提です。

**インターフェース：** `validate_ready(handle:&BuildHandle)->BoxFuture<VerifiedBundle>`は、Sourceスナップショット、保持許可、全DTO、変更不能な字句検索の最終ディレクトリとその再オープンを事前検証します。短いトランザクションで対象→guardをロックし、トークン・フェンス・DB上の失効時刻、P7の保存データ・整合性メタデータ、P3 Graphの行・整合性メタデータを同じ接続で再検証します。

P1-B02の`canonical_graph_staged_input_v1(typed_relations,document_owner_mapping)`と、P3-G02の`canonical_graph_digest(source,schema,resources,relations)`は、同じ復元済みの型付きレコード集合から**別々に**計算します。それぞれ仕様化した未加工のバイト列と固定の参照データを、P7-08着手前に揃えます。P1エンコーダーは型付きの多項関係とDocument所有者への対応付けを、P3エンコーダーはSource・スキーマ・全Graphリソース・関係を扱います。P3のダイジェストをP1の欄へ転記しません。`GraphReceiptMappingV1`は、キー・スナップショット・対応付け・スキーマ・リソース件数・関係件数を照合し、`graph_input_count`は添付の重複を除いた後の一意な関係数とします。P1 composite v1をCoreエンコーダーで再計算し、P3 Graph READYの実体を確認した同じ接続で、P7 READY＋整合性メタデータを一コミットで確定します。READY後の全子行は変更不能であり、READYだけではcurrentになりません。

- [ ] RED：`cargo test -p search-runtime --locked --test ready_bundle -- --test-threads=1`で、P1 compositeの参照データと、P1 Graph入力・P3内容の**両方**の参照データを扱います。P3ダイジェストをP1欄へコピーした値、キー・スナップショット・件数・対応付けの不一致、字句検索索引の消失、Unit本文の破損、READY中の子行更新・guard期限切れを拒否します。資格未取得のGraphでは、READY成功の正常系試験が通らないことも検査します。
- [ ] GREEN：型付きの`GraphReceiptMappingV1`、同一接続でのREADY、DB外のファイルの事前確認・再確認を実装します。PostgreSQLとファイルの原子性を仮定せず、後続のpin・読み取りでも再検証します。

<a id="p7-09--event-originmanual-cas-と-search-receipt"></a>
### P7-09 イベント由来の確認、手動CAS、Searchイベント処理記録

**対象ファイル：** ファイル対応表のP7-09行。P6-S03/S04の本番型・接続処理の変更を、本タスクと直列化します。

**インターフェース：** `PublishCandidate`は、outbox → Source → P7/Graph世代のキー順 → guard → リースID順 → Searchイベント処理記録の順でロックし、DB行からイベント・Sourceエポックを再取得します。候補の`stage_origin='EVENT'`、保存済みのイベントID・エポック・スナップショット・有効化状態・guard、同じキーのREADY・二つのダイジェストと、期待するポインター・改訂番号を再照合します。ポインター＋`pointer_revision`＋`last_published_epoch`のCAS、`(source_id,event_id)`のSearchイベント処理記録（バンドル版と両ダイジェスト、単調なエポック）、guard DELETEは一コミットにします。

`ReuseCurrent`は候補ハンドルを受け取らず、currentのキー・二つのダイジェスト・改訂番号と、実際のREADY・権限の正本・保持条件だけを再検証します。手動公開はSourceから始め、同じCAS/READY条件で公開し、保留中のイベントを確認応答しません。CASに負けた場合はguardを保持します。汎用の確認応答は、別のフェンス付きトランザクションです。コミット応答が不明な場合は`CompletionUnknown`とし、再接続後のcurrent・処理記録・フェンスの再読と、後続配送だけで収束させます。

- [ ] RED：`wrong_event_candidate_cannot_publish_or_write_receipt`, `old_epoch_or_manual_candidate_cannot_complete_event`, `reuse_current_requires_current_ready_after_gc`を実施します。同じエポックで異なるキー・ダイジェスト・版、古いエポック、確認応答の結果不明、公開コミットの応答不明、CASの敗北を、`cargo test -p search-runtime --locked --test event_completion -- --test-threads=1`の別接続・同期バリアで検査します。過去の処理記録だけがある場合や、GC済みのキーは成功になりません。
- [ ] GREEN：P6-S01と索引作成側の呼び出しを一緒に`CompleteEventRequest`の直和型へ移し、P6-S03がP7の非公開・トランザクションに結び付いたAPIだけを使うようにします。再配送で二重公開が起きず、Search側が`delivered_at`を更新しないことを確認します。

<a id="p7-10--actor-scope-pinrenewreturnrelease"></a>
### P7-10 actorの権限範囲に結び付いたpinの取得・更新・返却・解放

**対象ファイル：** ファイル対応表のP7-10行。

**インターフェース：** `pin_current`はSource行のロック前に字句検索を暫定的に事前検査し、Source → 世代 → guard → リースの短いトランザクションで、**その時点の**current・有効化状態・所有権ACTIVE・二つのダイジェスト・P3 READYを確認して、リースをINSERTします。所有テナント、ホスト発行の`actor_scope_ref`、登録・可視性・アクセスの改訂番号、評価、キー・定義データ・バンドル、DB上の失効時刻を保存し、変更不能にします。ホスト参照は、再起動後も同じactor・セッション・評価範囲へ再解決できる必要があり、不明な場合は安全側に倒して拒否します。

更新・結果返却・actorによる解放には、pinと、現在の`TrustedDiscoveryBinding`＋`AuthorizedSourceScope`の両方を要求し、DB行とP4/P5の現在状態のゲートを再照合します。Graph読み取りは`REPEATABLE READ READ ONLY`のスナップショットを使い、返却前にはDB時計に基づく新しいトランザクションを開きます。返却時は、項目・フィールド・Graph参加要素、Documentの現在のVersion・Part・raw・Readまで再判定し、条件を満たさなければ結果全体を返しません。旧キーのpinは、ポインターが前進しただけでは失効しません。

- [ ] RED：`pin_cannot_transfer_between_actor_scopes`, `registration_or_visibility_change_revokes_old_pin_read`, `foreign_tenant_cannot_renew_same_source_pin`を実施します。`cargo test -p search-runtime --locked --test pin_scope -- --test-threads=1`に加え、処理中の期限切れ・旧ポインターのpin、別プロセスのpin↔公開、再起動後に権限範囲参照を解決できない場合、readerロールの直接DML拒否を試します。権限を失ったactorによる解放を成功扱いせず、coordinatorのGCだけが失効リースを掃除します。
- [ ] GREEN：同一接続でのpin取得・更新・検証・解放と、P4/P5の現在状態のゲートを実装します。pinは旧キーを保持し、返却直前まで同じキー・整合性メタデータと、現在の権限の正本を再確認します。

<a id="p7-11--guarded-gc-と-fk-safe-cleanup"></a>
### P7-11 guardを尊重するGCと外部キー制約を守る削除

**対象ファイル：** `src/gc.rs`, `tests/gc_races.rs`。

**インターフェース：** `retire_unpinned`、`discard_unpublished`、失効guardの削除処理は、current、有効なpin、完全構築guardとP3の基底・対象guardを確認します。Source → 整列済み世代 → guard → リースの順でロックした後、未公開でpinのない対象を、一トランザクションで次の順に処理します。DELETINGへの変更 → **guard DELETE** → 失効リースDELETE → Graph参加要素・関係・リソースの削除 → P7子行の削除 → Graph/P7親行DELETEです。恒久識別情報と過去のSearchイベント処理記録は残します。完全構築と差分構築は同じSource `build_fence_seq`名前空間を使い、トークン・DB上の失効時刻・保存済みの結び付けで判定します。失効した対象へguardを再発行しません。ファイル削除は、コミット後の冪等な孤立ファイル回収だけで行います。

- [ ] RED：`guard_delete_then_child_failure_rolls_back_full_target`、current・pin・guardがあるキーのGC拒否、別プロセスのpin↔公開↔GC、基底・対象コピー↔GC、失効guardの再発行拒否、実際の外部キーエラー`23503`が起きない正常な削除処理を、`cargo test -p search-runtime --locked --test gc_races -- --test-threads=1`で試します。guard DELETE後の子行障害では同じトランザクションをロールバックし、別接続からguard・対象の双方が残ることを確認します。
- [ ] GREEN：P3のトランザクションに結び付いた子行削除を、同一接続で実装します。Sourceを取得しないコピー・検証バッチは世代→guardだけをロックし、後からSourceを取得しません。READYの基底世代は、対象削除後の別GCトランザクションでだけ退役させます。

<a id="p7-12--restartcorruption別-db-restore-と統合受入"></a>
### P7-12 再起動・破損・別DBへの復元と統合受入

**対象ファイル：** `src/recovery.rs`, `tests/process_restore.rs`。本計画の最後にだけ実施します。

**インターフェース：** 起動は、移行・ロール・完全なレジストリー・所有権の走査 → currentの全P1データ・定義データ、実際の字句検索ツリー・文書、P3 Graphの対応付け・内容、バンドル版・複合ダイジェストの再検証 → API/claimの有効化、の順に行います。中断したBUILDING、失効guard・リース、結果不明の確認応答・コミットは、DB時計とフェンスで回収し、有効なguardは削除しません。差分構築のコミット済みカーソルや対象全体のダイジェストを証明できない場合は、同じキーで再開せず、新しいキーで完全再構築します。破損やDB外のバイト列の喪失がある場合は、そのキーを利用不能にし、新しいキーでSource正本から再構築します。旧pinを別キーへ暗黙に移しません。`pg_dump -Fc`/`pg_restore`は**別の使い捨てDB**を使い、変更不能な字句検索ディレクトリも別途復元して、同じキー・ダイジェスト・実クエリーを確認します。RAM上のRemote世代・セッション・カーソルは復元しません。

- [ ] RED：`cargo test -p search-runtime --locked --test process_restore -- --test-threads=1`で、強制終了・再起動（構築中、公開コミット前後、確認応答前）、行・字句検索ファイル・ダイジェストの欠落や破損、コミット結果不明、古い世代、別DB復元時の索引バイト列の欠落・復元、current・pin・guard・GCの競合を、独立したプロセス・接続で注入します。DBだけを復元して索引がない場合は、準備完了ではありません。旧所有権行の補完失敗と、同じテナントを再有効化した際の旧権限範囲の失効も回帰試験します。
- [ ] GREEN：復旧・掃除と、新しいキーでの再構築を最小実装します。P6汎用配送、P3の実バックエンド資格判定、P1の本文・字句検索、P4/P5の現在の権限の正本について、各独立した検証記録を照合してから、共有永続化基盤の受入を判定します。最終の`mise run verify:fast`と、必要なホスト側の当該headに対するゲートの結果は別に記録し、最終P7のHTTP・運用・SLO・保持と送出の寿命管理の完了とは区別します。

<a id="review-focus-and-acceptance-ledger"></a>
## レビューの重点と受入証拠の対応

| 失敗しやすい入力・競合 | 担当する試験 |
| --- | --- |
| 全テナントを含まないRemoteの望ましい登録状態の対応表、同じ改訂番号で異なるDTO、Document/RemoteのSourceId衝突 | P7-02の所有権に関する4つの指定試験 |
| 別イベント・古いSourceエポック・MANUAL候補と、GC済み世代の過去のSearchイベント処理記録 | P7-09のイベントに関する3つの指定試験 |
| 完全構築guardの期限境界、別対象でのトークン再利用、削除途中の障害 | P7-06のguardに関する2つの指定試験と、P7-11の削除に関する1つの指定試験 |
| 別接続のGraph親行、Graph/guardの片方だけが見える状態、隔離された`stage_full` | P7-07のGraphに関する3つの指定試験 |
| pin IDを別actor・テナントへ移すこと、可視性変更、再起動後に権限範囲の参照を解決できないこと | P7-10のpinに関する3つの指定試験とP7-12 |

上記の**実PostgreSQLを使う16の指定ケース**は、改訂1 §6の名前を変更せずに割り当てたものです。各正常系ゲートは実ロール・独立接続を必要とし、型だけ、メモリー上の代替実装、Graphの隔離PoCで代用しません。全タスク完了後、独立した読み取り専用レビュアーが、移行チェックサム・既存データの失敗、SQLロール・トリガーの迂回、正規形式の参照データ、P1字句検索の双方向封印、P3の二つのダイジェストの対応、P6のポインター＋Searchイベント処理記録と別トランザクションの確認応答、pin・current・guard・GCの競合、再起動・別DBへの復元を照合します。共有PostgreSQLの局所受入、Graphの本番資格、P6の縦断検証、最終P7のランタイム・HTTP、当該headのCIは、別々の欄の証拠として報告します。
