CREATE TABLE document_access_state (
    id SMALLINT PRIMARY KEY CHECK (id = 1),
    access_revision BIGINT NOT NULL CHECK (access_revision >= 0)
);

INSERT INTO document_access_state (id, access_revision) VALUES (1, 0);

CREATE TABLE access_policy_bindings (
    policy_id UUID PRIMARY KEY,
    folder_id UUID NULL REFERENCES folders(folder_id),
    document_id UUID NULL REFERENCES documents(document_id),
    mode TEXT NOT NULL CHECK (mode IN ('EXPLICIT', 'INHERIT')),
    revision BIGINT NOT NULL CHECK (revision >= 1),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_access_policy_binding_one_target CHECK (
        (folder_id IS NOT NULL) <> (document_id IS NOT NULL)
    ),
    CONSTRAINT uq_access_policy_binding_folder UNIQUE (folder_id),
    CONSTRAINT uq_access_policy_binding_document UNIQUE (document_id)
);

CREATE TABLE access_policy_grants (
    policy_id UUID NOT NULL REFERENCES access_policy_bindings(policy_id),
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('principal', 'group', 'role')),
    identity_provider TEXT NOT NULL CHECK (btrim(identity_provider) <> ''),
    subject_id TEXT NOT NULL CHECK (btrim(subject_id) <> ''),
    action TEXT NOT NULL CHECK (action IN (
        'read', 'read_history', 'write', 'publish', 'administer'
    )),
    PRIMARY KEY (policy_id, subject_kind, identity_provider, subject_id, action)
);

CREATE TABLE document_management_operations (
    operation_id UUID PRIMARY KEY,
    operation_kind TEXT NOT NULL CHECK (operation_kind IN (
        'update_document_metadata', 'move_document', 'create_folder',
        'rename_folder', 'move_folder', 'set_access_policy'
    )),
    resource_type TEXT NOT NULL CHECK (resource_type IN ('Document', 'Folder', 'AccessPolicy')),
    resource_id UUID NOT NULL,
    expected_revision BIGINT NOT NULL CHECK (expected_revision >= 0),
    actor_identity_provider TEXT NOT NULL CHECK (btrim(actor_identity_provider) <> ''),
    actor_principal_id TEXT NOT NULL CHECK (btrim(actor_principal_id) <> ''),
    command_digest BYTEA NOT NULL CHECK (octet_length(command_digest) = 32),
    changed BOOLEAN NOT NULL,
    result JSONB NOT NULL CHECK (jsonb_typeof(result) = 'object'),
    resulting_revision BIGINT NOT NULL CHECK (resulting_revision >= 0),
    occurred_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX ix_document_management_operations_resource
    ON document_management_operations(resource_type, resource_id, occurred_at, operation_id);

ALTER TABLE audit_outbox_events
    ADD COLUMN resource_type TEXT NOT NULL DEFAULT 'Document'
        CHECK (resource_type IN ('Document', 'Folder', 'AccessPolicy'));

-- The current policy and requested actions are evaluated in the same statement
-- as a published/authoring read. The caller supplies verified subjects only.
CREATE FUNCTION dmb_allows_document(
    target_document_id UUID,
    verified_subjects JSONB,
    required_actions TEXT[]
) RETURNS BOOLEAN LANGUAGE SQL STABLE AS $$
    WITH RECURSIVE folder_chain AS (
        SELECT d.folder_id, 1 AS depth
        FROM documents d WHERE d.document_id = target_document_id
        UNION ALL
        SELECT f.parent_folder_id, chain.depth + 1
        FROM folder_chain chain
        JOIN folders f ON f.folder_id = chain.folder_id
        WHERE f.parent_folder_id IS NOT NULL AND chain.depth < 1024
    ), candidates AS (
        SELECT b.policy_id, 0 AS distance
        FROM access_policy_bindings b
        WHERE b.document_id = target_document_id AND b.mode = 'EXPLICIT'
        UNION ALL
        SELECT b.policy_id, chain.depth
        FROM folder_chain chain
        JOIN access_policy_bindings b ON b.folder_id = chain.folder_id
        WHERE b.mode = 'EXPLICIT'
    ), nearest AS (
        SELECT policy_id FROM candidates ORDER BY distance LIMIT 1
    )
    SELECT COALESCE(
        cardinality(required_actions) > 0
        AND EXISTS (
            SELECT 1 FROM access_policy_bindings root
            WHERE root.folder_id = '00000000-0000-7000-8000-000000000001'
              AND root.mode = 'EXPLICIT'
        )
        AND EXISTS (
            SELECT 1 FROM folder_chain chain
            JOIN folders root ON root.folder_id = chain.folder_id
            WHERE root.folder_id = '00000000-0000-7000-8000-000000000001'
              AND root.parent_folder_id IS NULL
        )
        AND EXISTS (SELECT 1 FROM nearest)
        AND NOT EXISTS (
            SELECT 1 FROM unnest(required_actions) AS required(action)
            WHERE NOT EXISTS (
                SELECT 1 FROM nearest p
                JOIN access_policy_grants g ON g.policy_id = p.policy_id
                JOIN jsonb_to_recordset(verified_subjects)
                    AS subject(kind TEXT, identity_provider TEXT, subject_id TEXT)
                  ON subject.kind = g.subject_kind
                 AND subject.identity_provider = g.identity_provider
                 AND subject.subject_id = g.subject_id
                WHERE g.action = required.action
            )
        ), FALSE
    );
$$;
