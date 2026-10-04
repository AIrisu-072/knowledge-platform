CREATE TABLE work.workflow_instances (
    id uuid PRIMARY KEY,
    revision bigint NOT NULL CHECK (revision >= 0),
    body jsonb NOT NULL CHECK (jsonb_typeof(body) = 'object')
);
CREATE TABLE work.operation_ledger (
    operation_id uuid PRIMARY KEY,
    workflow_id uuid NOT NULL REFERENCES work.workflow_instances(id),
    principal_id text NOT NULL CHECK (principal_id IN ('sales-01', 'office-01')),
    acting_assignment_id uuid NOT NULL,
    command_digest bytea NOT NULL CHECK (octet_length(command_digest) = 32),
    outcome jsonb NOT NULL,
    committed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE work.workflow_history (
    id uuid PRIMARY KEY,
    workflow_id uuid NOT NULL REFERENCES work.workflow_instances(id),
    operation_id uuid NOT NULL UNIQUE REFERENCES work.operation_ledger(operation_id),
    kind text NOT NULL CHECK (kind IN ('submitted', 'claimed')),
    occurred_at timestamptz NOT NULL
);
CREATE TABLE work.event_staging (
    id uuid PRIMARY KEY,
    operation_id uuid NOT NULL UNIQUE REFERENCES work.operation_ledger(operation_id),
    workflow_id uuid NOT NULL REFERENCES work.workflow_instances(id),
    principal_id text NOT NULL CHECK (principal_id IN ('sales-01', 'office-01')),
    acting_assignment_id uuid NOT NULL,
    task_id uuid NOT NULL,
    action text NOT NULL CHECK (action IN ('draft_saved', 'submitted', 'claimed')),
    occurred_at timestamptz NOT NULL,
    payload jsonb NOT NULL
);
