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
