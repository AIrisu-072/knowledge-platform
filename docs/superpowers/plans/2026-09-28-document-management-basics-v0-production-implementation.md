# 文書管理基本操作・一覧・履歴参照 v0 — 本番実装計画

> **For agentic workers:** 実装時は `superpowers:executing-plans` または `superpowers:subagent-driven-development` を使用する。実行方法は計画レビュー時に確定する。以下の `- [ ]` は将来の実行チェック欄であり、今回の実行済み記録ではない。

**状態:** **PROPOSED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**。

**Goal / 目的:** 既存の文書コアを再実装せず、T5〜T9、認可付き一覧・履歴・ファイル参照を追加し、将来のCLIとGUIが共用できるApplication契約を完成させる。

**Architecture / 構成:** RustのDomain、Application、PostgreSQL port境界を維持する。認可・業務更新・操作台帳・必須イベントを同じDB確定境界で扱い、純粋な読み取りは認可と可視性を同一statementで評価する。各機能PRで必要なT11/T12を実装し、最後の1PRは横断統合・検証とする。

**Tech Stack / 技術:** 既存Rust workspace（基準mainのRust 1.98 / edition 2024）、PostgreSQL/sqlx、tokio、serde_json、uuid v7、sha2、time、unicode-normalization、testcontainers。今回の計画で新しいproduction dependency、認証製品、検索エンジンを採用しない。

**Spec / 設計:** `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md`。承認記録は同ディレクトリの `2026-09-28-document-management-basics-v0-design-approval.md`。承認対象はPR #15の `0314f91a4e68221ed06778d36eaf228d0adfec86` にある本文（blob `38010802a04c285336810e9b9c637c656ed1a76b`）。原文は承認前表示も含めて保存し、現在の承認状態は承認記録とstatusで示す。

**基準:** `main@55dc3d3a430c8f36e1db8277fee15c4429258466`。設計・計画用PRは #15。実装PRはまだ作成しない。

## 1. 実装開始条件・共通制約

- 書面設計は承認済み。本計画の承認、実行方法の確定、先行する開発ログ一元管理の完了確認、依頼者の実装開始指示が揃うまでは製品コードを変更しない。
- 着手時にAGENTS、active、今回のstatus、設計・承認記録・本計画、mainと対象PRのexact headを再取得する。main差分が本計画の前提を変える場合は差分を先に解決する。
- 既存 `active.md` は準備段階で切り替えない。新機能の実行開始時だけ切り替える。mergeは別の明示指示を必要とする。
- Documentの正本とSearchの派生情報を分離する。通常削除、再公開、Document Diff、HTTP/OpenAPI・CLI・GUI、Windows/ADの実接続、配送worker、Audit Storeは本計画の対象外。
- Versionの永続状態は `WORKING / PUBLISHED / WITHDRAWN`。既存の版同一性、DSI、公開品質、原本不変性、T10の非復活を変更しない。
- 必要なDomain/Audit Outboxの失敗時は業務変更もrollbackする。開発ログは必須業務監査の代替にしない。
- 純粋な一覧取得・先読み・Agent参照では既読にしない。ReadStateは `(identity_provider, principal_id, document_version_id)` と `first_read_at` のみ。
- 権限はallow-only、最も近い明示policyが全体を置換する。明示空policyとinheritを区別し、操作権限間の暗黙包含を設けない。
- ロック順はaccess-state → Folder ID順 → Document ID順 → policy/操作台帳。外部identity取得・ファイル検査・ストリーム送信中に長いDBロックを保持しない。
- T5/T6の実変更および配下にPENDING予約があるFolder移動は取消後。権限剥奪は予約で妨げない。期限到達時に依頼者の現在権限を検査する。
- Folder名はNFC・前後空白除去、255 Unicode scalar values以下、大小文字を区別し、同じ親で一意。rootは既存固定IDを維持する。
- 一覧page sizeは既定50・上限200。未読条件は公開一覧のみ。日時範囲はUTCの開始含む・終了含まない。
- 新migrationの番号は着手時mainで採番する。既存migrationを書き換えない。以下のM-A/M-B/M-Cは実行計画上の論理名でありSQLファイル名ではない。

## 2. レビュー重点

| 失敗条件 | 期待する結果 | 担当 |
|---|---|---|
| 権限剥奪後の古い操作ID・cursor、検査待ち中の権限変更 | 保存済み成功は保持するが現在の開示・実行認可を迂回できない | MB-02/03/09 |
| Folder A→BとB→Aの同時実行、移動配下の明示policy | cycleなし。実効policyが変わる対象だけを全件確認し、不足なら全体rollback | MB-07 |
| 予約依頼者の権限喪失とidentity一時障害 | 前者は公開せず終端化、後者は同じ予約IDで再試行 | MB-04 |
| バイト提供前の監査失敗、公開終了後に新規開始するファイル取得 | 開示しない。access_grantedを送信完了と偽装しない | MB-10 |
| Outbox整理、旧migrationデータ、初版WORKING | 履歴は配送保持期間に依存せず、不明値を捏造しない。既存編集契約を維持 | MB-03/10/11 |

## 3. PR単位・依存順

