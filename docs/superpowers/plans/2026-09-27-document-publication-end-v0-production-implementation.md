# Document Publication End v0 本番実装計画

> **実装時の必須スキル:** `superpowers:executing-plans` を使い、この計画を Task ごとに進める。`- [ ]` は実行時の確認欄。依頼者が選択済みの実行方法は、このセッションでのインライン実装であり、worker に分割しない。

**状態:** **承認済み — Production Implementation を開始可能。** 承認記録: `2026-09-27-document-publication-end-v0-production-implementation-approval.md`。

**目的:** T10 として文書全体の公開を終了し、原本・過去版を保持しつつ、通常公開読み取りと検索から確実に外す。

**構成:** Domain が現行版から null への遷移を定義し、Application が UUIDv7 操作 ID と再実行を扱う。PostgreSQL が文書行ロックの下で現行版参照、予約、操作台帳、Domain/Audit Outbox を一括更新する。既存の編集・authoritative 読み取りは T10 後だけ遮断し、通常公開用の現行 `PUBLISHED` 版専用クエリと分離する。

**技術:** Rust workspace（`rust-version = "1.98"`）、PostgreSQL/sqlx、既存の `uuid` v7・`time`・`sha2`。新規 parser dependency は不要。

**設計:** `docs/superpowers/specs/2026-09-27-document-publication-end-v0-design.md`、承認記録、承認済みの読み取り境界改訂 1。規範仕様は `spec/data/logical-data-model-v0.md` と `spec/data/transaction-consistency-requirements-v0.md`。

**実装ブランチ:** 計画承認後、承認済みの `design/document-publication-end-v0` head から `feat/document-publication-end-v0` を作り、設計 PR #13 を base とする別の Draft 実装 PR を開く。PR #13 は設計・計画のレビュー用に維持する。

## 共通制約

- T10 は T4 の Version 取下げと別操作。元の現行版を `WITHDRAWN` にしない。
- `current_version_id` を null にし、元の `PUBLISHED` Version・原本・`published_at` を保持する。新しい Document フラグ、Version 状態、共通 cross-format content IR は追加しない。
- T10 操作は caller UUIDv7、期待 revision・現行版、実行者、理由で識別する。同一 ID の同一要求は完全再実行し、commit 不明時も同じ ID を使う。
- 公開終了、予約の終端化、操作結果、Domain/Audit Outbox を一つの DB transaction で確定する。Storage と DSI の障害は終了を妨げない。
- 既存の `get_document`・`open_primary_file` は T10 未終了の `WORKING` 初版を引き続き読めるが、T10 後は NotFound とする。通常公開用の別 API は現行 `PUBLISHED` 版だけを返す。
- T10 後の通常の Version 作成・更新・再基準化・予約・手動／期限到達 Publish は再公開できない。過去版 T4 は current null のままなら許容する。
- Search Extraction、DSI、Search consumer、HTTP/UI、AccessPolicy 実装を T10 に結合しない。検索結果の現行性照合は Document 側で提供する。
- Task ごとの確認は対象を絞ったローカル RED/GREEN に限る。ホストの標準 CI、DSI Sandbox Preflight、DSI PoC 回帰は、実装がそろった一つの head で確認する。設計 PR #13 と後続の実装 PR、および基点の PR #11・#12 を明示指示なしにマージしない。

## レビュー重点

1. T10 後も既存 `load_current` のフォールバックから旧版のファイルが読める危険を、Task 4 の既存 API・公開 API 双方の読み取りテストで防ぐ。同時に凍結済みの初版 `WORKING` 読み取りを維持する。
2. `current_version_id = null` を許す手動初版 Publish が終了済み文書を再公開する危険を、Task 3 の初版 Publish テストで防ぐ。
3. T10 と期限到達 worker の競合で予約が公開される危険を、Task 3 の並行テストで防ぐ。
4. T10 完了後の同じ ID の再試行が古い revision を理由に失敗する危険を、Task 1・2 の再実行テストで防ぐ。
5. T10 後の過去版 T4 が旧版を current に戻す危険を、Task 3 の取下げテストで防ぐ。

## ファイルと責務

