//! P1-V01 body search vertical slice on a real PostgreSQL Document Source and a
//! real filesystem store: current Version Parts → raw read → readers → Unit
//! manifest → Tantivy seal → CAS → BodyOnly → positive, negative and coverage.

#[path = "support/body.rs"]
mod body_support;
#[path = "support/document_discovery.rs"]
mod discovery_support;

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use document_application::{
    BootstrapRootPolicy, DocumentAccessCheckService, FileStorage, InvocationKind, StoreFileRequest,
    StoredFile, VerifiedActorContext,
};
use document_domain::{
    Action, FileId, MediaType, PolicyGrant, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_storage_fs::FileSystemStorage;
use search_application::body_ports::{CONTAINS_EXACT_PREDICATE, ExactTextSelector};
use search_application::content_scope::BodySearchSpec;
use search_application::discovery_service::{DiscoveryPorts, DiscoveryService};
use search_application::indexing_service::{DocumentIndexingService, IndexingOutcome};
use search_application::ports::{CurrentAccessEvaluatorPort, LexicalQuery};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{DiscoveryRequest, DiscoveryResult};
use search_core::evidence::{ClaimState, EvidenceRole};
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::source::RetentionMode;
use search_source_document::{
    BodyItemExtractor, DocumentBodyCoverageGaps, DocumentBodyExtractor, DocumentCoveragePreflight,
    DocumentCoverageRequirement, DocumentCurrentAccessAdapter, DocumentEvidenceCatalog,
    DocumentExactTextEvidenceCatalog, DocumentOutboxIndexer, MemoryDocumentIndexRuntime,
    PostgresDocumentSnapshotReader,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

fn principal() -> PrincipalRef {
    PrincipalRef::new("test-idp", "editor").unwrap()
}

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        principal(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

struct Source {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pool: PgPool,
    root: PathBuf,
    storage: FileSystemStorage,
    access: Arc<DocumentCurrentAccessAdapter>,
    source_id: SourceId,
}

struct Published {
    document_id: Uuid,
    version_id: Uuid,
    files: Vec<StoredFile>,
}

impl Source {
    async fn start() -> Self {
        let container = GenericImage::new("postgres", "18.6-bookworm")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", "body_vertical_test")
            .start()
            .await
            .unwrap();
        let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect(&format!(
                "postgres://postgres:postgres@127.0.0.1:{port}/body_vertical_test"
            ))
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
            pool.clone(),
            principal(),
        ));
        repository
            .initialize_root_policy(
                &actor(),
                vec![
                    PolicyGrant::new(
                        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor")
                            .unwrap(),
                        [Action::Read],
                    )
                    .unwrap(),
                ],
            )
            .await
            .unwrap();
        let source_id = SourceId::from_uuid(Uuid::now_v7());
        let access = Arc::new(DocumentCurrentAccessAdapter::new(
            source_id,
            pool.clone(),
            DocumentAccessCheckService::new(repository),
            actor(),
            discovery_support::ACCESS_CONTEXT.into(),
        ));
        let root = std::env::temp_dir().join(format!("search-body-vertical-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        Self {
            _container: container,
            pool,
            storage: FileSystemStorage::new(&root),
            root,
            access,
            source_id,
        }
    }

    async fn store(&self, bytes: &[u8], media: &str) -> (FileId, StoredFile) {
        let file_id = FileId::from_uuid(Uuid::now_v7());
        let stored = self
            .storage
            .put_immutable(StoreFileRequest::new(
                file_id,
                Box::pin(Cursor::new(bytes.to_vec())),
                MediaType::new(media).unwrap(),
            ))
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
             VALUES ($1,$2,$3,$4,$5,now())",
        )
        .bind(file_id.as_uuid())
        .bind(stored.content_hash().as_bytes().to_vec())
        .bind(media)
        .bind(stored.size_bytes().get())
        .bind(stored.storage_key().as_str())
        .execute(&self.pool)
        .await
        .unwrap();
        (file_id, stored)
    }

    /// Publishes a new current Version of `document` (created when `None`).
    async fn publish(
        &self,
        document: Option<Uuid>,
        title: &str,
        parts: &[(&[u8], &str)],
    ) -> Published {
        let document_id = match document {
            Some(document) => document,
            None => {
                let document = Uuid::now_v7();
                sqlx::query(
                    "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
                     VALUES ($1,$2,NULL,1,'{}'::jsonb,now())",
                )
                .bind(document)
                .bind(SYSTEM_ROOT_FOLDER_ID)
                .execute(&self.pool)
                .await
                .unwrap();
                document
            }
        };
        let version_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
             title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
             VALUES ($1,$2,(SELECT COUNT(*) + 1 FROM document_versions WHERE document_id = $2), \
                     'PUBLISHED',$3,now(),'test-idp','editor','{}'::jsonb,now())",
        )
        .bind(version_id)
        .bind(document_id)
        .bind(title)
        .execute(&self.pool)
        .await
        .unwrap();
        let mut files = Vec::new();
        for (ordinal, (bytes, media)) in parts.iter().enumerate() {
            let (file_id, stored) = self.store(bytes, media).await;
            let item = Uuid::now_v7();
            let representation = Uuid::now_v7();
            let mut tx = self.pool.begin().await.unwrap();
            sqlx::query(
                "INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) \
                 VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(item)
            .bind(version_id)
            .bind(format!("body/part-{ordinal}"))
            .bind(i32::try_from(ordinal).unwrap())
            .bind(representation)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
                 VALUES ($1,$2,$3,'AUTHORITATIVE','source')",
            )
            .bind(representation)
            .bind(item)
            .bind(file_id.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
            files.push(stored);
        }
        sqlx::query(
            "UPDATE documents SET current_version_id = $1, revision = revision + 1 WHERE document_id = $2",
        )
        .bind(version_id)
        .bind(document_id)
        .execute(&self.pool)
        .await
        .unwrap();
        Published {
            document_id,
            version_id,
            files,
        }
    }

    fn extractor(&self) -> Arc<dyn BodyItemExtractor> {
        Arc::new(DocumentBodyExtractor::new(
            self.source_id,
            self.storage.clone(),
            body_support::InProcessExtractor::new(body_support::Mode::Honest),
            body_support::registry(),
        ))
    }

    async fn index(&self, runtime: &MemoryDocumentIndexRuntime, document: Uuid) {
        let mut config = discovery_support::index_config(self.source_id);
        config.source.retention_mode = RetentionMode::PersistentResource;
        let indexer = DocumentIndexingService::new(
            DocumentOutboxIndexer::new(
                PostgresDocumentSnapshotReader::new(self.pool.clone()),
                config,
                runtime.clone(),
                discovery_support::Receipts::default(),
            )
            .with_body_extractor(self.extractor()),
        );
        let outcome = indexer
            .handle(discovery_support::event(
                "DocumentVersionPublished",
                document,
            ))
            .await
            .unwrap();
        assert!(
            matches!(outcome, IndexingOutcome::Published(_)),
            "{outcome:?}"
        );
    }

    async fn search(
        &self,
        runtime: &MemoryDocumentIndexRuntime,
        request: DiscoveryRequest,
        parent: Uuid,
        literal: &str,
    ) -> DiscoveryResult {
        let claim = request.need.required_claims[0];
        let reader = runtime.projection_reader();
        let lexical = runtime.lexical_reader();
        let evidence = DocumentEvidenceCatalog::new(reader.clone());
        let access: Arc<dyn CurrentAccessEvaluatorPort> = self.access.clone();
        let mut exact = DocumentExactTextEvidenceCatalog::new(
            self.source_id,
            runtime.clone(),
            Arc::new(PostgresDocumentSnapshotReader::new(self.pool.clone())),
            access.clone(),
            self.extractor(),
        );
        exact
            .register(ExactTextSelector {
                claim_id: claim,
                parent_resource: ResourceId::from_uuid(parent),
                predicate: CONTAINS_EXACT_PREDICATE.into(),
                expected_exact_text: literal.into(),
            })
            .unwrap();
        let coverage = DocumentBodyCoverageGaps::new(runtime.clone(), access);
        let mut source = discovery_support::source(self.source_id);
        source.retention_mode = RetentionMode::PersistentResource;
        let mut sources = InMemorySourceRegistry::default();
        sources.insert(source);
        let service = DiscoveryService::new(
            discovery_support::discovery_config(self.source_id, "unused"),
            DiscoveryPorts {
                sources: &sources,
                generations: &reader,
                concepts: &reader,
                retrieval: RetrievalExecutionPorts {
                    remote: None,
                    directory: Some(&reader),
                    structured: Some(&reader),
                    lexical: Some(&lexical),
                    hypergraph: None,
                    graph_resource_access: None,
                    access: self.access.as_ref(),
                    vector: None,
                },
                selectors: &evidence,
                assertions: &reader,
                evidence: &evidence,
                probe: None,
                probe_catalog: None,
                source_policy: None,
            },
        )
        .unwrap()
        .with_exact_text_evidence(&exact)
        .with_exact_text_absence(&exact)
        .with_body_coverage(&coverage);
        DocumentCoveragePreflight::discover(
            &service,
            request,
            DocumentCoverageRequirement::BodyRequired,
            Some(BodySearchSpec {
                query: LexicalQuery::body_only(literal, 10),
                exact_text_claim: Some(claim),
            }),
        )
        .await
        .unwrap()
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A DOCX whose referenced header is a located omission: Completed + Partial.
fn partial_docx(text: &str) -> Vec<u8> {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
    let package = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId9"/></w:sectPr></w:body></w:document>"#
    );
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
    let header = r#"<?xml version="1.0" encoding="UTF-8"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>ヘッダ</w:t></w:r></w:p></w:hdr>"#;
    body_support::zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", package.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
    ])
}

