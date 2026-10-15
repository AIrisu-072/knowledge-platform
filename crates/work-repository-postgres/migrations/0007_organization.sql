-- Organization policy is a separate aggregate with its own revision. Work commands
-- read it under a share lock; policy writers take the row update lock (fencing).
-- Earlier migrations, checksums and every existing record are preserved.
CREATE TABLE work.organization_policies (
    id uuid PRIMARY KEY,
    revision bigint NOT NULL CHECK (revision >= 0),
    body jsonb NOT NULL CHECK (jsonb_typeof(body) = 'object')
);
ALTER TABLE work.operation_ledger ALTER COLUMN workflow_id DROP NOT NULL;
ALTER TABLE work.operation_ledger ADD COLUMN policy_id uuid REFERENCES work.organization_policies(id);
ALTER TABLE work.operation_ledger ADD CONSTRAINT operation_ledger_target_check
    CHECK ((workflow_id IS NULL) <> (policy_id IS NULL));
ALTER TABLE work.operation_ledger DROP CONSTRAINT operation_ledger_principal_id_check;
ALTER TABLE work.operation_ledger ADD CONSTRAINT operation_ledger_principal_id_check
    CHECK (principal_id IN ('sales-01', 'office-01', 'review-01', 'approver-01',
                            'multi-role-01', 'delegate-01'));
ALTER TABLE work.event_staging ALTER COLUMN workflow_id DROP NOT NULL;
ALTER TABLE work.event_staging ALTER COLUMN task_id DROP NOT NULL;
ALTER TABLE work.event_staging ADD COLUMN policy_id uuid REFERENCES work.organization_policies(id);
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_target_check
    CHECK ((workflow_id IS NULL) <> (policy_id IS NULL)
           AND (workflow_id IS NULL OR task_id IS NOT NULL));
ALTER TABLE work.event_staging DROP CONSTRAINT event_staging_principal_id_check;
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_principal_id_check
    CHECK (principal_id IN ('sales-01', 'office-01', 'review-01', 'approver-01',
                            'multi-role-01', 'delegate-01'));
ALTER TABLE work.event_staging DROP CONSTRAINT event_staging_action_check;
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_action_check
    CHECK (action IN ('draft_saved', 'submitted', 'claimed', 'returned', 'completed', 'held', 'resumed',
                     'assigned',
                     'evidence_registered', 'finding_registered', 'decision_recorded',
                     'agent_execution_requested', 'agent_execution_cancelled',
                     'agent_execution_running', 'agent_execution_succeeded',
                     'agent_execution_failed', 'agent_execution_interrupted',
                     'agent_execution_outcome_unknown',
                     'role_assignment_created', 'role_assignment_revoked',
                     'delegation_created', 'delegation_revoked'));
ALTER TABLE work.workflow_history DROP CONSTRAINT workflow_history_kind_check;
ALTER TABLE work.workflow_history ADD CONSTRAINT workflow_history_kind_check
    CHECK (kind IN ('submitted', 'claimed', 'returned', 'completed', 'held', 'resumed', 'assigned'));
