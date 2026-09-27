CREATE TABLE document_publication_end_operations (
    operation_id UUID PRIMARY KEY,
    document_id UUID NOT NULL UNIQUE REFERENCES documents(document_id),
    command_digest BYTEA NOT NULL CHECK (octet_length(command_digest) = 32),
    expected_document_revision BIGINT NOT NULL CHECK (expected_document_revision >= 0),
    expected_current_version_id UUID NOT NULL,
    actor_identity_provider TEXT NOT NULL CHECK (btrim(actor_identity_provider) <> ''),
    actor_principal_id TEXT NOT NULL CHECK (btrim(actor_principal_id) <> ''),
    reason TEXT NOT NULL CHECK (btrim(reason) <> ''),
    former_current_version_id UUID NOT NULL,
    resulting_document_revision BIGINT NOT NULL CHECK (
        resulting_document_revision = expected_document_revision + 1
    ),
    ended_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_publication_end_former_matches_expected
        CHECK (former_current_version_id = expected_current_version_id),
    CONSTRAINT fk_publication_end_expected_same_document
        FOREIGN KEY (document_id, expected_current_version_id)
        REFERENCES document_versions(document_id, document_version_id),
    CONSTRAINT fk_publication_end_former_same_document
        FOREIGN KEY (document_id, former_current_version_id)
        REFERENCES document_versions(document_id, document_version_id)
);