| 単位 | 責務 | 主なファイル |
|---|---|---|
| Domain 遷移 | 現行公開版の検証、null への遷移、revision | `crates/document-domain/src/document.rs`, `error.rs`, `lib.rs` |
| Application 契約 | コマンド digest、完全再実行、イベント、サービス | `crates/document-application/src/publication_end.rs`, `ports.rs`, `events.rs`, `error.rs`, `lib.rs` |
| PostgreSQL 終了操作 | 操作台帳、行ロック、予約無効化、Outbox の原子性 | `crates/document-repository-postgres/migrations/0005_document_publication_end_v0.sql`, `src/publication_end.rs`, `lib.rs` |
| 再公開防止 | 既存の Version/Publish/予約 transaction 内の T10 ガード | `src/versioning_mutation.rs`, `src/publish.rs`, `src/schedule.rs`（PostgreSQL crate） |
| 読み取り境界 | 終了ガード付きの既存取得、現行公開版専用取得、内部 snapshot、検索結果照合 | `src/versioning_rows.rs`, `src/repository.rs`（PostgreSQL crate）、`crates/document-application/src/ports.rs`, `service.rs`, `versioning_service.rs` |

以下の public 名称は Task 間の契約とする。内部 SQL helper の名前は同じ責務を保つ範囲で調整してよい。

---

### Task 1: Domain と Application の公開終了契約

**Files:** `crates/document-domain/src/document.rs`, `error.rs`, `lib.rs`; 新規 `crates/document-application/src/publication_end.rs`, `tests/publication_end_contract.rs`; `src/ports.rs`, `events.rs`, `error.rs`, `lib.rs`。

**Interfaces:** `Document::end_publication(&mut self, current: &DocumentVersion) -> Result<EndPublicationTransition, DomainError>` は同じ Document の現行 `PUBLISHED` 版だけを許し、Version は変更せず current を null にして revision を 1 増やす。`PublicationEndOperationId::try_from_uuid(Uuid)` は v7 のみを許す。`EndDocumentPublicationCommand::new(operation_id: PublicationEndOperationId, document_id: DocumentId, expected_revision: i64, expected_current_version_id: DocumentVersionId, actor: PrincipalRef, reason: String)` は空白のみの理由を拒否する。`command_digest()` は `document-publication-end-v0\0` の domain tag、操作 ID・Document ID・期待現行 Version ID の UUID bytes、big-endian i64 の期待 revision、続いて identity provider・principal ID・reason の UTF-8 を big-endian u32 の長さ付きで連結し、SHA-256 とする。`DocumentPublicationEndService<I, C, R>::end_document_publication(command: EndDocumentPublicationCommand) -> Result<EndDocumentPublicationResult, ApplicationError>` は `PublicationEndRepository::{get_end_operation, get_end_candidate, end_document_publication}` を使う。Repository port の候補は Document と現行 Version のメタデータのみを返し、FileStorage/DSI を使わない。Domain/Audit イベント種別は `DocumentPublicationEnded` / `document.publication.ended`。

Repository port の戻り値は `EndPublicationCandidate { document: Document, current: Option<DocumentVersion> }`、`EndPublicationOperationRecord { command_digest: [u8; 32], result: EndDocumentPublicationResult }`、`EndPublicationRecord { command, ended_at, domain_event_id, audit_event_id }` とする。候補自体がない場合は Document 不在、候補の current が None の場合は未公開状態として区別する。`get_end_operation(PublicationEndOperationId) -> Result<Option<EndPublicationOperationRecord>, RepositoryError>`、`get_end_candidate(DocumentId) -> Result<Option<EndPublicationCandidate>, RepositoryError>`、`end_document_publication(EndPublicationRecord) -> Result<EndDocumentPublicationResult, RepositoryError>` を公開する。

- [ ] Domain と Application の RED テストを追加する。現行 `PUBLISHED`、他 Document・非公開版、revision overflow、非 v7 ID、空理由、actor/reason を変えた同 ID の Conflict、終了後の同 ID 完全再実行を pin する。
- [ ] `cargo test -p document-domain publication_end_` と `cargo test -p document-application --test publication_end_contract` を実行し、新規契約だけが RED であることを記録する。
- [ ] Domain 遷移と Application の型・digest・再実行・イベント生成を実装し、Repository port はテスト用 fake で確認する。commit 不明は同 ID を保持する専用 error とする。
- [ ] 同じ対象コマンドと `cargo fmt --check` を通し、契約を commit する。