| 論理PR | Task | そのPRの完成条件 |
|---|---|---|
| A: 認可・イベント基盤と既存経路への適用 | MB-01〜04 | T8、初期化、既存登録・版・公開・予約・T10への認可、各操作の必須記録と実DB試験 |
| B: 文書属性・フォルダ・移動 | MB-05〜07 | T5/T6/T7と必要なイベント・監査・原子性試験 |
| C: 既読・一覧・履歴・ファイル参照 | MB-08〜10 | T9、読取契約、情報漏洩防止、必要な開示監査 |
| D: T11/T12統合 | MB-11 | 全操作の対応表、互換性、冪等性、障害・競合の横断検証を1PRにまとめる |

A→B→C→Dの順。Dで初めて必須記録を追加する運用は禁止。未マージの前段を使う場合はstacked draft PRとし、実際のbase/headをstatusへ記録する。各PRのマージは明示承認後に行い、次PRを最新mainへ追従させる。独立した別システムを同時開発する計画ではない。

## 4. ファイル配置と公開契約

以下は基準mainから追加・変更する場所。略号はD=`crates/document-domain`、A=`crates/document-application`、P=`crates/document-repository-postgres`、S=`crates/document-publication-scheduler`。実行時は必ず完全パスに展開する。

| 責務 | 新規ファイル | 既存の主な変更先 |
|---|---|---|
| 型・policy評価 | D/src/resource_ref.rs、D/src/access_policy.rs、D/src/folder.rs | D/src/lib.rs、D/src/error.rs |
| identity・管理操作・digest | A/src/access_context.rs、A/src/management_command.rs、A/src/management_digest.rs、A/src/management_ports.rs | A/src/lib.rs、A/src/error.rs、A/src/events.rs |
| 認可付き既存操作 | A/src/authorized_document.rs、P/src/authorized_repository.rs、P/src/access_control.rs | A/src/service.rs、A/src/versioning_service.rs、A/src/publication_end.rs、P/src/repository.rs、publish.rs、schedule.rs、versioning_mutation.rs、withdrawal.rs、publication_end.rs |
| T8・台帳・監査 | A/src/access_policy_service.rs、P/src/access_policy.rs、P/src/management_ledger.rs、P/src/targeted_events.rs | P/src/lib.rs、P/migrations（M-A） |
| T5/T6/T7 | A/src/document_management.rs、A/src/folder_service.rs、P/src/document_management.rs、P/src/folder_management.rs、P/src/folder_preflight.rs | P/src/lib.rs、P/migrations（M-B） |
| 予約の認可 | A/src/scheduled_authorization.rs | A/src/schedule.rs、P/src/schedule.rs、S/src/runner.rs、S/src/lib.rs、S/src/main.rs |
| T9 | A/src/read_state.rs、P/src/read_state.rs | P/migrations（M-C） |
| 一覧・履歴・開示 | A/src/document_query.rs、A/src/document_history.rs、A/src/file_access.rs、A/src/query_cursor.rs、P/src/document_query.rs、P/src/document_history.rs、P/src/file_access.rs | A/src/lib.rs、P/src/lib.rs |
| 試験補助 | P/tests/support/management.rs | 既存P/tests/support/versioning.rsは再利用し、無関係な整理をしない |

新規コードは責務ごとに分割し、既存service.rsへすべて追記しない。新しい多数のcrate、汎用workflow/CQRS基盤は作らない。

### 4.1 共通型

次の名称はTask間の契約とする。既存型は既存モジュールから再利用する。

- `ResourceRef = Document(DocumentId) | Folder(FolderId) | AccessPolicy(PolicyId)`。`PolicyId`はUuidのnewtype。`PolicyTarget = Document(DocumentId) | Folder(FolderId)`。
- `Action = Read | ReadHistory | Write | Publish | Administer`。`PolicySubject`はkind、identity_provider、subject_idを持つ。kindはPrincipal/Group/Role。
- `PolicyMode = Inherit | Explicit(Vec<PolicyGrant>)`。`PolicyGrant`はsubjectと重複のないAction集合。Explicit空配列は明示拒否。
- `VerifiedActorContext`はprincipal、検証済みsubject集合、valid_until、HumanInteractive/Agent/Service、必要時のservice_executorを保持する。transport入力のDeserializeや任意boolから生成しない。コンストラクタは信頼済みadapterのassembly用であり、データ自身が認証を証明するという意味ではない。
- `IdentityContextResolver::resolve(&self, principal: &PrincipalRef) -> Future<Output=Result<VerifiedActorContext, IdentityResolutionError>>`。`IdentityResolutionError = Unavailable | InvalidIdentity`。productionにallow-all resolverを付けない。未検証委任は拒否する。
- `ManagementOperationId::try_from_uuid(Uuid) -> Result<Self, ApplicationError>`はUUIDv7のみ。`ManagementCommand`は下記の管理コマンドenum、`ManagementResult`はその結果enum。各コマンドはoperation_id、対象ID、期待revision、非空reasonを保持し、actorは別引数のcontextから取る。
- `ManagementRepository::execute(&self, ctx: &VerifiedActorContext, command: ManagementCommand) -> Future<Output=Result<ManagementResult, RepositoryError>>`。`lookup(&self, ctx: &VerifiedActorContext, operation_id: ManagementOperationId) -> Future<Output=Result<Option<ManagementResult>, RepositoryError>>`も現在の開示認可を行う。
- `ManagementErrorCode = InvalidInput | NotFound | Forbidden | RevisionConflict | OperationConflict | CursorStale | StaleVersion | ReservedDocument | FolderCycle | RootProtected | IdentityUnavailable | CommitOutcomeUnknown | IntegrityViolation`。既存ApplicationError/RepositoryErrorの回復可能性を維持して対応付け、エラー文字列で分岐しない。

