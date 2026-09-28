use document_domain::{
    ContentHash, CreateInitialDocument, CreateWorkingVersion, Document, DocumentId,
    DocumentVersion, DocumentVersionId, DomainError, FileId, FileSize, FolderId, InitialDocument,
    LifecycleState, LogicalPath, MediaType, Metadata, PrincipalRef, SemanticContentItem,
    StorageKey, StoredFileDescriptor, Title, VersionManifest, VersionNo,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn id(raw: u128) -> DocumentVersionId {
    DocumentVersionId::from_uuid(Uuid::from_u128(raw))
}

fn published_initial() -> (Document, DocumentVersion) {
    let initial = InitialDocument::restore_published(
        CreateInitialDocument {
            document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
            version_id: id(2),
            file_id: FileId::from_uuid(Uuid::from_u128(3)),
            folder_id: FolderId::from_uuid(Uuid::from_u128(4)),
            title: Title::new("Policy").unwrap(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: PrincipalRef::new("test", "actor").unwrap(),
            stored_file: StoredFileDescriptor::new(
                StorageKey::new("objects/policy").unwrap(),
                ContentHash::from_slice(&[3; 32]).unwrap(),
                FileSize::new(1).unwrap(),
                MediaType::new("text/plain").unwrap(),
            ),
            original_filename: "policy.txt".into(),
            created_at: OffsetDateTime::UNIX_EPOCH,
        },
        OffsetDateTime::from_unix_timestamp(10).unwrap(),
    )
    .unwrap();
    let (document, version, _, _) = initial.into_parts();
    (document, version)
}

fn working(
    document: &Document,
    version_raw: u128,
    number: i64,
    base: DocumentVersionId,
) -> DocumentVersion {
    DocumentVersion::new_working(CreateWorkingVersion {
        document_version_id: id(version_raw),
        document_id: document.document_id(),
        version_no: VersionNo::new(number).unwrap(),
        base_document_version_id: base,
        title: Title::new("Policy revision").unwrap(),
        created_by: PrincipalRef::new("test", "actor").unwrap(),
        metadata: Metadata::default(),
        created_at: OffsetDateTime::from_unix_timestamp(20).unwrap(),
    })
    .unwrap()
}

fn item(path: &str, ordinal: u32, fingerprint: u8) -> SemanticContentItem {
    SemanticContentItem::new(
        LogicalPath::new(path).unwrap(),
        ordinal,
        "txt",
        "dsi-v0",
        [fingerprint; 32],
    )
    .unwrap()
}

#[test]
fn versioning_path_normalizes_nfc_and_rejects_ambiguous_segments() {
    assert_eq!(
        LogicalPath::new("cafe\u{301}/part").unwrap().as_str(),
        "café/part"
    );
    for invalid in ["", "/root", "a/", "a//b", "a/./b", "a/../b", "a\\b"] {
        assert_eq!(
            LogicalPath::new(invalid),
            Err(DomainError::InvalidLogicalPath)
        );
    }
    assert_eq!(
        VersionManifest::new(
            Title::new("Policy").unwrap(),
            vec![item("cafe\u{301}", 0, 1), item("café", 0, 2)],
        ),
        Err(DomainError::DuplicateContentItemKey),
    );
}

#[test]
fn version_identity_uses_title_order_path_and_semantic_digest() {
    let canonical = VersionManifest::new(
        Title::new("  Café\r\nPolicy  ").unwrap(),
        vec![item("primary", 0, 7)],
    )
    .unwrap();
    let equivalent = VersionManifest::new(
        Title::new("Cafe\u{301}\nPolicy").unwrap(),
        vec![item("primary", 0, 7)],
    )
    .unwrap();
    assert_eq!(canonical.identity_digest(), equivalent.identity_digest());
    assert_eq!(
        canonical
            .identity_digest()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "c4bbc136b0ca2b2212099afed24c2144cb9acd1ab166bb6710a522f0285d94e1"
    );
    let base = VersionManifest::new(
        Title::new("Policy").unwrap(),
        vec![item("primary", 0, 7), item("annex", 1, 8)],
    )
    .unwrap();
    let reordered = VersionManifest::new(
        Title::new("Policy").unwrap(),
        vec![item("primary", 1, 7), item("annex", 0, 8)],
    )
    .unwrap();
    let changed_path = VersionManifest::new(
        Title::new("Policy").unwrap(),
        vec![item("main", 0, 7), item("annex", 1, 8)],
    )
    .unwrap();
    let changed_semantics = VersionManifest::new(
        Title::new("Policy").unwrap(),
        vec![item("primary", 0, 9), item("annex", 1, 8)],
    )
    .unwrap();
    assert_ne!(base.identity_digest(), reordered.identity_digest());
    assert_ne!(base.identity_digest(), changed_path.identity_digest());
    assert_ne!(base.identity_digest(), changed_semantics.identity_digest());
}

#[test]
fn one_working_base_and_rebase_rules() {
    let (mut document, base) = published_initial();
    let mut draft = working(&document, 5, 2, base.document_version_id());
    assert_eq!(
        draft.base_document_version_id(),
        Some(base.document_version_id())
    );
    assert!(document.validate_new_working(&base, None).is_ok());
    assert_eq!(
        document.validate_new_working(&base, Some(&draft)),
        Err(DomainError::ExistingWorkingVersion),
    );

    let mut replacement = working(&document, 6, 3, base.document_version_id());
    document
        .publish_next_version(
            &mut replacement,
            OffsetDateTime::from_unix_timestamp(30).unwrap(),
        )
        .unwrap();
    assert_eq!(
        draft.rebase_to_current(&document, &base),
        Err(DomainError::StaleVersionBase),
    );
    draft.rebase_to_current(&document, &replacement).unwrap();
    assert_eq!(
        draft.base_document_version_id(),
        Some(replacement.document_version_id())
    );
}

#[test]
fn withdraw_transition_restores_immediate_published_base_or_null() {
    let (mut document, base) = published_initial();
    let base_published_at = base.published_at();
    let mut next = working(&document, 5, 2, base.document_version_id());
    document
        .publish_next_version(&mut next, OffsetDateTime::from_unix_timestamp(30).unwrap())
        .unwrap();
    let withdrawn_at = OffsetDateTime::from_unix_timestamp(40).unwrap();
    let transition = document
        .withdraw_version(&mut next, Some(&base), withdrawn_at)
        .unwrap();
    assert_eq!(next.lifecycle_state(), LifecycleState::Withdrawn);
    assert_eq!(next.withdrawn_at(), Some(withdrawn_at));
    assert_eq!(base.published_at(), base_published_at);
    assert_eq!(
        document.current_version_id(),
        Some(base.document_version_id())
    );
    assert_eq!(
        transition.former_current_version_id(),
        Some(next.document_version_id())
    );
    assert_eq!(
        transition.resulting_current_version_id(),
        Some(base.document_version_id())
    );

    let (mut document, mut only) = published_initial();
    document
        .withdraw_version(&mut only, None, withdrawn_at)
        .unwrap();
    assert_eq!(document.current_version_id(), None);

    let (mut document, mut historical) = published_initial();
    let mut latest = working(&document, 7, 2, historical.document_version_id());
    document
        .publish_next_version(&mut latest, withdrawn_at)
        .unwrap();
    document
        .withdraw_version(&mut historical, None, withdrawn_at)
        .unwrap();
    assert_eq!(
        document.current_version_id(),
        Some(latest.document_version_id())
    );
}