fn exact_claim(result: &DiscoveryResult, claim: ClaimId) -> Vec<ClaimState> {
    result
        .evidence_set
        .iter()
        .filter(|item| item.claim_id == claim)
        .map(|item| item.state)
        .collect()
}

fn qualified(result: &DiscoveryResult) -> Vec<Uuid> {
    result
        .qualified_resources
        .iter()
        .map(|resource| resource.resource_ref.as_uuid())
        .collect()
}

/// The whole Discovery stack is one deep debug-build future; run it on a thread
/// with room for it instead of the default test stack.
#[test]
fn real_source_body_search_positive_negative_coverage_and_currency() {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(64 * 1024 * 1024)
                .enable_all()
                .build()
                .unwrap()
                .block_on(vertical())
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn vertical() {
    let source = Source::start().await;
    // A: Supported text and CSV. B: the phrase only in its title. C: Partial DOCX.
    let a = source
        .publish(
            None,
            "規程A",
            &[
                ("東京の本文\n大阪の補足\n".as_bytes(), "text/plain"),
                ("名,値\n京都,1\n".as_bytes(), "text/csv"),
            ],
        )
        .await;
    let b = source
        .publish(
            None,
            "東京の本文",
            &[("別の内容\n".as_bytes(), "text/plain")],
        )
        .await;
    let c = source
        .publish(None, "規程C", &[(&partial_docx("京都の本文"), DOCX)])
        .await;
    let runtime = MemoryDocumentIndexRuntime::new();
    source.index(&runtime, a.document_id).await;

    // Positive: only A's body Unit qualifies; the title-only B never does.
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source
        .search(&runtime, request.clone(), a.version_id, "東京の本文")
        .await;
    assert_eq!(qualified(&result), vec![a.version_id]);
    assert!(!qualified(&result).contains(&b.version_id));
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Supported]);
    let evidence = &result
        .evidence_set
        .iter()
        .find(|item| item.claim_id == claim)
        .unwrap()
        .evidence_refs[0];
    assert_eq!(evidence.role, EvidenceRole::Primary);
    assert!(!evidence.is_summary);
    // C's visible Partial item blocks global completeness without hiding A's claim.
    assert!(result.unresolved_gaps.iter().any(|gap| gap.blocking
        && gap.required_fact == format!("document.body.coverage:{}:0:partial", c.version_id)));

    // Negative: a finite scan of A's Supported items proves 名古屋 absent from A only.
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source
        .search(&runtime, request, a.version_id, "名古屋")
        .await;
    assert!(qualified(&result).is_empty());
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Absent]);
    // 京都 exists only in a CSV cell of A that no analyzer token matches exactly.
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source.search(&runtime, request, a.version_id, "京").await;
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Unknown]);
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.blocking && gap.required_fact == "document.body.recall_mismatch")
    );
    // A Partial parent never gets a negative proof.
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source.search(&runtime, request, c.version_id, "大阪").await;
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Unknown]);

    // An undecidable Read leaves nothing to qualify and no claim.
    let mut foreign = discovery_support::request();
    foreign.access_context = "another-session".into();
    let claim = foreign.need.required_claims[0];
    let result = source
        .search(&runtime, foreign, a.version_id, "東京の本文")
        .await;
    assert!(qualified(&result).is_empty());
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Unknown]);

    // Raw bytes changed on disk after publication: the positive proof is withdrawn.
    let path = source.root.join(a.files[0].storage_key().as_str());
    let original = std::fs::read(&path).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
    std::fs::write(&path, "東京の別文\n大阪の補足\n".as_bytes()).unwrap();
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source
        .search(&runtime, request, a.version_id, "東京の本文")
        .await;
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Unknown]);
    std::fs::write(&path, original).unwrap();

    // A new current Version invalidates the pinned Units until reindexing.
    let next = source
        .publish(
            Some(a.document_id),
            "規程A",
            &[("東京の改訂\n".as_bytes(), "text/plain")],
        )
        .await;
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source
        .search(&runtime, request, a.version_id, "東京の本文")
        .await;
    assert!(qualified(&result).is_empty());
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Unknown]);
    source.index(&runtime, a.document_id).await;
    let request = discovery_support::request();
    let claim = request.need.required_claims[0];
    let result = source
        .search(&runtime, request, next.version_id, "東京の改訂")
        .await;
    assert_eq!(qualified(&result), vec![next.version_id]);
    assert_eq!(exact_claim(&result, claim), vec![ClaimState::Supported]);
}
