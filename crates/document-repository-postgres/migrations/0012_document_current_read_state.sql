-- Keep historical first_read_at and Audit unchanged; legacy INSERTs start at r1.
ALTER TABLE document_read_states
    ADD COLUMN needs_recheck BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN read_state_revision BIGINT NOT NULL DEFAULT 1
        CHECK (read_state_revision BETWEEN 1 AND 9007199254740991);

CREATE TABLE document_read_state_operations (
    identity_provider TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    operation_id UUID NOT NULL CHECK (
        substring(operation_id::text,15,1) = '7'
        AND substring(operation_id::text,20,1) IN ('8','9','a','b')
    ),
    document_id UUID NOT NULL REFERENCES documents(document_id),
    document_version_id UUID NOT NULL REFERENCES document_versions(document_version_id),
    operation_kind TEXT NOT NULL CHECK (operation_kind IN ('VIEW','RESET')),
    command_digest BYTEA NOT NULL CHECK (octet_length(command_digest) = 32),
    expected_read_state_revision BIGINT NOT NULL CHECK (expected_read_state_revision BETWEEN 0 AND 9007199254740991),
    resulting_read_state_revision BIGINT NOT NULL CHECK (resulting_read_state_revision BETWEEN 1 AND 9007199254740991),
    first_read_at TIMESTAMPTZ NOT NULL,
    needs_recheck BOOLEAN NOT NULL,
    changed BOOLEAN NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (identity_provider,principal_id,operation_id),
    CHECK ((changed AND resulting_read_state_revision = expected_read_state_revision + 1)
        OR (NOT changed AND resulting_read_state_revision = expected_read_state_revision)),
    CHECK ((operation_kind = 'RESET' AND changed AND needs_recheck)
        OR (operation_kind = 'VIEW' AND NOT needs_recheck))
);
