-- Generic Domain outbox delivery metadata. Existing events and Audit delivery
-- remain unchanged; historical events acquire an attempt limit only on claim.
ALTER TABLE outbox_events
    ADD COLUMN lease_token UUID NULL,
    ADD COLUMN lease_owner UUID NULL,
    ADD COLUMN lease_expires_at TIMESTAMPTZ NULL,
    ADD COLUMN last_attempt_at TIMESTAMPTZ NULL,
    ADD COLUMN dead_lettered_at TIMESTAMPTZ NULL,
    ADD COLUMN last_error_code TEXT NULL,
    ADD COLUMN attempt_limit INTEGER NULL,
    ADD COLUMN traceparent TEXT NULL,
    ADD COLUMN tracestate TEXT NULL;

ALTER TABLE outbox_events
    ADD CONSTRAINT ck_outbox_attempt_limit
        CHECK (attempt_limit IS NULL OR attempt_limit > 0),
    ADD CONSTRAINT ck_outbox_lease_complete
        CHECK (
            (lease_token IS NULL AND lease_owner IS NULL AND lease_expires_at IS NULL)
            OR (lease_token IS NOT NULL AND lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)
        ),
    ADD CONSTRAINT ck_outbox_one_terminal
        CHECK (delivered_at IS NULL OR dead_lettered_at IS NULL),
    ADD CONSTRAINT ck_outbox_terminal_unleased
        CHECK ((delivered_at IS NULL AND dead_lettered_at IS NULL) OR lease_token IS NULL),
    ADD CONSTRAINT ck_outbox_error_code
        CHECK (
            last_error_code IS NULL OR last_error_code IN (
                'unsupported_event',
                'invalid_envelope',
                'source_unavailable',
                'indexing_failed',
                'handler_timeout',
                'delivery_unknown',
                'delivery_unknown_at_limit'
            )
        );

CREATE TABLE outbox_delivery_policy (
    policy_id SMALLINT PRIMARY KEY CHECK (policy_id = 1),
    revision BIGINT NOT NULL CHECK (revision > 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts BETWEEN 1 AND 32),
    lease_min_ms BIGINT NOT NULL CHECK (lease_min_ms BETWEEN 1000 AND 120000),
    lease_max_ms BIGINT NOT NULL CHECK (
        lease_max_ms BETWEEN 1000 AND 120000 AND lease_max_ms >= lease_min_ms
    ),
    backoff_min_ms BIGINT NOT NULL CHECK (backoff_min_ms BETWEEN 1000 AND 300000),
    backoff_max_ms BIGINT NOT NULL CHECK (
        backoff_max_ms BETWEEN 1000 AND 300000 AND backoff_max_ms >= backoff_min_ms
    )
);

INSERT INTO outbox_delivery_policy
    (policy_id, revision, max_attempts, lease_min_ms, lease_max_ms, backoff_min_ms, backoff_max_ms)
VALUES (1, 1, 8, 1000, 120000, 1000, 300000);

-- Candidate priority is not a guarantee of aggregate delivery order.
CREATE INDEX idx_outbox_delivery_candidate
    ON outbox_events (available_at, occurred_at, event_id)
    WHERE delivered_at IS NULL AND dead_lettered_at IS NULL;

-- Expired final claims stay visible for the bounded reaper, even after a crash.
CREATE INDEX idx_outbox_delivery_exhausted_reap
    ON outbox_events (last_attempt_at ASC NULLS FIRST, event_id)
    WHERE delivered_at IS NULL AND dead_lettered_at IS NULL
      AND attempt_limit IS NOT NULL AND attempt_count >= attempt_limit;

CREATE VIEW outbox_delivery_state AS
WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS observed_at)
SELECT
    o.event_id,
    CASE
        WHEN o.delivered_at IS NOT NULL THEN 'DELIVERED'
        WHEN o.dead_lettered_at IS NOT NULL THEN 'DEAD_LETTER'
        WHEN o.lease_token IS NOT NULL AND o.lease_expires_at > tick.observed_at THEN 'IN_FLIGHT'
        ELSE 'PENDING'
    END AS processing_state,
    (
        o.delivered_at IS NULL AND o.dead_lettered_at IS NULL
        AND o.attempt_limit IS NOT NULL AND o.attempt_count >= o.attempt_limit
        AND (o.lease_token IS NULL OR o.lease_expires_at <= tick.observed_at)
    ) AS recovery_pending
FROM outbox_events AS o CROSS JOIN tick;