### 4.2 新しい操作・戻り値

各Applicationサービスは `async fn operation(&self, ctx: &VerifiedActorContext, command: CommandType) -> Result<ResultType, ApplicationError>` 形とする。下表はoperation名とCommandType/ResultType。結果は操作ID・対象ID・resulting revision・changed・UTC時刻を持ち、参照不能な内容は返さない。

| operation | CommandTypeの固有入力 | ResultType |
|---|---|---|
| update_document_metadata | UpdateDocumentMetadata { document_id, expected_document_revision, set: BTreeMap<String, Value>, unset: BTreeSet<String> } | MetadataUpdateResult（更新後共通属性を含む） |
| move_document | MoveDocument { document_id, from_folder_id, to_folder_id, expected_document_revision } | DocumentMoveResult（旧/新Folder ID、access_revision） |
| create_folder | CreateFolder { folder_id, parent_folder_id, expected_parent_revision, name } | FolderMutationResult（作成時revision=0） |
| rename_folder | RenameFolder { folder_id, expected_folder_revision, name } | FolderMutationResult |
| move_folder | MoveFolder { folder_id, from_parent_id, to_parent_id, expected_folder_revision } | FolderMutationResult（access_revision） |
| set_access_policy | SetAccessPolicy { target, expected_policy_revision, mode } | PolicyMutationResult（安定Policy ID、policy/access revision） |
| mark_version_read | MarkVersionRead { document_id, document_version_id }。actor/他人のprincipal/汎用operation_idは入力しない | ReadStateResult { principal, document_version_id, first_read_at, inserted } |

`BootstrapRootPolicy`は通常ManagementCommandに含めない。信頼済みbootstrap主体と明示grantsを専用portで受け、未設定rootを一度だけ初期化し監査する。通常T8からroot置換・回復はできない。

### 4.3 読み取り契約

`DocumentQueryService`は `list_published_documents` / `list_authoring_documents` / `list_history_documents` を別methodで公開する。引数はctxとそれぞれのQuery型、戻り値は `Page<PublishedDocumentSummary>` / `Page<AuthoringDocumentSummary>` / `Page<HistoryDocumentSummary>`。`Page<T>`はitems、next_cursorだけを必須とし、無許可件数・総件数は返さない。

`DocumentListFilter`はtitle_contains、folder_id、include_descendants、document_type、owning_department、category、created_from、created_before。PublishedQueryだけがunread_onlyを持つ。A/src/query_cursor.rsの `validate_page_size(value: u16) -> Result<u16, ApplicationError>` は1〜200のみを許し、省略時は呼出側で50を適用する。page_sizeとcursor、sortは各Queryで持ち、history/authoringはPublishedAt sortを受け付けない。

`list_child_folders(ctx, FolderPageQuery) -> Page<FolderSummary>`、`list_document_versions(ctx, VersionPageQuery) -> Page<VersionSummary>`、`list_document_history(ctx, HistoryPageQuery) -> Page<DocumentHistoryEntry>`、`get_document_version(ctx, VersionRequest) -> VersionDetail`、`list_version_files(ctx, VersionRequest) -> Vec<VersionFileSummary>`、`open_version_file(ctx, VersionFileRequest) -> OpenedVersionFile`を用意する。すべてasync/ResultでApplicationErrorを返す。

`VersionRequest`はDocument ID、Version ID、Published/Authoring/Historyのtyped purpose。purposeは権限ではなく、常に対象状態と必要権限を検査する。`VersionFileRequest`はこれにContentItem IDとRepresentation IDを追加する。`OpenedVersionFile`は既存ContentReader、MIME、サイズ、安全な表示名、access_grantedのaudit event IDを持ち、Storage locatorを含めない。

### 4.4 永続化と予約との整合

M-Aは `document_access_state`（単一行id=1、access_revision=0）、`access_policy_bindings`、`access_policy_grants`、`document_management_operations` と監査対象種別の追加を含める。bindingはPolicy ID、FolderまたはDocumentのFK（片方だけ非null）、mode、revisionを保持する。資源ごと一意。未作成bindingの期待revisionは0、最初の明示設定で1。inheritへ戻してもPolicy IDとrevision履歴を保持する。root初期化も同じbinding表を使う。

M-BはFolder名の一意性と必要index、M-CはReadStateの複合PKとVersion FKを含める。既存データに推定ACL、架空利用者、既読の一括値を付けない。既存Documentイベントのpayload/name/IDは保持し、新対象をDocumentIdに偽装しない。既存event recordから型付き記録への互換adapterを設け、保存・戻し読みの両方を試験する。

管理台帳は操作kind・型付きresource・actor・期待revision・command digest・changed/unchanged・結果JSON・結果revision・occurred_atを保持し、v0でTTLを設けない。新規no-opも台帳に残すがmutationイベントは0件。Folder作成は親revisionを検査し、作成したFolder revisionだけを0で開始する。T8はDocument revisionを更新しない。

