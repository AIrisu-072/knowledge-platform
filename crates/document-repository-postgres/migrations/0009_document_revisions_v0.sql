CREATE TABLE IF NOT EXISTS document_revisions (
    revision_id UUID PRIMARY KEY DEFAULT uuidv7(),
    document_id UUID NOT NULL,
    document_version_id UUID NOT NULL,
    major_no BIGINT NOT NULL CHECK (major_no >= 1),
    minor_no BIGINT NOT NULL CHECK (minor_no >= 0),
    metadata_snapshot JSONB NULL,
    metadata_snapshot_status TEXT NOT NULL CHECK (
        metadata_snapshot_status IN ('complete', 'unavailable_legacy')
    ),
    source_kind TEXT NOT NULL CHECK (
        source_kind IN (
            'initialPublication',
            'contentPublication',
            'metadataRevision',
            'withdrawFallback',
            'legacyBackfill'
        )
    ),
    operation_id UUID NULL,
    created_at TIMESTAMPTZ NOT NULL,
    actor_identity_provider TEXT NULL,
    actor_principal_id TEXT NULL,
    reason TEXT NULL,
    CONSTRAINT uq_document_revisions_number
        UNIQUE (document_id, major_no, minor_no),
    CONSTRAINT fk_document_revisions_version_same_document
        FOREIGN KEY (document_id, document_version_id)
        REFERENCES document_versions(document_id, document_version_id),
    CONSTRAINT ck_document_revisions_snapshot_status CHECK (
        (
            metadata_snapshot_status = 'complete'
            AND jsonb_typeof(metadata_snapshot) = 'object'
            AND metadata_snapshot ?& ARRAY[
                'document_type', 'owning_department', 'category', 'extensions'
            ]
        )
        OR (
            metadata_snapshot_status = 'unavailable_legacy'
            AND metadata_snapshot IS NULL
            AND source_kind = 'legacyBackfill'
        )
    ),
    CONSTRAINT ck_document_revisions_provenance CHECK (
        (
            source_kind = 'legacyBackfill'
            AND operation_id IS NULL
            AND actor_identity_provider IS NULL
            AND actor_principal_id IS NULL
            AND reason IS NULL
        )
        OR (
            source_kind <> 'legacyBackfill'
            AND operation_id IS NOT NULL
            AND actor_identity_provider IS NOT NULL
            AND btrim(actor_identity_provider) <> ''
            AND actor_principal_id IS NOT NULL
            AND btrim(actor_principal_id) <> ''
        )
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_document_revisions_operation
    ON document_revisions(document_id, source_kind, operation_id)
    WHERE operation_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS ix_document_revisions_history
    ON document_revisions(document_id, major_no DESC, minor_no DESC);

CREATE OR REPLACE FUNCTION reject_document_revision_mutation()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'document revisions are append-only'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS document_revisions_append_only ON document_revisions;

CREATE TRIGGER document_revisions_append_only
    BEFORE UPDATE OR DELETE ON document_revisions
    FOR EACH ROW EXECUTE FUNCTION reject_document_revision_mutation();

WITH published_versions AS (
    SELECT
        version.document_id,
        version.document_version_id,
        version.published_at,
        row_number() OVER (
            PARTITION BY version.document_id
            ORDER BY version.published_at, version.version_no, version.document_version_id
        ) AS major_no
    FROM document_versions AS version
    WHERE version.lifecycle_state IN ('PUBLISHED', 'WITHDRAWN')
      AND version.published_at IS NOT NULL
), last_known_versions AS (
    SELECT
        document.document_id,
        COALESCE(
            (
                SELECT current_version.document_version_id
                FROM document_versions AS current_version
                WHERE current_version.document_id = document.document_id
                  AND current_version.document_version_id = document.current_version_id
                  AND current_version.lifecycle_state = 'PUBLISHED'
                  AND current_version.published_at IS NOT NULL
            ),
            (
                SELECT version.document_version_id
                FROM published_versions AS version
                WHERE version.document_id = document.document_id
                ORDER BY version.published_at DESC, version.major_no DESC
                LIMIT 1
            )
        ) AS document_version_id
    FROM documents AS document
)
INSERT INTO document_revisions (
    revision_id,
    document_id,
    document_version_id,
    major_no,
    minor_no,
    metadata_snapshot,
    metadata_snapshot_status,
    source_kind,
    operation_id,
    created_at,
    actor_identity_provider,
    actor_principal_id,
    reason
)
SELECT
    uuidv7(),
    version.document_id,
    version.document_version_id,
    version.major_no,
    0,
    CASE
        WHEN version.document_version_id = last_known.document_version_id
         AND jsonb_typeof(document.metadata) = 'object'
        THEN jsonb_build_object(
            'document_type', document.metadata->'document_type',
            'owning_department', document.metadata->'owning_department',
            'category', document.metadata->'category',
            'extensions', document.metadata->'extensions'
        )
        ELSE NULL
    END,
    CASE
        WHEN version.document_version_id = last_known.document_version_id
         AND jsonb_typeof(document.metadata) = 'object'
        THEN 'complete'
        ELSE 'unavailable_legacy'
    END,
    'legacyBackfill',
    NULL,
    version.published_at,
    NULL,
    NULL,
    NULL
FROM published_versions AS version
JOIN documents AS document USING (document_id)
JOIN last_known_versions AS last_known USING (document_id)
ON CONFLICT (document_id, major_no, minor_no) DO NOTHING;
