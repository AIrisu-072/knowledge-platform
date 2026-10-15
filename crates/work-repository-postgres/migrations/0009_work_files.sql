-- Private work files (U3): file bytes live in the Work-owned artifact store, and
-- their metadata in the workflow aggregate. Only the closed staging vocabulary
-- grows; earlier migrations, records and the history vocabulary are preserved.
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
                     'delegation_created', 'delegation_revoked',
                     'artifact_created', 'artifact_content_written', 'artifact_discarded',
                     'submission_imported'));