認可付きRepositoryは、既存の業務transaction内部でaccess guardを先に取得する。Applicationで認可した後に無認可Repositoryを呼ぶだけの実装は不可。ファイル検査前の早期拒否は可能だが、確定transaction内の再認可を省略しない。純粋な読み取りはread-policyとVersion可視性を単一SQLで評価する。

### 4.5 digest・cursor・入力上限

本節の技術上限は本計画レビューの対象。reasonはtrim後非空・UTF-8で1024 bytes以下・制御文字拒否。metadata patchはUTF-8 JSONで64 KiB以下、extensionsの最大入れ子深さ16。対象キーは設計どおり4つだけ。任意の既存未知キーは保持する。

digestは `SHA256(b"document-management-basics-v0\0" + canonical_json(identity))`。identityはschema_version=1、operation_id、operation_kind、型付き対象・期待revision・正規化済みpayload、actorのprovider/ID、invocation kind、reasonから成る。membership・有効期限・再試行時刻・request traceは含めず、別途現在認可する。objectキーはUTF-8辞書順に再帰sort、array順序保持、空白なし、string/numberはserde_jsonの同じ固定workspace版の表現を使う。数値表現を勝手に浮動小数へ変換しない。

canonicalizerの固定vector: `{ "z":2, "a":{"y":true,"x":"あ"} }` → `{"a":{"x":"あ","y":true},"z":2}`。このvectorのprefix付きSHA-256期待hexは `9c13119acee0ec840ee6af42b4d6f80a4cbdd5d650206d7070db7aef20f3bc30`。Task MB-01でcanonical bytesとこのhex、および実コマンドfixtureの期待hexを凍結する。独立した計算との一致を記録し、期待値を実装の同じ関数からテスト時に生成しない。

cursorはversion=1、query kind、sort/key、filter fingerprint、principal/membership fingerprint、access_revisionを持つ。canonical JSON→UTF-8 hexのopaque文字列として実装でき、新しいbase64依存は不要。最大8192 bytes、無効hex/構造/型/上限違反はValidation。不一致はCursorStale。署名の有無に依存せず、cursorを認可証明や信頼できるSQLとして使わない。membership fingerprintには有効期限を含めず、issuer付きsubject集合をsortする。

## 5. 実装Task

各Taskの手順はRED→原因確認→最小実装→GREEN→差分確認→commit。fixtureに必要なproduction機能を先に実装してREDを消さない。実DB試験は既存testcontainersのfixtureを拡張し、Docker不在をPASS扱いにしない。以下のコマンドはrepository rootから実行する。

### MB-01: 純粋な権限・対象型と管理コマンド契約（PR A）

**Files:** 新規D/src/resource_ref.rs、access_policy.rs、A/src/access_context.rs、management_command.rs、management_digest.rs、management_ports.rs。変更D/Aのlib.rs・error.rs。試験D/tests/access_policy_contract.rs、A/tests/management_contract.rs。

**Interfaces:** §4.1〜4.2の型。`evaluate_policy(subjects: &[PolicySubject], effective: &PolicyMode, required: &[Action]) -> bool`。`canonical_command_bytes(ctx: &VerifiedActorContext, command: &ManagementCommand) -> Result<Vec<u8>, ApplicationError>` と `management_command_digest(ctx: &VerifiedActorContext, command: &ManagementCommand) -> Result<[u8;32], ApplicationError>`。effectiveはnearest policy解決後で、Inherit単独はdeny。

- [ ] REDを作成: `nearest_policy_replaces_not_unions`、`explicit_empty_is_not_inherit`、`issuer_separates_groups`、`administer_does_not_imply_read`、`digest_ignores_map_order_not_command_content`、`uuid_v4_and_conflicting_patch_are_rejected`。
- [ ] `cargo test --locked -p document-domain --test access_policy_contract` と `cargo test --locked -p document-application --test management_contract` を実行し、未定義契約によるREDを確認する。
- [ ] 型・pure evaluator・検証・digestを実装する。全action必要条件はAND、複数主体の同一action許可はOR。trusted contextからのみactorを取り、未検証委任・期限切れを拒否する。
- [ ] 同コマンドをGREENにし、固定canonical vector・digest hexをfixtureへ記録する。境界1024 bytes/64 KiB/深さ16、数値・Unicode・空policyを検査する。
- [ ] 対象ファイルと試験だけをstageし `feat: add document management policy and command contracts` でcommitする。

固定assertion例: `assert!(!evaluate_policy(&subjects, &PolicyMode::Explicit(vec![]), &[Action::Read]));`。同じfixtureをSQLへ投入してdenyを確認する。

### MB-02: T8・原子的な認可・管理台帳・型付きイベント（PR A）

**Files:** 新規A/src/access_policy_service.rs、P/src/access_control.rs、access_policy.rs、management_ledger.rs、targeted_events.rs、P/tests/access_policy_transaction.rs、P/tests/support/management.rs、M-A。変更A/src/events.rs、P/src/lib.rs。

**Interfaces:** `AccessPolicyService::set_access_policy`、`ManagementRepository::{execute,lookup}`、`BootstrapRootPolicy`専用port。P内だけの `lock_access_state(tx, mode) -> AccessRevision`、`authorize_in_tx(tx, ctx, requirements) -> Result<(), RepositoryError>`。modeはShared/Exclusiveで、T8はExclusive。

