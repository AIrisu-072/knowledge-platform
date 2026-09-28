CREATE TABLE document_read_states (
    identity_provider TEXT NOT NULL CHECK (btrim(identity_provider) <> ''),
    principal_id TEXT NOT NULL CHECK (btrim(principal_id) <> ''),
    document_version_id UUID NOT NULL REFERENCES document_versions(document_version_id),
    first_read_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (identity_provider, principal_id, document_version_id)
);

CREATE INDEX ix_document_read_states_version
    ON document_read_states(document_version_id);
