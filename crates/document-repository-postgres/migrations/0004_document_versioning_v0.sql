ALTER TABLE document_versions
    ADD COLUMN base_document_version_id UUID NULL,
    ADD COLUMN requires_content_classification BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE document_versions
    ADD CONSTRAINT fk_document_versions_base_same_document
    FOREIGN KEY (document_id, base_document_version_id)
    REFERENCES document_versions(document_id, document_version_id);

CREATE UNIQUE INDEX uq_document_versions_one_working
    ON document_versions(document_id)
    WHERE lifecycle_state = 'WORKING';

CREATE TABLE content_items (
    content_item_id UUID PRIMARY KEY,
    document_version_id UUID NOT NULL REFERENCES document_versions(document_version_id),
    logical_path TEXT NOT NULL CHECK (
        logical_path <> ''
        AND left(logical_path, 1) <> '/'
        AND right(logical_path, 1) <> '/'
        AND position('//' in logical_path) = 0
    ),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    authoritative_representation_id UUID NOT NULL,
    authoritative_role TEXT NOT NULL DEFAULT 'AUTHORITATIVE'
        CHECK (authoritative_role = 'AUTHORITATIVE'),
    CONSTRAINT uq_content_items_manifest_key
        UNIQUE (document_version_id, logical_path, ordinal)
);

CREATE TABLE content_representations (
    content_representation_id UUID PRIMARY KEY,
    content_item_id UUID NOT NULL REFERENCES content_items(content_item_id) ON DELETE CASCADE,
    file_id UUID NOT NULL REFERENCES file_objects(file_id),
    role TEXT NOT NULL CHECK (role IN ('AUTHORITATIVE', 'RENDITION')),
    original_filename TEXT NOT NULL CHECK (btrim(original_filename) <> ''),
    detected_format TEXT NULL,
    inspection_profile_version TEXT NULL,
    semantic_fingerprint BYTEA NULL CHECK (
        semantic_fingerprint IS NULL OR octet_length(semantic_fingerprint) = 32
    ),
    CONSTRAINT uq_content_representations_item_id_role
        UNIQUE (content_item_id, content_representation_id, role)
);

CREATE UNIQUE INDEX uq_content_representations_one_authoritative
    ON content_representations(content_item_id)
    WHERE role = 'AUTHORITATIVE';

ALTER TABLE content_items
    ADD CONSTRAINT fk_content_items_authoritative_representation
    FOREIGN KEY (content_item_id, authoritative_representation_id, authoritative_role)
    REFERENCES content_representations(content_item_id, content_representation_id, role)
    DEFERRABLE INITIALLY DEFERRED;

UPDATE document_versions AS version
SET requires_content_classification = TRUE
WHERE (
    SELECT count(*)
    FROM version_files AS file
    WHERE file.document_version_id = version.document_version_id
      AND file.role = 'PRIMARY'
) <> 1
OR EXISTS (
    SELECT 1
    FROM version_files AS file
    WHERE file.document_version_id = version.document_version_id
      AND file.role = 'ATTACHMENT'
);

CREATE TEMP TABLE _versioning_primary_backfill (
    content_item_id UUID NOT NULL,
    representation_id UUID NOT NULL,
    document_version_id UUID NOT NULL,
    file_id UUID NOT NULL,
    original_filename TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _versioning_primary_backfill
    (content_item_id, representation_id, document_version_id, file_id, original_filename)
SELECT gen_random_uuid(), gen_random_uuid(), file.document_version_id,
       file.file_id, file.original_filename
FROM version_files AS file
JOIN document_versions AS version
  ON version.document_version_id = file.document_version_id
WHERE file.role = 'PRIMARY'
  AND version.requires_content_classification = FALSE;

INSERT INTO content_items
    (content_item_id, document_version_id, logical_path, ordinal,
     authoritative_representation_id)
SELECT content_item_id, document_version_id, 'primary', 0, representation_id
FROM _versioning_primary_backfill;

INSERT INTO content_representations
    (content_representation_id, content_item_id, file_id, role, original_filename)
SELECT representation_id, content_item_id, file_id, 'AUTHORITATIVE', original_filename
FROM _versioning_primary_backfill;

CREATE TABLE document_version_operations (
    operation_id UUID PRIMARY KEY,
    operation_kind TEXT NOT NULL CHECK (operation_kind IN (
        'CREATE', 'UPDATE', 'REBASE', 'WITHDRAW', 'CANCEL_SCHEDULE'
    )),
    document_id UUID NOT NULL REFERENCES documents(document_id),
    target_document_version_id UUID NOT NULL,
    expected_document_revision BIGINT NOT NULL CHECK (expected_document_revision >= 0),
    actor_identity_provider TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    command_digest BYTEA NOT NULL CHECK (octet_length(command_digest) = 32),
    result JSONB NOT NULL CHECK (jsonb_typeof(result) = 'object'),
    resulting_document_revision BIGINT NOT NULL CHECK (resulting_document_revision >= 0),
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT fk_document_version_operations_target
        FOREIGN KEY (document_id, target_document_version_id)
        REFERENCES document_versions(document_id, document_version_id)
);

CREATE TABLE document_publish_schedules (
    publish_operation_id UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(document_id),
    target_document_version_id UUID NOT NULL,
    base_document_version_id UUID NULL,
    current_version_id UUID NULL,
    expected_document_revision BIGINT NOT NULL CHECK (expected_document_revision >= 0),
    accepted_document_revision BIGINT NOT NULL CHECK (
        accepted_document_revision = expected_document_revision + 1
    ),
    scheduled_publish_at TIMESTAMPTZ NOT NULL,
    actor_identity_provider TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    manifest_digest BYTEA NOT NULL CHECK (octet_length(manifest_digest) = 32),
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'PUBLISHED', 'CANCELLED', 'TERMINAL')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_attempt_at TIMESTAMPTZ NULL,
    next_retry_at TIMESTAMPTZ NULL,
    terminal_reason TEXT NULL,
    published_at TIMESTAMPTZ NULL,
    cancelled_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_document_publish_schedules_base_current
        CHECK (base_document_version_id IS NOT DISTINCT FROM current_version_id),
    CONSTRAINT fk_document_publish_schedules_target
        FOREIGN KEY (document_id, target_document_version_id)
        REFERENCES document_versions(document_id, document_version_id),
    CONSTRAINT fk_document_publish_schedules_base
        FOREIGN KEY (document_id, base_document_version_id)
        REFERENCES document_versions(document_id, document_version_id)
);

CREATE UNIQUE INDEX uq_document_publish_schedules_pending_document
    ON document_publish_schedules(document_id)
    WHERE status = 'PENDING';

CREATE INDEX ix_document_publish_schedules_due
    ON document_publish_schedules(scheduled_publish_at, publish_operation_id)
    WHERE status = 'PENDING';