- [ ] REDを作成: root未設定deny、bootstrap二重初期化拒否、nearest policyのRust/SQL同一fixture、T8予約中変更、同ID再実行・異要求・no-op、audit insert失敗でpolicy/epoch/台帳全rollback。
- [ ] `cargo test --locked -p document-repository-postgres --test access_policy_transaction` を実行し、期待する未実装箇所のREDを確認する。
- [ ] M-Aとサービスを実装する。対象の変更前administerで判定し、共有/排他guardと最新policyをtransaction内で使用する。new resultを返す前に台帳・Domain/Auditを確定する。
- [ ] lookup/replayも現在の開示認可を行う。Denied監査は別transactionで記録し、記録失敗時も許可へ反転しない。監査理由は分類code中心とし、本文・ACL全値・無制限入力を入れない。
- [ ] 同コマンドをGREENにし、Domain/Audit旧Document形式のround-tripを追加する。`access_policy.changed`、対象binding、policy/access revisionを区別する。
- [ ] M-Aの実採番とschema diffをstatusへ記録し、`feat: persist access policies and atomic management events` でcommitする。

固定assertion例: `assert_eq!(after.access_revision, before.access_revision + 1); assert_eq!(after.document_revision, before.document_revision);`（T8実変更）。rollback fixtureでは両者ともbeforeと一致する。

### MB-03: 既存登録・版・公開・T10へ認可を接続（PR A）

**Files:** 新規A/src/authorized_document.rs、P/src/authorized_repository.rs、P/tests/authorized_document_transaction.rs。変更§4の既存業務サービス・Repository実装。必要なarchitecture試験はA/tests/authorized_entrypoint_contract.rsへ追加する。

**Interfaces:** `AuthorizedDocumentService`は既存コマンド/結果型を使い、create_document、create_version、update_working_version、rebase_working_version、publish_document、withdraw_version、schedule_publish、cancel_schedule、end_document_publicationにctxを追加した入口を持つ。既存コアのoperation IDや結果を新台帳で置き換えない。

- [ ] REDを作成: readだけではwrite/publish不能、初版WORKING編集はread+writeで可能、検査後の剥奪commitで操作拒否、T10後の通常取得不可、過去T10結果のreplayが現在認可を迂回しない。
- [ ] `cargo test --locked -p document-repository-postgres --test authorized_document_transaction` とAの `authorized_entrypoint_contract` を実行する。
- [ ] trust boundary付き入口とRepositoryのtransaction内部guardを接続する。旧公開ロジックを複製せず、共通transaction helperへ最小限分離する。既存の成功ledger replayは業務的には維持し、開示だけ現在認可する。
- [ ] 未認可の既存サービスは内部境界に限定する。新入口・将来transportの許可importを契約試験で固定し、ctxを省略するとallow-allとなる既定値を追加しない。
- [ ] GREEN後に既存 `publication_end_visibility`、`publication_end_guards`、`publish_transaction`、`versioning_transaction` をPのtest targetとして実行する。副作用・イベントの二重生成が0であることを確認する。
- [ ] `feat: enforce document authorization at commit and read boundaries` でcommitする。

固定assertion例: `assert_eq!(after.publish_operation_count, before.publish_operation_count); assert_eq!(after.current_version_id, before.current_version_id);`（検査中の権限剥奪）。

### MB-04: 期限到達予約の現在認可と失敗分類（PR Aの完了条件）

**Files:** 新規A/src/scheduled_authorization.rs、P/tests/scheduled_authorization_transaction.rs。変更A/src/schedule.rs、P/src/schedule.rs、S/src/runner.rs・lib.rs・main.rs。

**Interfaces:** `authorize_scheduled_publish(resolver, requester, executor) -> Result<VerifiedActorContext, IdentityResolutionError>`。DueSchedulerへresolverを必須注入するassembly境界を設け、既存期限到達処理は認可付きRepositoryで実行する。未設定resolverは起動時の明示エラーで、固定全権主体へ置換しない。

- [ ] REDを作成: 権限剥奪は予約があっても成功、予約実行は拒否して既存終端処理へ、identity Unavailableは同IDでretry、複数workerでもpublish成功と監査は1回。
- [ ] `cargo test --locked -p document-repository-postgres --test scheduled_authorization_transaction` を実行しREDを確認する。
- [ ] 依頼者identityは長いDBロックの外で更新し、commit前にexpiryと最新policyを再確認する。確定した権限不足は `authorization_revoked`、InvalidIdentityは `identity_invalid` として既存終端処理へ渡す。終端化は信頼済みschedulerだけが使う限定操作とし、拒否された依頼者contextを許可済みに偽装せず、workerの権限でPublishへ迂回しない。一時障害では公開も終端確定もせず再試行する。
- [ ] 既存のDB時刻・revision・manifest・DSI・品質・operation IDを維持する。予約依頼者とservice executorを監査で区別する。古い予約にexecutor情報を捏造しない。
- [ ] GREEN後、Pの `due_transaction` と `schedule_transaction`、`cargo test --locked -p document-publication-scheduler` を実行する。実identity接続が未提供の間は本番scheduler配備の前提が未充足と記録する。
- [ ] `feat: reauthorize scheduled publication requests` でcommitし、A全体のイベント・回帰・exact-head gateを確認する。

