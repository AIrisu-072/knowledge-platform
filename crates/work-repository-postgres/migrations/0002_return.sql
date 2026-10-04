-- Extend closed action vocabularies without rewriting original migration or records.
ALTER TABLE work.workflow_history DROP CONSTRAINT workflow_history_kind_check;
ALTER TABLE work.workflow_history ADD CONSTRAINT workflow_history_kind_check
    CHECK (kind IN ('submitted', 'claimed', 'returned'));
ALTER TABLE work.event_staging DROP CONSTRAINT event_staging_action_check;
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_action_check
    CHECK (action IN ('draft_saved', 'submitted', 'claimed', 'returned'));
