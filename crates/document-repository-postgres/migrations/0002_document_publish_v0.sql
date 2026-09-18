ALTER TABLE document_versions
    ADD CONSTRAINT uq_document_versions_document_id_version_id
    UNIQUE (document_id, document_version_id);

ALTER TABLE documents
    DROP CONSTRAINT fk_documents_current_version;

ALTER TABLE documents
    ADD CONSTRAINT fk_documents_current_version
    FOREIGN KEY (document_id, current_version_id)
    REFERENCES document_versions(document_id, document_version_id);

CREATE TABLE document_publish_operations (
    publish_operation_id UUID PRIMARY KEY,
    document_id UUID NOT NULL,
    target_document_version_id UUID NOT NULL,
    expected_document_revision BIGINT NOT NULL
        CHECK (expected_document_revision >= 0),
    actor_identity_provider TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    published_at TIMESTAMPTZ NOT NULL,
    resulting_document_revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_document_publish_operations_revision
        CHECK (resulting_document_revision = expected_document_revision + 1),
    CONSTRAINT fk_document_publish_operations_target
        FOREIGN KEY (document_id, target_document_version_id)
        REFERENCES document_versions(document_id, document_version_id)
);