固定assertion例: `assert_eq!(denied.current_version_id, before.current_version_id); assert_eq!(temporary_failure.schedule_status, "PENDING");`。temporary_failureでは同じpublish_operation_idを保持する。

### MB-05: T5の部分更新・予約制限・no-op（PR B）

**Files:** 新規A/src/document_management.rs、P/src/document_management.rs、P/tests/document_metadata_transaction.rs。必要なmodule export。

**Interfaces:** §4.2 `update_document_metadata`。T5台帳/イベントはMB-02を利用し、DocumentMetadataChanged / document.metadata.changedを同時に保存する。

- [ ] REDを作成: 既存未知キー保持、set/unset重複拒否、extensions object検証、同値no-op、古いrevisionならno-opでもConflict、PENDING実変更拒否、T10後の追加権限。
- [ ] `cargo test --locked -p document-repository-postgres --test document_metadata_transaction` を実行する。
- [ ] access共有guard→Document lock→現在権限・期待revision→no-opまたは実変更→台帳/イベントの順に実装する。extensionsのsetはそのobject全体の置換で、任意JSONPath patchは実装しない。
- [ ] GREENで原本・Version番号・Version metadata・ReadState不変、変更時revision+1、no-opイベント0、audit失敗時全rollbackを検査する。
- [ ] `feat: update document metadata transactionally` でcommitする。

固定assertion例: `assert_eq!(after.version_ids, before.version_ids); assert_eq!(after.metadata["legacy_key"], before.metadata["legacy_key"]); assert_eq!(noop.event_count, 0);`。

### MB-06: T7の作成・改名・安全な名前移行（PR B）

**Files:** 新規D/src/folder.rs、A/src/folder_service.rs、P/src/folder_management.rs、folder_preflight.rs、P/tests/folder_management_transaction.rs、M-B。

**Interfaces:** §4.2 `create_folder` / `rename_folder`、`normalize_folder_name(&str) -> Result<String, DomainError>`、`preflight_folder_names(&PgPool) -> Result<FolderPreflightReport, RepositoryError>`。

- [ ] REDを作成: NFC同名、大小文字別名、255/256 scalars、空/制御文字/slash/dot拒否、root保護、親なし、同名同時作成、一方失敗時イベント0。
- [ ] `cargo test --locked -p document-repository-postgres --test folder_management_transaction` を実行する。
- [ ] 正規化・親lock・DB一意性を実装する。DBの一意キーはparent IDと正規化name、大小文字区別の比較に固定する。名前はpath/識別子にしない。
- [ ] 移行前検査でcycle・孤立・複数root・不正名・正規化衝突・非正規化既存名を検出したら停止する。対象ID/分類のみを報告し、自動改名しない。保守時間内の書込停止下で検査とmigrationを実施し、無検査の部分移行を避ける。
- [ ] GREENでFolderCreated/FolderRenamedと監査、revision/no-op、失敗rollback、初期root ID不変を確認する。
- [ ] `feat: add folder creation and renaming with migration guards` でcommitする。

固定assertion例: `assert!(normalize_folder_name(&"あ".repeat(255)).is_ok()); assert!(normalize_folder_name(&"あ".repeat(256)).is_err());`。

### MB-07: T6文書移動・T7サブツリー移動（PR Bの完了条件）

**Files:** 変更A/src/document_management.rs、folder_service.rs、P/src/document_management.rs、folder_management.rs。試験P/tests/management_move_transaction.rs。

**Interfaces:** §4.2 `move_document` / `move_folder`。access排他guardと現在の旧/新親・影響対象administerを用いる。

- [ ] REDを作成: from不一致、同一場所no-op、予約中拒否、A→B/B→A同時移動、明示policyで継承が遮断される子、配下1対象だけadminister不足、T10後のread_history不足。
- [ ] `cargo test --locked -p document-repository-postgres --test management_move_transaction` を実行する。
- [ ] 影響範囲をrecursive queryで全件確認し、cycle/認可/予約検査後だけ移動する。大きいsubtreeで時間切れなら全体失敗とし、部分移動しない。子孫Document revisionを一括加算しない。
- [ ] GREENで実効ACL切替、明示Document policy・Version・原本・既読不変、DocumentMoved/FolderMoved、subtree影響とaccess_revisionを検査する。T10のcurrentはnullのまま。
- [ ] `feat: move documents and folder subtrees safely` でcommitし、Bの原子性・予約回帰・exact-head gateを確認する。

固定assertion例: `assert_eq!(success_count, 1); assert!(!graph_has_cycle); assert_eq!(after.version_ids, before.version_ids);`（同時相互移動）。

### MB-08: T9本人の初回既読確認（PR C）

**Files:** 新規A/src/read_state.rs、P/src/read_state.rs、P/tests/read_state_transaction.rs、M-C。

**Interfaces:** §4.2 `mark_version_read`。一意キーによる `INSERT ... ON CONFLICT DO NOTHING` と初回監査を同一transactionで扱う。

