CREATE TABLE document_read_states (
    identity_provider TEXT NOT NULL CHECK (btrim(identity_provider) <> ''),
    principal_id TEXT NOT NULL CHECK (btrim(principal_id) <> ''),
    document_version_id UUID NOT NULL REFERENCES document_versions(document_version_id),
    first_read_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (identity_provider, principal_id, document_version_id)
);

CREATE INDEX ix_document_read_states_version
    ON document_read_states(document_version_id);

ALTER TABLE document_publish_schedules
    ADD COLUMN terminal_at TIMESTAMPTZ NULL,
    ADD COLUMN terminal_executor_identity_provider TEXT NULL,
    ADD COLUMN terminal_executor_principal_id TEXT NULL,
    ADD CONSTRAINT ck_document_publish_schedule_terminal_executor_pair CHECK (
        (terminal_executor_identity_provider IS NULL)
        = (terminal_executor_principal_id IS NULL)
    );

-- Folder names are visible only when the same nearest-policy evaluator grants Read.
CREATE FUNCTION dmb_allows_folder(
    target_folder_id UUID,
    verified_subjects JSONB,
    required_actions TEXT[]
) RETURNS BOOLEAN LANGUAGE SQL STABLE AS $$
    WITH RECURSIVE folder_chain AS (
        SELECT f.folder_id, f.parent_folder_id, 0 AS depth
        FROM folders f WHERE f.folder_id = target_folder_id
        UNION ALL
        SELECT f.folder_id, f.parent_folder_id, chain.depth + 1
        FROM folder_chain chain
        JOIN folders f ON f.folder_id = chain.parent_folder_id
        WHERE chain.depth < 1024
    ), nearest AS (
        SELECT b.policy_id FROM folder_chain chain
        JOIN access_policy_bindings b ON b.folder_id = chain.folder_id
        WHERE b.mode = 'EXPLICIT'
        ORDER BY chain.depth LIMIT 1
    )
    SELECT COALESCE(
        cardinality(required_actions) > 0
        AND EXISTS (
            SELECT 1 FROM access_policy_bindings root
            WHERE root.folder_id = '00000000-0000-7000-8000-000000000001'
              AND root.mode = 'EXPLICIT'
        )
        AND EXISTS (
            SELECT 1 FROM folder_chain
            WHERE folder_id = '00000000-0000-7000-8000-000000000001'
              AND parent_folder_id IS NULL
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
