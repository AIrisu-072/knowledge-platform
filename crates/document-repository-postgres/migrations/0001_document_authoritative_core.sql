CREATE TABLE folders (
    folder_id UUID PRIMARY KEY,
    parent_folder_id UUID NULL REFERENCES folders(folder_id),
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision >= 0),
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE documents (
    document_id UUID PRIMARY KEY,
    folder_id UUID NOT NULL REFERENCES folders(folder_id),
    current_version_id UUID NULL,
    revision BIGINT NOT NULL CHECK (revision >= 0),
    metadata JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE document_versions (
    document_version_id UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(document_id),
    version_no BIGINT NOT NULL CHECK (version_no >= 1),
    lifecycle_state TEXT NOT NULL CHECK (
        lifecycle_state IN ('WORKING', 'PUBLISHED', 'WITHDRAWN')
    ),
    title TEXT NOT NULL,
    revision_reason TEXT NULL,
    approved_at TIMESTAMPTZ NULL,
    scheduled_publish_at TIMESTAMPTZ NULL,
    published_at TIMESTAMPTZ NULL,
    withdrawn_at TIMESTAMPTZ NULL,
    effective_from TIMESTAMPTZ NULL,
    effective_to TIMESTAMPTZ NULL,
    created_by_identity_provider TEXT NOT NULL,
    created_by_principal_id TEXT NOT NULL,
    metadata JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_document_versions_document_version_no UNIQUE (document_id, version_no),
    CONSTRAINT ck_document_versions_published_at CHECK (
        lifecycle_state <> 'PUBLISHED' OR published_at IS NOT NULL
    ),
    CONSTRAINT ck_document_versions_withdrawn_at CHECK (
        lifecycle_state <> 'WITHDRAWN' OR withdrawn_at IS NOT NULL
    )
);

ALTER TABLE documents
    ADD CONSTRAINT fk_documents_current_version
    FOREIGN KEY (current_version_id)
    REFERENCES document_versions(document_version_id);

CREATE TABLE file_objects (
    file_id UUID PRIMARY KEY,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    media_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    storage_locator TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE version_files (
    document_version_id UUID NOT NULL REFERENCES document_versions(document_version_id),
    file_id UUID NOT NULL REFERENCES file_objects(file_id),
    role TEXT NOT NULL CHECK (role IN ('PRIMARY', 'ATTACHMENT')),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    original_filename TEXT NOT NULL,
    PRIMARY KEY (document_version_id, file_id)
);

CREATE UNIQUE INDEX uq_version_files_primary
    ON version_files(document_version_id)
    WHERE role = 'PRIMARY';

CREATE TABLE outbox_events (
    event_id UUID PRIMARY KEY,
    event_type TEXT NOT NULL,
    aggregate_type TEXT NOT NULL,
    aggregate_id UUID NOT NULL,
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    available_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    delivered_at TIMESTAMPTZ NULL
);

CREATE TABLE audit_outbox_events (
    event_id UUID PRIMARY KEY,
    event_type TEXT NOT NULL,
    source TEXT NOT NULL,
    subject TEXT NOT NULL,
    actor_identity_provider TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    resource_id UUID NOT NULL,
    resource_version_id UUID NULL,
    result TEXT NOT NULL,
    trace_id TEXT NULL,
    data JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    delivered_at TIMESTAMPTZ NULL
);

INSERT INTO folders (
    folder_id,
    parent_folder_id,
    name,
    status,
    revision,
    created_at
) VALUES (
    '00000000-0000-7000-8000-000000000001',
    NULL,
    'System Root',
    'ACTIVE',
    0,
    now()
);