- [ ] REDを作成: HumanInteractiveのみ、Agent/Service拒否、別provider同一IDの分離、並行2要求で1行/1監査、新版切替との競合、既存旧版既読の再照会にも現在認可。
- [ ] `cargo test --locked -p document-repository-postgres --test read_state_transaction` を実行する。
- [ ] access共有guard→Document lock→現在の参照権限→既存ReadStateの安全な再生、未記録なら同じDocumentの現行PUBLISHED・T10未終了を確認してinsertする。新版を自動的に既読にしない。
- [ ] GREENで監査失敗rollback、Document revision不変、first_read_at不変、新版未読・旧版行保持を確認する。Domainイベント・汎用操作台帳は作らない。
- [ ] `feat: record explicit per-version read confirmation` でcommitする。

固定assertion例: `assert_eq!(read_state_rows, 1); assert_eq!(read_confirmation_audits, 1); assert_eq!(after.document_revision, before.document_revision);`（T9並行実行）。

### MB-09: 認可付き一覧・簡易検索・ページング（PR C）

**Files:** 新規A/src/document_query.rs、query_cursor.rs、P/src/document_query.rs、P/tests/document_query_authorization.rs、A/tests/query_cursor_contract.rs。

**Interfaces:** §4.3の3一覧とlist_child_folders。`Sort = CreatedAtDesc | TitleAsc | PublishedAtDesc`。戻りDTOは公開・編集・履歴で別型にする。

- [ ] REDを作成: 3scope別の可視性、WORKING秘密titleで公開一覧を検索できない、未読条件、literal `%`/`_`、同一sort keyの境界、page size50/200/201、cursorの別principal/filter/scope/access epoch拒否。
- [ ] Pの `document_query_authorization` とAの `query_cursor_contract` をcargo test --lockedで実行する。
- [ ] policy/公開条件を同じSQLに組み込み、論理的な認可済み集合を絞り込み・sort・keyset limitする。SQL plannerの物理実行順に依存しない。取得後の権限filterは禁止。全sortにDocument IDのtie-breakを付け、タイトル比較はRustとSQLで同じUnicode正規化/大小文字区別に固定する。
- [ ] Authoringの表示対象はWORKING優先、なければcurrent。History一覧の表示代表は最も大きいversion_noの履歴閲覧可能な版で、WORKINGはwriteがある場合だけ候補。代表選択でT10文書を公開一覧へ混ぜない。
- [ ] GREENで読めないFolder名/祖先名/子の件数・ACL・他人の既読を返さないこと、cursor再認可、期限切れctxを確認する。純粋な読み取りの副作用0も検査する。
- [ ] `feat: query authorized document and folder lists` でcommitする。

固定assertion例: `assert_eq!(default_page.items.len(), 50); assert!(validate_page_size(201).is_err()); assert!(!public_ids.contains(&hidden_document));`（十分な件数のfixture）。

### MB-10: 版・操作履歴と監査先行のファイル参照（PR Cの完了条件）

**Files:** 新規A/src/document_history.rs、file_access.rs、P/src/document_history.rs、file_access.rs、P/tests/document_history_projection.rs、P/tests/version_file_access.rs。必要な予約台帳の補足列はM-Cと関連するP/src/schedule.rsに含める。

**Interfaces:** §4.3の残り。`DocumentHistoryEntry`はsource_kind、source_key、occurred_at（不明はNone）、actor（不明はNone）、stable action code、許可済み詳細、provenance_qualityを持つ。送信前監査はdocument.file.access_granted。

- [ ] REDを作成: read_historyなし、WORKINGにwriteなし、T10後の履歴だけ可、他文書Version/他item表現拒否、Outboxを削除しても履歴が保持される、古い行の未知actor/timeを生成しない、audit障害でStorage open回数0。
- [ ] Pの `document_history_projection` と `version_file_access` をcargo test --lockedで実行する。
- [ ] 既存publish/version/schedule/T10台帳と新管理台帳を投影する。Version行fallbackとledger由来を明示し、同じoperationを二重表示しない。履歴はoccurred_atとnamespaced source keyで決定的に並べ、未知時刻は末尾・source key順とする。
- [ ] 予約終端の時刻・executorが既存台帳に残らない場合に備え、nullableなterminal_at/terminal_executor参照を追記し、新規終端時に同時保存する。既存NULLをOutboxから推測補完しない。既存予約状態の意味を変えない。再実行で履歴の時刻を更新しない。
- [ ] 認可・所属・可視性を同一snapshotで検査し、必須開示監査commit後だけStorageをopenする。scopeを自己申告しても権限は増えない。必要なら監査commit不明時は開示せず、誤って送信成功を記録しない。
- [ ] GREENでContentItem/Representationを正本とし、未分類のlegacy添付を推測変換しないこと、Storage locator・原本パス・無許可旧Folder名を返さないことを確認する。ストリーム中はDB lockを保持しない。初版WORKINGの既存編集取得とT10禁止の回帰を実行する。
- [ ] `feat: expose authorized history and audited file access` でcommitし、Cのexact-head gateを確認する。

固定assertion例: `assert_eq!(storage_open_calls, 0); assert_eq!(history_after_outbox_cleanup, history_before_outbox_cleanup);`。前者はaudit失敗、後者は配送整理fixture。

### MB-11: T11/T12横断統合・受入検証（PR D、1本）

**Files:** 新規P/tests/management_event_matrix.rs、management_concurrency.rs、management_vertical_slice.rs、management_migration.rs。新規 `docs/superpowers/execution/document-management-basics-v0-acceptance.md`。修正が必要なら担当機能の最小差分と回帰試験だけを含める。

