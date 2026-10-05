-- Keep original migration checksums/digests. Internal execution transitions share
-- the atomic Work aggregate/staging transaction but are not Human commands.
ALTER TABLE work.event_staging ALTER COLUMN operation_id DROP NOT NULL;
ALTER TABLE work.event_staging DROP CONSTRAINT event_staging_action_check;
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_action_check
    CHECK (action IN ('draft_saved', 'submitted', 'claimed', 'returned',
                     'evidence_registered', 'finding_registered', 'decision_recorded',
                     'agent_execution_requested', 'agent_execution_cancelled',
                     'agent_execution_running', 'agent_execution_succeeded',
                     'agent_execution_failed', 'agent_execution_interrupted',
                     'agent_execution_outcome_unknown'));
