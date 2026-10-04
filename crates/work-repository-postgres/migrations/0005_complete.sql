-- Add terminal completion without rewriting earlier migrations or workflow definitions.
ALTER TABLE work.workflow_history DROP CONSTRAINT workflow_history_kind_check;
ALTER TABLE work.workflow_history ADD CONSTRAINT workflow_history_kind_check
    CHECK (kind IN ('submitted', 'claimed', 'returned', 'completed'));
ALTER TABLE work.event_staging DROP CONSTRAINT event_staging_action_check;
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_action_check
    CHECK (action IN ('draft_saved', 'submitted', 'claimed', 'returned', 'completed',
                     'evidence_registered', 'finding_registered', 'decision_recorded',
                     'agent_execution_requested', 'agent_execution_cancelled',
                     'agent_execution_running', 'agent_execution_succeeded',
                     'agent_execution_failed', 'agent_execution_interrupted',
                     'agent_execution_outcome_unknown'));
