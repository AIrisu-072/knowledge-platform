-- Attention acknowledgment is presentation state for one principal's own current
-- assignment period. It is not a Work mutation: no workflow revision, operation
-- ledger or business staging. Earlier migrations and records are preserved.
CREATE TABLE work.attention_acknowledgements (
    principal_id text NOT NULL CHECK (principal_id IN ('sales-01', 'office-01', 'review-01',
                                                        'approver-01', 'multi-role-01', 'delegate-01')),
    work_assignment_id uuid NOT NULL,
    workflow_id uuid NOT NULL REFERENCES work.workflow_instances(id),
    task_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    acknowledged_at timestamptz NOT NULL,
    PRIMARY KEY (principal_id, work_assignment_id)
);