### Task 2: PostgreSQL の公開終了 transaction

**Files:** 新規 `crates/document-repository-postgres/migrations/0005_document_publication_end_v0.sql`, `src/publication_end.rs`, `tests/publication_end_transaction.rs`; `src/lib.rs`、Application の Repository port 実装。

**Interfaces:** `document_publication_end_operations` は UUIDv7 操作 ID の主キー、Document ID の一意制約、同一 Document の元現行 Version 参照、32-byte command digest、期待 revision/現行版、actor、理由、結果、UTC 日時を持つ。`PublicationEndRepository::end_document_publication(record)` は Document 行を最初に `FOR UPDATE` し、台帳再照会、現行版・revision・状態の再検証、null current と revision 更新、全 `PENDING` 予約の `TERMINAL` 化、`scheduled_publish_at` の消去、台帳・Domain/Audit Outbox 追加を一括 commit する。

- [ ] PostgreSQL の RED テストを追加する。版・原本を変えない終了、初版／後続版予約の終端、監査 Outbox 生成失敗時の rollback、同 ID 再実行、ID 衝突、別 ID 再終了、二者競合、Storage/DSI 不在での成功を pin する。
- [ ] `cargo test -p document-repository-postgres --test publication_end_transaction` で意図した RED を記録する。
- [ ] 追加 migration と短い locked transaction を実装する。操作結果・イベントを commit するまでは呼出側に成功を返さず、結果不明時は同じ ID で回復させる。
- [ ] 同じ対象コマンドと migration の rollback canary を通し、transaction を commit する。

### Task 3: 既存の公開・予約・Version 操作からの再公開防止

**Files:** `crates/document-repository-postgres/src/publication_end.rs`, `versioning_mutation.rs`, `publish.rs`, `schedule.rs`; `tests/publication_end_guards.rs`。必要な Application の早期拒否は `crates/document-application/src/versioning_service.rs`, `service.rs` に限る。

**Interfaces:** `publication_end::ensure_not_ended(&mut Transaction<'_, Postgres>, DocumentId) -> Result<(), RepositoryError>` を Document 行ロック後に使う。Task 2 の同一 Document 一意台帳を正本とし、preflight だけでは再公開を防がない。既存 Publish operation ID の完全再実行は、T10 前に保存済みの結果を返しても新規 mutation を起こさない。

- [ ] RED テストを追加する。T10 後の create/update/rebase、初版・後続版の手動 Publish、予約登録、期限到達 worker のすべてを拒否し、期限到達と T10 の競合でも現行版が戻らないことを pin する。過去版 T4 は current null のまま成立するケースも pin する。
- [ ] `cargo test -p document-repository-postgres --test publication_end_guards` で RED を記録する。
- [ ] `versioning_mutation::mutate`、`publish_initial_version`、`publish_next_version`、`schedule::reserve` の各ロック済み transaction に共通ガードを置く。due Publish は Publish transaction のガードと終端予約の確認で防ぐ。T4 の旧版取下げはブロックしない。
- [ ] 同じ対象コマンドに加え、既存 `publish_transaction`・`schedule_transaction`・`withdrawal_transaction` の対象を通して commit する。

### Task 4: 既存読み取り・通常公開読み取り・内部 snapshot の分離

**Files:** `crates/document-application/src/ports.rs`, `service.rs`, `versioning_service.rs`; `crates/document-repository-postgres/src/versioning_rows.rs`, `repository.rs`; 新規 `crates/document-application/tests/publication_end_read_contract.rs`, `crates/document-repository-postgres/tests/publication_end_visibility.rs`。既存 fake と凍結済み下書き読込テストは必要な新 port だけ合わせる。

**Interfaces:** 既存の `DocumentRepository::get_authoritative_document(DocumentId)` は Create の commit 結果照会用の内部 snapshot として残し、通常公開 transport には接続しない。明示的な Version ID の `get_version_snapshot` も Versioning 内部に残す。新しい `DocumentRepository::get_authoring_document(DocumentId) -> Result<Option<AuthoritativeDocument>, RepositoryError>` は従来の選択順を保ちながら、T10 台帳の `NOT EXISTS` 条件を取得と同じ SQL statement に含める。`DocumentService::{get_document, open_primary_file}` はこの終了ガード付き port を使い、T10 後は NotFound、未終了の初版 `WORKING` は従来どおり取得する。