**Interfaces:** A/B/Cの公開契約をそのまま使う。新しい検索・配送・監査保存基盤を追加しない。

- [ ] 各操作のrequired Domain/Audit数、対象型、revision、台帳、replay/no-opの期待値を表にする。既存T1〜T4/T3a/T10も含め、T9に不要な検索イベントを要求しない。
- [ ] 新しい障害注入・競合ケースでREDを記録する。既に満たすcaseは最初からGREENだったと正直に記録し、架空のREDを作らない。
- [ ] Pの `management_event_matrix`、`management_concurrency`、`management_vertical_slice`、`management_migration` をcargo test --lockedで実行する。Domain/Audit書込失敗、commit結果不明、同ID再試行、権限剥奪、相互Folder移動、予約競合を実PostgreSQLで確認する。
- [ ] migrationは旧0005相当snapshot→新schema、失敗時のtransaction rollback、移行前snapshotへの復元を隔離DBで実証する。稼働後の無損失down-migrationは約束せず、書込停止・バックアップ復元またはforward fixとする。試験のために本番データを削除しない。
- [ ] 既存Create/Get・Publish・Versioning・DSI・T10・scheduler回帰と `mise run verify:fast`、pin済みPDFium/Dockerを用意した `mise run verify` を実行する。full gateは差分・repository policyが要求する場合に `mise run verify:full` を実行する。環境不足は未検証と記録する。
- [ ] 合成データで1,000 principal、10,000文書、1,000 Folder、深さ10を基準に一覧・継承・移動・既読の時間/SQL planを計測する。これは負荷fixtureであり容量上限・未合意のSLOではない。timeoutはfail closed、秘密値は計測ログへ出さない。
- [ ] `test: qualify document management event and audit consistency` でcommitし、最終exact headのCI/required checksを取得する。DMB-01〜25の実行証拠と残る本番identity/transport前提を記録し、レビュー・明示merge指示を待つ。

固定assertion例: `assert_eq!(after_exact_replay.revision, first_success.revision); assert_eq!(after_exact_replay.event_ids, first_success.event_ids);`。集計上のexactly-onceとネットワーク配送保証を混同しない。

## 6. 受入条件とTaskの対応

| 設計の受入ID | 実装・個別試験 | 横断確認 |
|---|---|---|
| DMB-01, DMB-02 | MB-05 | MB-11 |
| DMB-03 | MB-04/05/07 | MB-11 |
| DMB-04 | MB-07 | MB-11 |
| DMB-05, DMB-06 | MB-06/07 | MB-11 |
| DMB-07, DMB-08 | MB-01/02 | MB-11 |
| DMB-09 | MB-02/03/07 | MB-11 |
| DMB-10, DMB-11 | MB-04 | MB-11 |
| DMB-12, DMB-13, DMB-14 | MB-08/09 | MB-11 |
| DMB-15, DMB-16 | MB-09 | MB-11 |
| DMB-17 | MB-03/09/10 | MB-11 |
| DMB-18, DMB-19, DMB-20 | MB-05/07/10 | MB-11 |
| DMB-21, DMB-22 | MB-02および各mutation Task | MB-11 |
| DMB-23 | MB-10 | MB-11 |
| DMB-24 | MB-02/03/04/06/08/10 | MB-11 |
| DMB-25 | MB-02/09/10 | MB-11 |

## 7. 規範反映・設計差分の扱い

本計画は規範specを上書きしない。実装開始前に設計§17の追記を設計/計画ブランチで反映し、承認済み本文と同じ意味であることを差分レビューする。必要な追記先は `spec/data/logical-data-model-v0.md`、`spec/data/transaction-consistency-requirements-v0.md`、`spec/operations/observability-audit-requirements-v0.md`。

既存 `2026-09-27-document-versioning-v0-design.md` の認可除外に対して、今回のCapabilityが提供する期限到達認可を別の追記・承認記録で接続する。T10設計の通常公開遮断は変更せず、履歴専用経路の接続だけを追記する。過去の承認記録は改変しない。規範追記の承認状態を記録し、意味が設計承認を超える差分はその差分だけレビューする。

今回レビュー対象の具体化は、入力上限、digest/cursor符号化、DTO・module名、History一覧の代表版、履歴の未知値並び順、予約台帳のnullable終端証跡、移行前検査・復元手順である。既存設計の権限・公開・既読の意味を黙って変える権限は本計画にない。

## 8. 引継ぎ・次のexact action

現在の作業は計画作成まで。Task MB-01〜11はすべて未実行で、テストコードもmigrationも追加していない。ローカルのrepository cloneは実行環境の名前解決で失敗したため、GitHub connectorのsourceを用いた計画照合のみであり、ローカルRust/実DB gateの成功を主張しない。

次は本計画の書面レビューと実行方法の確定。承認後もログ一元管理の完了が未確認なら `PLAN APPROVED / WAITING_FOR_LOG_CENTRALIZATION` として保持する。完了確認は依頼者の明示確認または指定された正本証拠のみ。時間経過やknowledge-platformのCI成功で代用しない。

実装開始条件が満たされた時だけ、最新mainとの差分、migration採番、規範反映、環境、exact-head checksを確認し、今回のstatusをactiveへ接続してMB-01のREDから開始する。自動監視・自動開始は設定していない。
