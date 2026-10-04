-- Human records are append-only Work data, not workflow transitions.
-- Preserve 0001/0002 checksums and every existing JSON/operation ledger record.
ALTER TABLE work.event_staging DROP CONSTRAINT event_staging_action_check;
ALTER TABLE work.event_staging ADD CONSTRAINT event_staging_action_check
    CHECK (action IN ('draft_saved', 'submitted', 'claimed', 'returned',
                     'evidence_registered', 'finding_registered', 'decision_recorded'));