`DocumentRepository::get_current_published_document(DocumentId) -> Result<Option<AuthoritativeDocument>, RepositoryError>` は単一 SQL statement で current と同じ Document の `PUBLISHED` Version だけを返す。current が null なら None、壊れた非 null 参照なら IntegrityViolation とし、フォールバックしない。`DocumentService::get_current_published_document(DocumentId) -> Result<AuthoritativeDocument, ApplicationError>` は None を NotFound として返す。`DocumentService::open_current_primary_file(DocumentId) -> Result<ContentReader, ApplicationError>` はこの公開用取得を通してファイルを開く。将来の通常公開・Search 連携 transport の入口はこの経路とする。`DocumentRepository::is_current_published_version(DocumentId, DocumentVersionId) -> Result<bool, RepositoryError>` は、Search 結果を提供する直前の検証契約とする。再構築元の契約として `list_current_published_versions(after: Option<DocumentId>, limit: i64) -> Result<Vec<CurrentPublishedVersionRef>, RepositoryError>` を追加し、Document ID の昇順に最大 1,000 件ずつ列挙する。`CurrentPublishedVersionRef` は Document ID、現行 Version ID、Document revision のみを持つ。T10 commit 後に開始した既存・公開用読み取りは旧版を返さない。

- [ ] RED テストを追加する。未終了の `WORKING` 初版は既存 `get_document`・`open_primary_file` で読める。T10 後はその両 API が NotFound となる。新しい通常公開用 API は未公開下書き・過去版・T10 終了済み文書を返さず、現行 `PUBLISHED` 版だけを返す。古い Search hit は抑止し、明示 Version ID の内部 snapshot と Create 結果照会は保持する。壊れた非 null current は IntegrityViolation とする。再構築元のページング列挙には現行公開版だけが含まれ、T10 後の文書は含まれないことを pin する。
- [ ] `cargo test -p document-application --test publication_end_read_contract` と `cargo test -p document-repository-postgres --test publication_end_visibility` で意図した RED を記録する。
- [ ] 終了ガード付きの既存取得と current 専用クエリを実装し、既存の Create 結果照会と Versioning snapshot は内部経路に保つ。公開の過去版 API は追加しない。
- [ ] 同じ対象コマンドと既存 `repository_contract`・`vertical_slice`・`publish_vertical_slice` の影響部分を通して commit する。

### Task 5: 全体の整合とレビュー準備

**Files:** 新規 `crates/document-repository-postgres/tests/publication_end_vertical_slice.rs`; `docs/superpowers/execution/document-publication-end-v0-status.md`, `active.md`。後続の実装 PR の説明を更新する。

**Interfaces:** 最初の Publish → 後続版 Publish → 予約 → T10 → 通常公開読み取りなし／Search hit 抑止／過去版保持を一つの実 DB 経路で確認する。最後の branch head は Design・規範仕様と一致し、PR #11・#12 の凍結された意味を変えない。

- [ ] RED の vertical slice を追加し、最小の対象確認で GREEN にする。T10 と due worker の競合、操作 ID の再実行、Outbox、履歴を含める。
- [ ] 組み上がった branch で `mise run verify` を一度実行し、観測された失敗だけ修正する。
- [ ] まとまった実装 head を push し、その exact head の標準 CI、DSI Sandbox Preflight、DSI PoC 回帰を確認する。コード変更があれば新 head の結果を確認し直す。
- [ ] Active/Status に head、run ID、結果、blocker、次の exact action、Design Freeze 差分を記録し、実装 PR を一度レビューする。明示指示なしにマージしない。

## 計画承認ゲート

依頼者は本計画を「承認します。これで実装からテスト全ての今回のタスクを完了するまで続けてください。」と明示承認した。T10 の本番コードは設計ブランチから分けた実装ブランチでインライン実装する。終了済み Document の再公開、T4 の旧版復帰、Version の同一性、予約の意味、DSI の信頼境界を変える必要が出た場合は実装を止め、承認済み設計の改訂に戻る。
