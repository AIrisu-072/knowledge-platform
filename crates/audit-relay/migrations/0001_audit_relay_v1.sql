-- Audit relay v1 on the Document database (design 2026-10-07 revision 2,
-- §5, §6, §12; decision record D4).
--
-- Ledger audit_relay_sqlx_migrations; Document's _sqlx_migrations is never
-- written. Run after the Document migrations by a superuser (or the owner of
-- public.audit_outbox_events that may SET ROLE audit_relay_owner).
--
-- The whole migration is one transaction. Its first statements take
-- SHARE ROW EXCLUSIVE on the staging table: concurrent producer INSERTs wait
-- until commit, so the backfill and the registration trigger leave no gap.
-- Producer INSERTs are blocked for the duration of this migration.

SET TRANSACTION ISOLATION LEVEL READ COMMITTED;
SET LOCAL lock_timeout = '10s';
LOCK TABLE public.audit_outbox_events IN SHARE ROW EXCLUSIVE MODE;

-- ---------------------------------------------------------------------------
-- Preflight (repeated in Rust before the migrator runs)
-- ---------------------------------------------------------------------------

DO $preflight$
DECLARE
    v_missing TEXT;
BEGIN
    IF pg_catalog.to_regclass('public._sqlx_migrations') IS NULL THEN
        RAISE EXCEPTION 'audit relay preflight: the Document migration ledger is missing'
            USING ERRCODE = '55000';
    END IF;
    SELECT string_agg(e.name, ',' ORDER BY e.name) INTO v_missing
    FROM (VALUES ('event_id', 'uuid'), ('event_type', 'text'), ('source', 'text'),
                 ('subject', 'text'), ('actor_identity_provider', 'text'),
                 ('actor_principal_id', 'text'), ('resource_type', 'text'),
                 ('resource_id', 'uuid'), ('resource_version_id', 'uuid'),
                 ('result', 'text'), ('trace_id', 'text'), ('data', 'jsonb'),
                 ('occurred_at', 'timestamp with time zone'), ('attempt_count', 'integer'),
                 ('delivered_at', 'timestamp with time zone')) AS e(name, type)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_attribute AS a
        WHERE a.attrelid = 'public.audit_outbox_events'::regclass
          AND a.attname = e.name AND NOT a.attisdropped AND a.attnum > 0
          AND pg_catalog.format_type(a.atttypid, a.atttypmod) = e.type);
    IF v_missing IS NOT NULL THEN
        RAISE EXCEPTION 'audit relay preflight: public.audit_outbox_events lacks columns %',
            v_missing USING ERRCODE = '55000';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_index AS i
        JOIN pg_catalog.pg_attribute AS a
          ON a.attrelid = i.indrelid AND a.attnum = i.indkey[0]
        WHERE i.indrelid = 'public.audit_outbox_events'::regclass
          AND i.indisprimary AND i.indnatts = 1 AND a.attname = 'event_id') THEN
        RAISE EXCEPTION 'audit relay preflight: event_id is not the primary key'
            USING ERRCODE = '55000';
    END IF;
END
$preflight$;

-- ---------------------------------------------------------------------------
-- Owner role and schema (design §5.3: NOLOGIN, non-superuser owner)
-- ---------------------------------------------------------------------------

DO $owner$
DECLARE
    v_role record;
BEGIN
    SELECT r.rolsuper, r.rolcanlogin INTO v_role
    FROM pg_catalog.pg_roles AS r WHERE r.rolname = 'audit_relay_owner';
    IF NOT FOUND THEN
        BEGIN
            CREATE ROLE audit_relay_owner
                NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
        EXCEPTION WHEN duplicate_object THEN
            NULL;
        END;
    ELSIF v_role.rolsuper OR v_role.rolcanlogin THEN
        RAISE EXCEPTION 'audit_relay_owner must be NOLOGIN and NOSUPERUSER';
    END IF;
END
$owner$;

-- The Document owner lends read access and the FK target (design §10.1).
GRANT SELECT, REFERENCES ON public.audit_outbox_events TO audit_relay_owner;

CREATE SCHEMA audit_relay AUTHORIZATION audit_relay_owner;
REVOKE ALL ON SCHEMA audit_relay FROM PUBLIC;

-- Every audit_relay object below is created by, and owned by, the owner.
SET LOCAL ROLE audit_relay_owner;

-- ---------------------------------------------------------------------------
-- Tables (design §5.1)
-- ---------------------------------------------------------------------------

CREATE TABLE audit_relay.delivery_policy (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    revision BIGINT NOT NULL,
    max_attempts INTEGER NOT NULL,
    lease_min_ms BIGINT NOT NULL,
    lease_max_ms BIGINT NOT NULL,
    backoff_min_ms BIGINT NOT NULL,
    backoff_max_ms BIGINT NOT NULL,
    outage_streak_limit INTEGER NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_policy_singleton CHECK (singleton),
    CONSTRAINT ck_policy_revision CHECK (revision > 0),
    CONSTRAINT ck_policy_attempts CHECK (max_attempts BETWEEN 1 AND 32),
    CONSTRAINT ck_policy_lease CHECK (lease_min_ms BETWEEN 1000 AND 120000
                                      AND lease_max_ms BETWEEN lease_min_ms AND 120000),
    CONSTRAINT ck_policy_backoff CHECK (backoff_min_ms BETWEEN 1000 AND 300000
                                        AND backoff_max_ms BETWEEN backoff_min_ms AND 300000),
    CONSTRAINT ck_policy_streak CHECK (outage_streak_limit BETWEEN 1 AND 1024)
);

INSERT INTO audit_relay.delivery_policy (
    singleton, revision, max_attempts, lease_min_ms, lease_max_ms, backoff_min_ms,
    backoff_max_ms, outage_streak_limit, updated_at)
VALUES (TRUE, 1, 16, 1000, 120000, 1000, 300000, 64, clock_timestamp());

-- A persisted success generation: outage_streak counts an outage only when
-- another delivery succeeded since the row's previous outage.
CREATE TABLE audit_relay.delivery_progress (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    success_generation BIGINT NOT NULL,
    CONSTRAINT ck_progress_singleton CHECK (singleton),
    CONSTRAINT ck_progress_generation CHECK (success_generation >= 0)
);

INSERT INTO audit_relay.delivery_progress (singleton, success_generation) VALUES (TRUE, 0);

CREATE TABLE audit_relay.deliveries (
    event_id UUID PRIMARY KEY REFERENCES public.audit_outbox_events (event_id),
    registered_at TIMESTAMPTZ NOT NULL,
    registration_kind TEXT NOT NULL,
    source_digest BYTEA NOT NULL,
    commitment_salt BYTEA NOT NULL,
    legacy_attempt_count INTEGER NULL,
    legacy_delivered_at TIMESTAMPTZ NULL,
    available_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    attempt_limit INTEGER NULL,
    lease_token UUID NULL,
    lease_owner UUID NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    last_attempt_at TIMESTAMPTZ NULL,
    last_error_code TEXT NULL,
    last_outage_code TEXT NULL,
    outage_streak INTEGER NOT NULL DEFAULT 0,
    last_outage_generation BIGINT NULL,
    last_outage_at TIMESTAMPTZ NULL,
    delivered_at TIMESTAMPTZ NULL,
    store_seq BIGINT NULL,
    store_envelope_digest BYTEA NULL,
    store_outcome TEXT NULL,
    store_recovery_epoch BIGINT NULL,
    source_mismatch_code TEXT NULL,
    source_mismatch_seq BIGINT NULL,
    quarantined_at TIMESTAMPTZ NULL,
    quarantine_code TEXT NULL,
    replay_count INTEGER NOT NULL DEFAULT 0,
    CONSTRAINT ck_deliveries_kind CHECK (registration_kind IN ('trigger', 'backfill', 'repair')),
    CONSTRAINT ck_deliveries_digest CHECK (octet_length(source_digest) = 32),
    CONSTRAINT ck_deliveries_salt CHECK (octet_length(commitment_salt) = 32),
    CONSTRAINT ck_deliveries_attempts CHECK (attempt_count >= 0
        AND (attempt_limit IS NULL OR attempt_limit BETWEEN 1 AND 32)),
    CONSTRAINT ck_deliveries_lease CHECK (
        (lease_token IS NULL) = (lease_owner IS NULL)
        AND (lease_token IS NULL) = (lease_expires_at IS NULL)),
    CONSTRAINT ck_deliveries_codes CHECK (
        (last_error_code IS NULL OR last_error_code ~ '^[a-z0-9_]{1,64}$')
        AND (last_outage_code IS NULL OR last_outage_code ~ '^[a-z0-9_]{1,64}$')
        AND (quarantine_code IS NULL OR quarantine_code ~ '^[a-z0-9_]{1,64}$')
        AND (source_mismatch_code IS NULL
             OR source_mismatch_code IN ('source_digest_mismatch', 'actor_mismatch'))),
    CONSTRAINT ck_deliveries_streak CHECK (outage_streak >= 0 AND replay_count >= 0),
    CONSTRAINT ck_deliveries_receipt CHECK (
        (delivered_at IS NULL) = (store_seq IS NULL)
        AND (delivered_at IS NULL) = (store_envelope_digest IS NULL)
        AND (delivered_at IS NULL) = (store_outcome IS NULL)
        AND (delivered_at IS NULL) = (store_recovery_epoch IS NULL)
        AND (store_seq IS NULL OR store_seq > 0)
        AND (store_envelope_digest IS NULL OR octet_length(store_envelope_digest) = 32)
        AND (store_outcome IS NULL OR store_outcome IN
             ('stored', 'duplicate', 'duplicate_expired', 'duplicate_reprojected'))),
    CONSTRAINT ck_deliveries_mismatch CHECK (
        (source_mismatch_code IS NULL) = (source_mismatch_seq IS NULL)),
    CONSTRAINT ck_deliveries_quarantine CHECK ((quarantined_at IS NULL) = (quarantine_code IS NULL)),
    CONSTRAINT ck_deliveries_terminal CHECK (
        NOT (delivered_at IS NOT NULL AND quarantined_at IS NOT NULL)
        AND (lease_token IS NULL OR (delivered_at IS NULL AND quarantined_at IS NULL)))
);

CREATE INDEX ix_deliveries_claimable ON audit_relay.deliveries
    (available_at, registered_at, event_id)
    WHERE delivered_at IS NULL AND quarantined_at IS NULL;
CREATE INDEX ix_deliveries_acked ON audit_relay.deliveries (store_recovery_epoch, store_seq)
    WHERE delivered_at IS NOT NULL;

-- Terminal evidence preserved before replay/repair (design §5.1, §6.4).
-- control_seq is the Store seq of audit.delivery.replay_requested (one to
-- one); reconcile_seq is the Store seq of the repair run's
-- audit.reconciliation.completed; control_epoch is the Store recovery epoch
-- in which that control event was recorded.
CREATE TABLE audit_relay.delivery_history (
    history_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id UUID NOT NULL REFERENCES audit_relay.deliveries (event_id),
    transition TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL,
    control_seq BIGINT NULL UNIQUE,
    control_epoch BIGINT NOT NULL,
    reconcile_seq BIGINT NULL,
    attempt_count INTEGER NOT NULL,
    attempt_limit INTEGER NULL,
    last_attempt_at TIMESTAMPTZ NULL,
    last_error_code TEXT NULL,
    outage_streak INTEGER NOT NULL,
    quarantine_code TEXT NULL,
    quarantined_at TIMESTAMPTZ NULL,
    delivered_at TIMESTAMPTZ NULL,
    store_seq BIGINT NULL,
    store_envelope_digest BYTEA NULL,
    store_outcome TEXT NULL,
    store_recovery_epoch BIGINT NULL,
    CONSTRAINT ck_history_transition CHECK (
        transition IN ('replay', 'repair_ack_stored', 'repair_reset_missing')),
    CONSTRAINT ck_history_reference CHECK (
        control_epoch >= 1
        AND ((transition = 'replay' AND control_seq > 0 AND reconcile_seq IS NULL)
             OR (transition <> 'replay' AND control_seq IS NULL AND reconcile_seq > 0)))
);

CREATE INDEX ix_history_event ON audit_relay.delivery_history (event_id, history_id);

-- ---------------------------------------------------------------------------
-- Source digest and commitment (design §5.2)
-- ---------------------------------------------------------------------------

-- SQL-standard body: PostgreSQL records a dependency on every digested
-- column, so a Document migration that drops or retypes one fails at migrate
-- time (decision record D4). Adding a column is unaffected.
CREATE FUNCTION audit_relay.source_digest(o public.audit_outbox_events)
RETURNS BYTEA LANGUAGE sql STABLE PARALLEL SAFE
SET search_path = pg_catalog, pg_temp
BEGIN ATOMIC
    SELECT pg_catalog.sha256(pg_catalog.convert_to(pg_catalog.jsonb_build_array(
        'kp-audit-source-v1'::text, o.event_id, o.event_type, o.source, o.subject,
        o.actor_identity_provider, o.actor_principal_id, o.resource_type, o.resource_id,
        o.resource_version_id, o.result, o.trace_id, o.data,
        pg_catalog.to_char((o.occurred_at AT TIME ZONE 'UTC'),
                           'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'))::text, 'UTF8'));
END;

CREATE FUNCTION audit_relay.commitment(p_salt BYTEA, p_digest BYTEA)
RETURNS BYTEA LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path = pg_catalog, pg_temp
BEGIN ATOMIC
    SELECT pg_catalog.sha256(
        pg_catalog.convert_to('kp-audit-source-commitment-v1'::text, 'UTF8')
        OPERATOR(pg_catalog.||) p_salt OPERATOR(pg_catalog.||) p_digest);
END;

-- ---------------------------------------------------------------------------
-- Helpers
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_relay.is_code(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE
SET search_path = pg_catalog, pg_temp AS $is_code$
    SELECT p_value IS NOT NULL AND p_value ~ '^[a-z0-9_]{1,64}$'
$is_code$;

CREATE FUNCTION audit_relay.current_policy()
RETURNS audit_relay.delivery_policy LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $current_policy$
DECLARE
    p audit_relay.delivery_policy;
BEGIN
    SELECT * INTO STRICT p FROM audit_relay.delivery_policy AS x WHERE x.singleton;
    RETURN p;
END
$current_policy$;

-- Registers one staging row. Called by the trigger, the backfill below and
-- register_missing.
CREATE FUNCTION audit_relay.insert_delivery(o public.audit_outbox_events, p_kind TEXT)
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $insert_delivery$
BEGIN
    INSERT INTO audit_relay.deliveries (
        event_id, registered_at, registration_kind, source_digest, commitment_salt,
        legacy_attempt_count, legacy_delivered_at, available_at)
    VALUES (
        o.event_id, clock_timestamp(), p_kind, audit_relay.source_digest(o),
        uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid()),
        o.attempt_count, o.delivered_at, clock_timestamp());
END
$insert_delivery$;

-- The claim projection of one staging row (design §5.4). Total: no row
-- content can raise. source_intact is computed over the full row, also for
-- oversize rows; the reason text never leaves this function.
CREATE FUNCTION audit_relay.claim_projection(o public.audit_outbox_events,
                                             d audit_relay.deliveries)
RETURNS JSONB LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $claim_projection$
DECLARE
    v_kind TEXT := jsonb_typeof(o.data);
    v_intact BOOLEAN := audit_relay.source_digest(o) IS NOT DISTINCT FROM d.source_digest;
    v_oversize BOOLEAN;
    v_data JSONB;
    v_reason_kind TEXT;
    v_reason_bytes BIGINT;
    v_occurred TEXT := coalesce(to_char((o.occurred_at AT TIME ZONE 'UTC'),
                                        'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'), '');
BEGIN
    v_oversize := octet_length(o.event_type) > 1024 OR octet_length(o.source) > 1024
        OR octet_length(o.subject) > 1024 OR octet_length(o.actor_identity_provider) > 1024
        OR octet_length(o.actor_principal_id) > 1024 OR octet_length(o.resource_type) > 1024
        OR octet_length(o.result) > 1024 OR coalesce(octet_length(o.trace_id), 0) > 1024;
    IF v_kind = 'object' THEN
        v_data := o.data - 'reason'::text;
        IF octet_length(v_data::text) > 16384 THEN
            v_oversize := TRUE;
        END IF;
        v_reason_kind := jsonb_typeof(o.data -> 'reason');
        IF v_reason_kind = 'string' THEN
            v_reason_bytes := octet_length(o.data ->> 'reason');
        END IF;
    END IF;
    IF v_oversize THEN
        RETURN jsonb_build_object(
            'event_id', o.event_id::text, 'event_type', '', 'source', '', 'subject', '',
            'actor_identity_provider', '', 'actor_principal_id', '', 'resource_type', '',
            'resource_id', '', 'resource_version_id', NULL, 'result', '', 'trace_id', NULL,
            'occurred_at', v_occurred, 'oversize', TRUE, 'data', NULL,
            'data_kind', coalesce(v_kind, 'null'), 'reason_kind', NULL, 'reason_bytes', NULL,
            'source_intact', v_intact,
            'source_commitment', encode(audit_relay.commitment(d.commitment_salt,
                                                               d.source_digest), 'hex'),
            'registration_kind', d.registration_kind);
    END IF;
    RETURN jsonb_build_object(
        'event_id', o.event_id::text, 'event_type', o.event_type, 'source', o.source,
        'subject', o.subject, 'actor_identity_provider', o.actor_identity_provider,
        'actor_principal_id', o.actor_principal_id, 'resource_type', o.resource_type,
        'resource_id', o.resource_id::text, 'resource_version_id', o.resource_version_id::text,
        'result', o.result, 'trace_id', o.trace_id, 'occurred_at', v_occurred,
        'oversize', FALSE, 'data', v_data, 'data_kind', coalesce(v_kind, 'null'),
        'reason_kind', v_reason_kind, 'reason_bytes', v_reason_bytes,
        'source_intact', v_intact,
        'source_commitment', encode(audit_relay.commitment(d.commitment_salt,
                                                           d.source_digest), 'hex'),
        'registration_kind', d.registration_kind);
END
$claim_projection$;

-- History snapshot of the terminal evidence before a replay/repair.
CREATE FUNCTION audit_relay.write_history(
    d audit_relay.deliveries, p_transition TEXT, p_control_seq BIGINT, p_control_epoch BIGINT,
    p_reconcile_seq BIGINT)
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $write_history$
BEGIN
    INSERT INTO audit_relay.delivery_history (
        event_id, transition, recorded_at, control_seq, control_epoch, reconcile_seq,
        attempt_count, attempt_limit, last_attempt_at, last_error_code, outage_streak,
        quarantine_code, quarantined_at, delivered_at, store_seq, store_envelope_digest,
        store_outcome, store_recovery_epoch)
    VALUES (
        d.event_id, p_transition, clock_timestamp(), p_control_seq, p_control_epoch,
        p_reconcile_seq, d.attempt_count, d.attempt_limit, d.last_attempt_at,
        d.last_error_code, d.outage_streak, d.quarantine_code, d.quarantined_at,
        d.delivered_at, d.store_seq, d.store_envelope_digest, d.store_outcome,
        d.store_recovery_epoch);
END
$write_history$;

-- ---------------------------------------------------------------------------
-- Guards (design §5.3). The transition GUC (set transaction-locally around
-- the transition UPDATE by replay/repair functions) is accident prevention;
-- the boundary is that nobody but the owner holds table privileges.
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_relay.register_staged()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $register_staged$
BEGIN
    IF TG_RELID IS DISTINCT FROM 'public.audit_outbox_events'::regclass
       OR TG_OP IS DISTINCT FROM 'INSERT' OR TG_LEVEL IS DISTINCT FROM 'ROW'
       OR TG_WHEN IS DISTINCT FROM 'AFTER' THEN
        RAISE EXCEPTION 'audit_relay.register_staged: unexpected trigger context'
            USING ERRCODE = '55000';
    END IF;
    PERFORM audit_relay.insert_delivery(NEW, 'trigger');
    RETURN NULL;
END
$register_staged$;

CREATE FUNCTION audit_relay.guard_staging()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_staging$
BEGIN
    RAISE EXCEPTION 'public.audit_outbox_events is append-only: % refused', TG_OP
        USING ERRCODE = '55000';
END
$guard_staging$;

CREATE FUNCTION audit_relay.transition_is(p_names TEXT[])
RETURNS BOOLEAN LANGUAGE sql STABLE
SET search_path = pg_catalog, pg_temp AS $transition_is$
    SELECT coalesce(current_setting('audit_relay.transition', TRUE), '') = ANY (p_names)
$transition_is$;

CREATE FUNCTION audit_relay.guard_deliveries()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_deliveries$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'audit_relay.deliveries: DELETE refused' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.delivered_at IS NOT NULL OR NEW.quarantined_at IS NOT NULL
           OR NEW.lease_token IS NOT NULL OR NEW.attempt_count <> 0
           OR NEW.attempt_limit IS NOT NULL OR NEW.replay_count <> 0
           OR NEW.outage_streak <> 0 OR NEW.source_mismatch_seq IS NOT NULL THEN
            RAISE EXCEPTION 'audit_relay.deliveries: a registration starts pending'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.event_id, NEW.registered_at, NEW.registration_kind, NEW.source_digest,
        NEW.commitment_salt, NEW.legacy_attempt_count, NEW.legacy_delivered_at)
       IS DISTINCT FROM
       (OLD.event_id, OLD.registered_at, OLD.registration_kind, OLD.source_digest,
        OLD.commitment_salt, OLD.legacy_attempt_count, OLD.legacy_delivered_at) THEN
        RAISE EXCEPTION 'audit_relay.deliveries: registration columns are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF OLD.delivered_at IS NOT NULL
       AND (NEW.delivered_at, NEW.store_seq, NEW.store_envelope_digest, NEW.store_outcome,
            NEW.store_recovery_epoch)
           IS DISTINCT FROM
           (OLD.delivered_at, OLD.store_seq, OLD.store_envelope_digest, OLD.store_outcome,
            OLD.store_recovery_epoch)
       AND NOT audit_relay.transition_is(ARRAY['repair_reset_missing']) THEN
        RAISE EXCEPTION 'audit_relay.deliveries: a receipt is set once' USING ERRCODE = '55000';
    END IF;
    IF OLD.quarantined_at IS NOT NULL
       AND (NEW.quarantined_at, NEW.quarantine_code)
           IS DISTINCT FROM (OLD.quarantined_at, OLD.quarantine_code)
       AND NOT audit_relay.transition_is(ARRAY['replay', 'repair_ack_stored']) THEN
        RAISE EXCEPTION 'audit_relay.deliveries: a quarantine is set once'
            USING ERRCODE = '55000';
    END IF;
    IF OLD.source_mismatch_seq IS NOT NULL
       AND (NEW.source_mismatch_code, NEW.source_mismatch_seq)
           IS DISTINCT FROM (OLD.source_mismatch_code, OLD.source_mismatch_seq)
       AND NOT audit_relay.transition_is(ARRAY['replay']) THEN
        RAISE EXCEPTION 'audit_relay.deliveries: a mismatch record is set once'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.replay_count < OLD.replay_count
       OR (NEW.replay_count > OLD.replay_count AND NOT audit_relay.transition_is(ARRAY['replay'])) THEN
        RAISE EXCEPTION 'audit_relay.deliveries: replay_count changes only by replay'
            USING ERRCODE = '55000';
    END IF;
    IF OLD.attempt_limit IS NOT NULL AND NEW.attempt_limit IS DISTINCT FROM OLD.attempt_limit
       AND NOT audit_relay.transition_is(ARRAY['replay', 'repair_reset_missing']) THEN
        RAISE EXCEPTION 'audit_relay.deliveries: the attempt limit is fixed at first claim'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END
$guard_deliveries$;

CREATE FUNCTION audit_relay.refuse_mutation()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $refuse_mutation$
BEGIN
    RAISE EXCEPTION '%.%: % refused', TG_TABLE_SCHEMA, TG_TABLE_NAME, TG_OP
        USING ERRCODE = '55000';
END
$refuse_mutation$;

CREATE FUNCTION audit_relay.guard_policy()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_policy$
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.revision > OLD.revision THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_relay.delivery_policy: % refused (a change bumps the revision)',
        TG_OP USING ERRCODE = '55000';
END
$guard_policy$;

CREATE FUNCTION audit_relay.guard_progress()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_progress$
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.success_generation >= OLD.success_generation THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_relay.delivery_progress: % refused', TG_OP USING ERRCODE = '55000';
END
$guard_progress$;

CREATE TRIGGER deliveries_guard BEFORE INSERT OR UPDATE OR DELETE ON audit_relay.deliveries
    FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_deliveries();
CREATE TRIGGER deliveries_no_truncate BEFORE TRUNCATE ON audit_relay.deliveries
    FOR EACH STATEMENT EXECUTE FUNCTION audit_relay.refuse_mutation();
CREATE TRIGGER history_guard BEFORE UPDATE OR DELETE ON audit_relay.delivery_history
    FOR EACH ROW EXECUTE FUNCTION audit_relay.refuse_mutation();
CREATE TRIGGER history_no_truncate BEFORE TRUNCATE ON audit_relay.delivery_history
    FOR EACH STATEMENT EXECUTE FUNCTION audit_relay.refuse_mutation();
CREATE TRIGGER policy_guard BEFORE INSERT OR UPDATE OR DELETE ON audit_relay.delivery_policy
    FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_policy();
CREATE TRIGGER policy_no_truncate BEFORE TRUNCATE ON audit_relay.delivery_policy
    FOR EACH STATEMENT EXECUTE FUNCTION audit_relay.refuse_mutation();
CREATE TRIGGER progress_guard BEFORE INSERT OR UPDATE OR DELETE ON audit_relay.delivery_progress
    FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_progress();
CREATE TRIGGER progress_no_truncate BEFORE TRUNCATE ON audit_relay.delivery_progress
    FOR EACH STATEMENT EXECUTE FUNCTION audit_relay.refuse_mutation();

-- ---------------------------------------------------------------------------
-- Worker functions (audit_relay_worker, design §6.2)
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_relay.policy()
RETURNS TABLE (revision BIGINT, max_attempts INTEGER, lease_min_ms BIGINT, lease_max_ms BIGINT,
               backoff_min_ms BIGINT, backoff_max_ms BIGINT, outage_streak_limit INTEGER)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $policy$
    SELECT p.revision, p.max_attempts, p.lease_min_ms, p.lease_max_ms, p.backoff_min_ms,
           p.backoff_max_ms, p.outage_streak_limit
    FROM audit_relay.delivery_policy AS p WHERE p.singleton
$policy$;

CREATE FUNCTION audit_relay.claim(p_owner UUID, p_limit INTEGER, p_lease_ms BIGINT)
RETURNS TABLE (event_id UUID, event_type TEXT, occurred_at TIMESTAMPTZ, attempt_count INTEGER,
               attempt_limit INTEGER, lease_token UUID, lease_owner UUID,
               lease_expires_at TIMESTAMPTZ, projection JSONB)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $claim$
#variable_conflict use_column
DECLARE
    p audit_relay.delivery_policy := audit_relay.current_policy();
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_owner IS NULL OR p_owner = '00000000-0000-0000-0000-000000000000'::uuid
       OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32
       OR p_lease_ms IS NULL OR p_lease_ms NOT BETWEEN p.lease_min_ms AND p.lease_max_ms THEN
        RAISE EXCEPTION 'audit_relay.claim: arguments outside the policy' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t),
    picked AS (
        SELECT d.event_id FROM audit_relay.deliveries AS d CROSS JOIN tick
        WHERE d.delivered_at IS NULL AND d.quarantined_at IS NULL
          AND d.available_at <= tick.t
          AND (d.lease_expires_at IS NULL OR d.lease_expires_at <= tick.t)
          AND d.attempt_count < coalesce(d.attempt_limit, p.max_attempts)
        ORDER BY d.available_at, d.registered_at, d.event_id
        LIMIT p_limit
        FOR UPDATE OF d SKIP LOCKED
    )
    UPDATE audit_relay.deliveries AS d
    SET attempt_count = d.attempt_count + 1,
        attempt_limit = coalesce(d.attempt_limit, p.max_attempts),
        lease_token = gen_random_uuid(),
        lease_owner = p_owner,
        lease_expires_at = tick.t + p_lease_ms * interval '1 millisecond',
        last_attempt_at = tick.t
    FROM picked, tick, public.audit_outbox_events AS o
    WHERE d.event_id = picked.event_id AND o.event_id = d.event_id
    RETURNING d.event_id,
              CASE WHEN octet_length(o.event_type) <= 1024 THEN o.event_type ELSE '' END,
              o.occurred_at, d.attempt_count, d.attempt_limit, d.lease_token, d.lease_owner,
              d.lease_expires_at, audit_relay.claim_projection(o, d);
END
$claim$;

CREATE FUNCTION audit_relay.renew(p_event_id UUID, p_token UUID, p_lease_ms BIGINT)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $renew$
DECLARE
    p audit_relay.delivery_policy := audit_relay.current_policy();
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_lease_ms IS NULL OR p_lease_ms NOT BETWEEN p.lease_min_ms AND p.lease_max_ms THEN
        RAISE EXCEPTION 'audit_relay.renew: lease outside the policy' USING ERRCODE = '22023';
    END IF;
    UPDATE audit_relay.deliveries AS d
    SET lease_expires_at = v_tick + p_lease_ms * interval '1 millisecond'
    WHERE d.event_id = p_event_id AND d.lease_token = p_token
      AND d.delivered_at IS NULL AND d.quarantined_at IS NULL
      AND d.lease_expires_at > v_tick;
    RETURN FOUND;
END
$renew$;

-- Fenced ack with the Store receipt (design §6.2 step 5).
CREATE FUNCTION audit_relay.settle_success(
    p_event_id UUID, p_token UUID, p_store_seq BIGINT, p_envelope_digest BYTEA,
    p_outcome TEXT, p_store_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $settle_success$
DECLARE
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_store_seq IS NULL OR p_store_seq < 1
       OR p_envelope_digest IS NULL OR octet_length(p_envelope_digest) <> 32
       OR p_outcome IS NULL
       OR p_outcome NOT IN ('stored', 'duplicate', 'duplicate_expired', 'duplicate_reprojected')
       OR p_store_epoch IS NULL OR p_store_epoch < 1 THEN
        RAISE EXCEPTION 'audit_relay.settle_success: invalid receipt' USING ERRCODE = '22023';
    END IF;
    UPDATE audit_relay.deliveries AS d
    SET delivered_at = v_tick, store_seq = p_store_seq, store_envelope_digest = p_envelope_digest,
        store_outcome = p_outcome, store_recovery_epoch = p_store_epoch,
        lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL,
        outage_streak = 0, last_error_code = NULL
    WHERE d.event_id = p_event_id AND d.lease_token = p_token
      AND d.delivered_at IS NULL AND d.quarantined_at IS NULL
      AND d.lease_expires_at > v_tick;
    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;
    UPDATE audit_relay.delivery_progress AS g
    SET success_generation = g.success_generation + 1 WHERE g.singleton;
    RETURN TRUE;
END
$settle_success$;

-- Fenced failure (design §6.3). An outage returns the attempt, counts the
-- streak only for residual errors after another delivery succeeded since the
-- row's previous outage, and uses a Store-side capped backoff. A terminal
-- verdict, or a non-outage failure at the attempt limit, quarantines.
CREATE FUNCTION audit_relay.settle_failure(
    p_event_id UUID, p_token UUID, p_code TEXT, p_terminal BOOLEAN, p_backoff_ms BIGINT,
    p_outage BOOLEAN, p_streak_countable BOOLEAN)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $settle_failure$
DECLARE
    p audit_relay.delivery_policy := audit_relay.current_policy();
    v_tick TIMESTAMPTZ := clock_timestamp();
    d audit_relay.deliveries;
    v_generation BIGINT;
    v_streak INTEGER;
    v_backoff BIGINT;
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF NOT audit_relay.is_code(p_code) OR p_terminal IS NULL OR p_outage IS NULL
       OR p_streak_countable IS NULL
       OR p_backoff_ms IS NULL OR p_backoff_ms NOT BETWEEN p.backoff_min_ms AND p.backoff_max_ms
       OR (p_terminal AND p_outage) THEN
        RAISE EXCEPTION 'audit_relay.settle_failure: invalid arguments' USING ERRCODE = '22023';
    END IF;
    SELECT * INTO d FROM audit_relay.deliveries AS x
    WHERE x.event_id = p_event_id AND x.lease_token = p_token
      AND x.delivered_at IS NULL AND x.quarantined_at IS NULL
      AND x.lease_expires_at > v_tick AND x.attempt_limit IS NOT NULL
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;
    IF p_outage THEN
        SELECT g.success_generation INTO STRICT v_generation
        FROM audit_relay.delivery_progress AS g WHERE g.singleton;
        v_streak := d.outage_streak;
        IF p_streak_countable
           AND (d.last_outage_generation IS NULL OR v_generation > d.last_outage_generation) THEN
            v_streak := v_streak + 1;
        END IF;
        v_backoff := least(p.backoff_max_ms,
                           p.backoff_min_ms * (2::bigint ^ least(v_streak, 16))::bigint);
        UPDATE audit_relay.deliveries AS x
        SET attempt_count = greatest(x.attempt_count - 1, 0),
            outage_streak = v_streak,
            last_outage_generation = v_generation,
            last_outage_at = v_tick,
            last_outage_code = p_code,
            last_error_code = p_code,
            available_at = v_tick + v_backoff * interval '1 millisecond',
            quarantined_at = CASE WHEN v_streak >= p.outage_streak_limit THEN v_tick END,
            quarantine_code = CASE WHEN v_streak >= p.outage_streak_limit
                                   THEN 'outage_suspected_event_specific' END,
            lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL
        WHERE x.event_id = d.event_id;
    ELSIF p_terminal OR d.attempt_count >= d.attempt_limit THEN
        UPDATE audit_relay.deliveries AS x
        SET last_error_code = p_code, quarantined_at = v_tick, quarantine_code = p_code,
            lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL
        WHERE x.event_id = d.event_id;
    ELSE
        UPDATE audit_relay.deliveries AS x
        SET last_error_code = p_code,
            available_at = v_tick + p_backoff_ms * interval '1 millisecond',
            lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL
        WHERE x.event_id = d.event_id;
    END IF;
    RETURN TRUE;
END
$settle_failure$;

-- Unknown outcomes at the limit (crash on the last attempt) quarantine as
-- delivery_unknown_at_limit; reconcile may later ack them with a fence.
CREATE FUNCTION audit_relay.reap_exhausted(p_limit INTEGER)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $reap_exhausted$
DECLARE
    v_count BIGINT;
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
        RAISE EXCEPTION 'audit_relay.reap_exhausted: limit must be 1..32' USING ERRCODE = '22023';
    END IF;
    WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t),
    exhausted AS (
        SELECT d.event_id FROM audit_relay.deliveries AS d CROSS JOIN tick
        WHERE d.delivered_at IS NULL AND d.quarantined_at IS NULL
          AND d.attempt_limit IS NOT NULL AND d.attempt_count >= d.attempt_limit
          AND (d.lease_token IS NULL OR d.lease_expires_at <= tick.t)
        ORDER BY d.last_attempt_at NULLS FIRST, d.event_id
        LIMIT p_limit
        FOR UPDATE OF d SKIP LOCKED
    ), reaped AS (
        UPDATE audit_relay.deliveries AS d
        SET quarantined_at = tick.t, quarantine_code = 'delivery_unknown_at_limit',
            last_error_code = 'delivery_unknown_at_limit',
            lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL
        FROM exhausted, tick WHERE d.event_id = exhausted.event_id
        RETURNING d.event_id
    )
    SELECT count(*) INTO v_count FROM reaped;
    RETURN v_count;
END
$reap_exhausted$;

-- Idempotency of audit.integrity.source_mismatch_detected per (event, code).
CREATE FUNCTION audit_relay.mismatch_seq(p_event_id UUID, p_token UUID, p_code TEXT)
RETURNS BIGINT LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $mismatch_seq$
    SELECT d.source_mismatch_seq FROM audit_relay.deliveries AS d
    WHERE d.event_id = p_event_id AND d.lease_token = p_token
      AND d.source_mismatch_code = p_code
$mismatch_seq$;

CREATE FUNCTION audit_relay.note_mismatch(
    p_event_id UUID, p_token UUID, p_code TEXT, p_seq BIGINT)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $note_mismatch$
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_code IS NULL OR p_code NOT IN ('source_digest_mismatch', 'actor_mismatch')
       OR p_seq IS NULL OR p_seq < 1 THEN
        RAISE EXCEPTION 'audit_relay.note_mismatch: invalid arguments' USING ERRCODE = '22023';
    END IF;
    UPDATE audit_relay.deliveries AS d
    SET source_mismatch_code = p_code, source_mismatch_seq = p_seq
    WHERE d.event_id = p_event_id AND d.lease_token = p_token
      AND d.lease_expires_at > clock_timestamp() AND d.source_mismatch_seq IS NULL;
    RETURN FOUND;
END
$note_mismatch$;

-- The highest acknowledged receipt in one Store recovery epoch (regression
-- gate, design §11).
CREATE FUNCTION audit_relay.acked_head(p_store_epoch BIGINT)
RETURNS TABLE (event_id UUID, store_seq BIGINT, store_envelope_digest BYTEA)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $acked_head$
    SELECT d.event_id, d.store_seq, d.store_envelope_digest
    FROM audit_relay.deliveries AS d
    WHERE d.delivered_at IS NOT NULL AND d.store_recovery_epoch = p_store_epoch
    ORDER BY d.store_seq DESC, d.event_id
    LIMIT 1
$acked_head$;

-- Content-free counts for health (design §12).
CREATE FUNCTION audit_relay.status()
RETURNS JSONB LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $status$
DECLARE
    v_tick TIMESTAMPTZ := clock_timestamp();
    v JSONB;
    v_quarantined JSONB;
    v_registration JSONB;
BEGIN
    SELECT coalesce(jsonb_object_agg(q.code, q.n), '{}'::jsonb) INTO v_quarantined
    FROM (SELECT d.quarantine_code AS code, count(*) AS n FROM audit_relay.deliveries AS d
          WHERE d.quarantined_at IS NOT NULL GROUP BY d.quarantine_code) AS q;
    SELECT jsonb_build_object(
        'trigger', count(*) FILTER (WHERE d.registration_kind = 'trigger'),
        'backfill', count(*) FILTER (WHERE d.registration_kind = 'backfill'),
        'repair', count(*) FILTER (WHERE d.registration_kind = 'repair'))
    INTO v_registration FROM audit_relay.deliveries AS d;
    SELECT jsonb_build_object(
        'staged', (SELECT count(*) FROM public.audit_outbox_events),
        'registered', count(*),
        'unregistered', (SELECT count(*) FROM public.audit_outbox_events AS o
                         WHERE NOT EXISTS (SELECT 1 FROM audit_relay.deliveries AS x
                                           WHERE x.event_id = o.event_id)),
        'registration', v_registration,
        'legacy_marked', count(*) FILTER (WHERE coalesce(d.legacy_attempt_count, 0) > 0
                                          OR d.legacy_delivered_at IS NOT NULL),
        'pending', count(*) FILTER (WHERE d.delivered_at IS NULL AND d.quarantined_at IS NULL
                                    AND (d.lease_expires_at IS NULL
                                         OR d.lease_expires_at <= v_tick)),
        'retry_waiting', count(*) FILTER (WHERE d.delivered_at IS NULL
                                          AND d.quarantined_at IS NULL
                                          AND d.lease_token IS NULL
                                          AND d.available_at > v_tick),
        'leased', count(*) FILTER (WHERE d.lease_expires_at > v_tick),
        'outage_held', count(*) FILTER (WHERE d.delivered_at IS NULL
                                        AND d.quarantined_at IS NULL
                                        AND d.last_outage_code IS NOT NULL
                                        AND d.last_error_code = d.last_outage_code),
        'catalog_skew_held', count(*) FILTER (WHERE d.delivered_at IS NULL
                                              AND d.quarantined_at IS NULL
                                              AND d.last_error_code = 'relay_catalog_skew'),
        'quarantined', v_quarantined,
        'quarantined_total', count(*) FILTER (WHERE d.quarantined_at IS NOT NULL),
        'delivered', count(*) FILTER (WHERE d.delivered_at IS NOT NULL),
        'oldest_pending_age_seconds',
            extract(epoch FROM v_tick - min(d.registered_at) FILTER (
                WHERE d.delivered_at IS NULL AND d.quarantined_at IS NULL))::double precision,
        'max_acked_store_seq', max(d.store_seq),
        'replayed', coalesce(sum(d.replay_count), 0),
        'policy_revision', (SELECT p.revision FROM audit_relay.delivery_policy AS p
                            WHERE p.singleton),
        'installed', jsonb_build_object(
            'registration_trigger', audit_relay.trigger_enabled(
                'public.audit_outbox_events'::regclass, 'audit_relay_register'),
            'staging_guard', audit_relay.trigger_enabled(
                'public.audit_outbox_events'::regclass, 'audit_relay_append_only'),
            'staging_truncate_guard', audit_relay.trigger_enabled(
                'public.audit_outbox_events'::regclass, 'audit_relay_no_truncate'),
            'deliveries_guard', audit_relay.trigger_enabled(
                'audit_relay.deliveries'::regclass, 'deliveries_guard'),
            'digest_function', pg_catalog.to_regprocedure(
                'audit_relay.source_digest(public.audit_outbox_events)') IS NOT NULL))
    INTO v FROM audit_relay.deliveries AS d;
    RETURN v;
END
$status$;

CREATE FUNCTION audit_relay.trigger_enabled(p_table REGCLASS, p_name TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE
SET search_path = pg_catalog, pg_temp AS $trigger_enabled$
    SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger AS t
                   WHERE t.tgrelid = p_table AND t.tgname = p_name
                     AND t.tgenabled IN ('O', 'A'))
$trigger_enabled$;

-- Rows not yet delivered, projected without claiming: health forecasts the
-- quarantine codes after a backfill (design §5.1).
CREATE FUNCTION audit_relay.preview_pending(p_after UUID, p_limit INTEGER)
RETURNS TABLE (event_id UUID, projection JSONB)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $preview_pending$
#variable_conflict use_column
BEGIN
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'audit_relay.preview_pending: limit must be 1..1000'
            USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT d.event_id, audit_relay.claim_projection(o, d)
    FROM audit_relay.deliveries AS d
    JOIN public.audit_outbox_events AS o ON o.event_id = d.event_id
    WHERE d.delivered_at IS NULL AND d.quarantined_at IS NULL
      AND d.event_id > coalesce(p_after, '00000000-0000-0000-0000-000000000000'::uuid)
    ORDER BY d.event_id
    LIMIT p_limit;
END
$preview_pending$;

-- ---------------------------------------------------------------------------
-- Reconciliation reads (worker read-only and operator, design §12).
-- Content-free: never the source digest, the salt or row content.
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_relay.delivery_view(o public.audit_outbox_events,
                                          d audit_relay.deliveries, p_tick TIMESTAMPTZ)
RETURNS TABLE (event_id UUID, state TEXT, registration_kind TEXT, delivered_at TIMESTAMPTZ,
               store_seq BIGINT, store_envelope_digest BYTEA, store_outcome TEXT,
               store_recovery_epoch BIGINT, quarantine_code TEXT, source_commitment BYTEA,
               source_intact BOOLEAN, last_error_code TEXT)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $delivery_view$
    SELECT o.event_id,
           CASE WHEN d.event_id IS NULL THEN 'unregistered'
                WHEN d.delivered_at IS NOT NULL THEN 'delivered'
                WHEN d.quarantined_at IS NOT NULL THEN 'quarantined'
                WHEN d.lease_expires_at > p_tick THEN 'leased'
                ELSE 'pending' END,
           d.registration_kind, d.delivered_at, d.store_seq, d.store_envelope_digest,
           d.store_outcome, d.store_recovery_epoch, d.quarantine_code,
           CASE WHEN d.event_id IS NOT NULL
                THEN audit_relay.commitment(d.commitment_salt, d.source_digest) END,
           CASE WHEN d.event_id IS NOT NULL
                THEN audit_relay.source_digest(o) IS NOT DISTINCT FROM d.source_digest END,
           -- A bounded code ([a-z0-9_]{1,64}, table CHECK): reconcile counts
           -- rows held as relay_catalog_skew (design §12).
           d.last_error_code
$delivery_view$;

CREATE FUNCTION audit_relay.reconcile_page(p_after UUID, p_limit INTEGER)
RETURNS TABLE (event_id UUID, state TEXT, registration_kind TEXT, delivered_at TIMESTAMPTZ,
               store_seq BIGINT, store_envelope_digest BYTEA, store_outcome TEXT,
               store_recovery_epoch BIGINT, quarantine_code TEXT, source_commitment BYTEA,
               source_intact BOOLEAN, last_error_code TEXT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $reconcile_page$
#variable_conflict use_column
DECLARE
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'audit_relay.reconcile_page: limit must be 1..1000'
            USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT v.* FROM public.audit_outbox_events AS o
    LEFT JOIN audit_relay.deliveries AS d ON d.event_id = o.event_id
    CROSS JOIN LATERAL audit_relay.delivery_view(o, d, v_tick) AS v
    WHERE o.event_id > coalesce(p_after, '00000000-0000-0000-0000-000000000000'::uuid)
    ORDER BY o.event_id
    LIMIT p_limit;
END
$reconcile_page$;

CREATE FUNCTION audit_relay.lookup_deliveries(p_event_ids UUID[])
RETURNS TABLE (event_id UUID, state TEXT, registration_kind TEXT, delivered_at TIMESTAMPTZ,
               store_seq BIGINT, store_envelope_digest BYTEA, store_outcome TEXT,
               store_recovery_epoch BIGINT, quarantine_code TEXT, source_commitment BYTEA,
               source_intact BOOLEAN, last_error_code TEXT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lookup_deliveries$
#variable_conflict use_column
DECLARE
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    IF coalesce(cardinality(p_event_ids), 0) > 1000 THEN
        RAISE EXCEPTION 'audit_relay.lookup_deliveries: at most 1000 ids' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT v.* FROM public.audit_outbox_events AS o
    LEFT JOIN audit_relay.deliveries AS d ON d.event_id = o.event_id
    CROSS JOIN LATERAL audit_relay.delivery_view(o, d, v_tick) AS v
    WHERE o.event_id = ANY (p_event_ids)
    ORDER BY o.event_id;
END
$lookup_deliveries$;

CREATE FUNCTION audit_relay.reconcile_history_page(p_after BIGINT, p_limit INTEGER)
RETURNS TABLE (history_id BIGINT, event_id UUID, transition TEXT, control_seq BIGINT,
               control_epoch BIGINT, reconcile_seq BIGINT, quarantine_code TEXT,
               recorded_at TIMESTAMPTZ)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $reconcile_history_page$
#variable_conflict use_column
BEGIN
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'audit_relay.reconcile_history_page: limit must be 1..1000'
            USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT h.history_id, h.event_id, h.transition, h.control_seq, h.control_epoch,
           h.reconcile_seq, h.quarantine_code, h.recorded_at
    FROM audit_relay.delivery_history AS h
    WHERE h.history_id > coalesce(p_after, 0)
    ORDER BY h.history_id
    LIMIT p_limit;
END
$reconcile_history_page$;

-- ---------------------------------------------------------------------------
-- Operator transitions (audit_relay_operator, design §6.4, §12). The CLI
-- records the Store control event first; these functions cannot verify it,
-- which reconcile detects as unaudited_replay.
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_relay.replay(p_event_id UUID, p_control_seq BIGINT, p_control_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $replay$
DECLARE
    d audit_relay.deliveries;
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_event_id IS NULL OR p_control_seq IS NULL OR p_control_seq < 1
       OR p_control_epoch IS NULL OR p_control_epoch < 1 THEN
        RAISE EXCEPTION 'audit_relay.replay: a Store control seq and epoch are required'
            USING ERRCODE = '22023';
    END IF;
    SELECT * INTO d FROM audit_relay.deliveries AS x WHERE x.event_id = p_event_id FOR UPDATE;
    IF NOT FOUND OR d.quarantined_at IS NULL
       OR (d.lease_token IS NOT NULL AND d.lease_expires_at > v_tick) THEN
        RETURN FALSE;
    END IF;
    PERFORM audit_relay.write_history(d, 'replay', p_control_seq, p_control_epoch, NULL);
    PERFORM set_config('audit_relay.transition', 'replay', TRUE);
    UPDATE audit_relay.deliveries AS x
    SET attempt_count = 0, attempt_limit = NULL, outage_streak = 0,
        last_outage_generation = NULL, quarantined_at = NULL, quarantine_code = NULL,
        source_mismatch_code = NULL, source_mismatch_seq = NULL,
        lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL,
        available_at = v_tick, replay_count = x.replay_count + 1
    WHERE x.event_id = d.event_id;
    PERFORM set_config('audit_relay.transition', '', TRUE);
    RETURN TRUE;
END
$replay$;

-- Acks a delivery_unknown_at_limit quarantine whose event the Store holds
-- with the same commitment (quarantined_stored). SQL-side fence: only that
-- code, only when the commitment recomputed here equals the Store's.
CREATE FUNCTION audit_relay.repair_ack_stored(
    p_event_id UUID, p_store_seq BIGINT, p_envelope_digest BYTEA, p_store_commitment BYTEA,
    p_store_outcome TEXT, p_control_seq BIGINT, p_store_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $repair_ack_stored$
DECLARE
    d audit_relay.deliveries;
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_store_seq IS NULL OR p_store_seq < 1
       OR p_envelope_digest IS NULL OR octet_length(p_envelope_digest) <> 32
       OR p_store_commitment IS NULL OR octet_length(p_store_commitment) <> 32
       OR p_store_outcome IS NULL OR p_store_outcome NOT IN ('duplicate', 'duplicate_expired')
       OR p_control_seq IS NULL OR p_control_seq < 1
       OR p_store_epoch IS NULL OR p_store_epoch < 1 THEN
        RAISE EXCEPTION 'audit_relay.repair_ack_stored: invalid arguments'
            USING ERRCODE = '22023';
    END IF;
    SELECT * INTO d FROM audit_relay.deliveries AS x WHERE x.event_id = p_event_id FOR UPDATE;
    IF NOT FOUND OR d.quarantine_code IS DISTINCT FROM 'delivery_unknown_at_limit'
       OR d.delivered_at IS NOT NULL
       OR (d.lease_token IS NOT NULL AND d.lease_expires_at > v_tick)
       OR audit_relay.commitment(d.commitment_salt, d.source_digest)
          IS DISTINCT FROM p_store_commitment THEN
        RETURN FALSE;
    END IF;
    PERFORM audit_relay.write_history(d, 'repair_ack_stored', NULL, p_store_epoch,
                                      p_control_seq);
    PERFORM set_config('audit_relay.transition', 'repair_ack_stored', TRUE);
    UPDATE audit_relay.deliveries AS x
    SET delivered_at = v_tick, store_seq = p_store_seq,
        store_envelope_digest = p_envelope_digest, store_outcome = p_store_outcome,
        store_recovery_epoch = p_store_epoch, quarantined_at = NULL, quarantine_code = NULL,
        lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL, outage_streak = 0
    WHERE x.event_id = d.event_id;
    PERFORM set_config('audit_relay.transition', '', TRUE);
    RETURN TRUE;
END
$repair_ack_stored$;

-- Returns an acked delivery that the Store no longer holds (delivered_missing,
-- e.g. after a Store restore) to pending, preserving the receipt in history.
CREATE FUNCTION audit_relay.repair_reset_missing(
    p_event_id UUID, p_expected_store_seq BIGINT, p_control_seq BIGINT, p_control_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $repair_reset_missing$
DECLARE
    d audit_relay.deliveries;
    v_tick TIMESTAMPTZ := clock_timestamp();
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_expected_store_seq IS NULL OR p_control_seq IS NULL OR p_control_seq < 1
       OR p_control_epoch IS NULL OR p_control_epoch < 1 THEN
        RAISE EXCEPTION 'audit_relay.repair_reset_missing: invalid arguments'
            USING ERRCODE = '22023';
    END IF;
    SELECT * INTO d FROM audit_relay.deliveries AS x WHERE x.event_id = p_event_id FOR UPDATE;
    IF NOT FOUND OR d.delivered_at IS NULL OR d.store_seq IS DISTINCT FROM p_expected_store_seq THEN
        RETURN FALSE;
    END IF;
    PERFORM audit_relay.write_history(d, 'repair_reset_missing', NULL, p_control_epoch,
                                      p_control_seq);
    PERFORM set_config('audit_relay.transition', 'repair_reset_missing', TRUE);
    UPDATE audit_relay.deliveries AS x
    SET delivered_at = NULL, store_seq = NULL, store_envelope_digest = NULL,
        store_outcome = NULL, store_recovery_epoch = NULL, attempt_count = 0,
        attempt_limit = NULL, outage_streak = 0, last_outage_generation = NULL,
        available_at = v_tick
    WHERE x.event_id = d.event_id;
    PERFORM set_config('audit_relay.transition', '', TRUE);
    RETURN TRUE;
END
$repair_reset_missing$;

-- Registers staging rows that lack a delivery (a lost trigger). The source
-- digest is the value at repair time (registration_kind = 'repair').
CREATE FUNCTION audit_relay.register_missing(p_limit INTEGER)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $register_missing$
DECLARE
    v_count BIGINT;
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'audit_relay.register_missing: limit must be 1..1000'
            USING ERRCODE = '22023';
    END IF;
    WITH missing AS (
        SELECT o.event_id, audit_relay.source_digest(o) AS digest, o.attempt_count,
               o.delivered_at
        FROM public.audit_outbox_events AS o
        WHERE NOT EXISTS (SELECT 1 FROM audit_relay.deliveries AS d
                          WHERE d.event_id = o.event_id)
        ORDER BY o.event_id
        LIMIT p_limit
    ), inserted AS (
        INSERT INTO audit_relay.deliveries (
            event_id, registered_at, registration_kind, source_digest, commitment_salt,
            legacy_attempt_count, legacy_delivered_at, available_at)
        SELECT m.event_id, clock_timestamp(), 'repair', m.digest,
               uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid()),
               m.attempt_count, m.delivered_at, clock_timestamp()
        FROM missing AS m
        ON CONFLICT (event_id) DO NOTHING
        RETURNING 1
    )
    SELECT count(*) INTO v_count FROM inserted;
    RETURN v_count;
END
$register_missing$;

-- ---------------------------------------------------------------------------
-- Posture (content-free). Each row is a violation; run/replay/repair refuse
-- while any is reported and health raises an alarm.
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_relay.posture_check()
RETURNS TABLE (violation TEXT, object TEXT)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $posture_check$
WITH matrix(signature, rolname) AS (
    SELECT s, 'audit_relay_worker' FROM unnest(ARRAY[
        'audit_relay.claim(uuid,integer,bigint)',
        'audit_relay.renew(uuid,uuid,bigint)',
        'audit_relay.settle_success(uuid,uuid,bigint,bytea,text,bigint)',
        'audit_relay.settle_failure(uuid,uuid,text,boolean,bigint,boolean,boolean)',
        'audit_relay.reap_exhausted(integer)',
        'audit_relay.mismatch_seq(uuid,uuid,text)',
        'audit_relay.note_mismatch(uuid,uuid,text,bigint)',
        'audit_relay.acked_head(bigint)',
        'audit_relay.status()',
        'audit_relay.policy()',
        'audit_relay.preview_pending(uuid,integer)',
        'audit_relay.reconcile_page(uuid,integer)',
        'audit_relay.lookup_deliveries(uuid[])',
        'audit_relay.reconcile_history_page(bigint,integer)',
        'audit_relay.posture_check()']) AS s
    UNION ALL
    SELECT s, 'audit_relay_operator' FROM unnest(ARRAY[
        'audit_relay.replay(uuid,bigint,bigint)',
        'audit_relay.repair_ack_stored(uuid,bigint,bytea,bytea,text,bigint,bigint)',
        'audit_relay.repair_reset_missing(uuid,bigint,bigint,bigint)',
        'audit_relay.register_missing(integer)',
        'audit_relay.acked_head(bigint)',
        'audit_relay.status()',
        'audit_relay.policy()',
        'audit_relay.preview_pending(uuid,integer)',
        'audit_relay.reconcile_page(uuid,integer)',
        'audit_relay.lookup_deliveries(uuid[])',
        'audit_relay.reconcile_history_page(bigint,integer)',
        'audit_relay.posture_check()']) AS s
), capability(rolname) AS (
    VALUES ('audit_relay_worker'), ('audit_relay_operator')
), owner_role AS (
    SELECT r.oid, r.rolname FROM pg_roles AS r WHERE r.rolname = 'audit_relay_owner'
), schema_oid AS (
    SELECT n.oid, n.nspowner, n.nspacl FROM pg_namespace AS n WHERE n.nspname = 'audit_relay'
), functions AS (
    SELECT p.oid, p.oid::regprocedure::text AS signature, p.proowner, p.proacl,
           p.prosecdef, p.proconfig
    FROM pg_proc AS p WHERE p.pronamespace = (SELECT s.oid FROM schema_oid AS s)
), granted AS (
    SELECT f.signature, g.grantee
    FROM functions AS f, aclexplode(f.proacl) AS g
    WHERE g.privilege_type = 'EXECUTE' AND g.grantee <> f.proowner
), capability_members AS (
    WITH RECURSIVE reach(member) AS (
        SELECT m.member FROM pg_auth_members AS m
        JOIN pg_roles AS c ON c.oid = m.roleid
        WHERE c.rolname IN (SELECT rolname FROM capability)
        UNION
        SELECT m.member FROM pg_auth_members AS m JOIN reach ON m.roleid = reach.member
    )
    SELECT DISTINCT l.oid, l.rolname::text AS rolname FROM reach
    JOIN pg_roles AS l ON l.oid = reach.member WHERE l.rolcanlogin
)
SELECT 'owner_role_missing', 'audit_relay_owner'
WHERE NOT EXISTS (SELECT 1 FROM owner_role)
UNION ALL
SELECT 'owner_role_privileged', r.rolname::text FROM pg_roles AS r
WHERE r.rolname = 'audit_relay_owner'
  AND (r.rolsuper OR r.rolcanlogin OR r.rolbypassrls OR r.rolcreaterole)
UNION ALL
SELECT 'schema_owner', 'audit_relay' FROM schema_oid AS s
WHERE s.nspowner IS DISTINCT FROM (SELECT o.oid FROM owner_role AS o)
UNION ALL
SELECT 'schema_privilege', coalesce(r.rolname::text, 'PUBLIC')
FROM schema_oid AS s, aclexplode(s.nspacl) AS g
LEFT JOIN pg_roles AS r ON r.oid = g.grantee
WHERE g.grantee <> s.nspowner
  AND NOT (g.privilege_type = 'USAGE'
           AND r.rolname IN ('audit_relay_worker', 'audit_relay_operator'))
UNION ALL
SELECT 'function_owner', f.signature FROM functions AS f
WHERE f.proowner IS DISTINCT FROM (SELECT o.oid FROM owner_role AS o)
UNION ALL
SELECT 'search_path_unpinned', f.signature FROM functions AS f
WHERE NOT coalesce('search_path=pg_catalog, pg_temp' = ANY (f.proconfig), FALSE)
UNION ALL
SELECT 'public_execute', f.signature FROM functions AS f
WHERE f.proacl IS NULL
   OR EXISTS (SELECT 1 FROM aclexplode(f.proacl) AS g
              WHERE g.grantee = 0 AND g.privilege_type = 'EXECUTE')
UNION ALL
SELECT 'acl_unexpected', g.signature || ' ' || r.rolname
FROM granted AS g JOIN pg_roles AS r ON r.oid = g.grantee
WHERE NOT EXISTS (SELECT 1 FROM matrix AS m
                  WHERE m.signature = g.signature AND m.rolname = r.rolname)
UNION ALL
SELECT 'acl_missing', m.signature || ' ' || m.rolname FROM matrix AS m
WHERE NOT EXISTS (SELECT 1 FROM granted AS g JOIN pg_roles AS r ON r.oid = g.grantee
                  WHERE g.signature = m.signature AND r.rolname = m.rolname)
UNION ALL
SELECT 'table_privilege', c.oid::regclass::text || ' ' || coalesce(r.rolname::text, 'PUBLIC')
FROM pg_class AS c, aclexplode(c.relacl) AS g
LEFT JOIN pg_roles AS r ON r.oid = g.grantee
WHERE c.relnamespace = (SELECT s.oid FROM schema_oid AS s) AND g.grantee <> c.relowner
UNION ALL
SELECT 'table_owner', c.oid::regclass::text FROM pg_class AS c
WHERE c.relnamespace = (SELECT s.oid FROM schema_oid AS s)
  AND c.relowner IS DISTINCT FROM (SELECT o.oid FROM owner_role AS o)
UNION ALL
SELECT 'capability_role_missing', c.rolname FROM capability AS c
WHERE NOT EXISTS (SELECT 1 FROM pg_roles AS r WHERE r.rolname = c.rolname)
UNION ALL
SELECT 'capability_role_privileged', r.rolname::text FROM pg_roles AS r
WHERE r.rolname IN (SELECT c.rolname FROM capability AS c)
  AND (r.rolcanlogin OR r.rolsuper OR r.rolbypassrls OR r.rolcreaterole)
UNION ALL
SELECT 'database_public_create', current_database()::text
FROM pg_database AS d, aclexplode(coalesce(d.datacl, acldefault('d', d.datdba))) AS g
WHERE d.datname = current_database() AND g.grantee = 0 AND g.privilege_type = 'CREATE'
UNION ALL
SELECT 'trigger_missing', t.name FROM (VALUES
    ('public.audit_outbox_events'::regclass, 'audit_relay_register'),
    ('public.audit_outbox_events'::regclass, 'audit_relay_append_only'),
    ('public.audit_outbox_events'::regclass, 'audit_relay_no_truncate'),
    ('audit_relay.deliveries'::regclass, 'deliveries_guard'),
    ('audit_relay.deliveries'::regclass, 'deliveries_no_truncate'),
    ('audit_relay.delivery_history'::regclass, 'history_guard'),
    ('audit_relay.delivery_history'::regclass, 'history_no_truncate')) AS t(rel, name)
WHERE NOT audit_relay.trigger_enabled(t.rel, t.name)
UNION ALL
SELECT 'login_timeouts_missing', m.rolname FROM capability_members AS m
WHERE EXISTS (
    SELECT 1 FROM unnest(ARRAY['statement_timeout', 'lock_timeout',
                               'idle_in_transaction_session_timeout']) AS setting(name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_db_role_setting AS s, unnest(s.setconfig) AS cfg
        WHERE s.setrole = m.oid
          AND s.setdatabase IN (0, (SELECT d.oid FROM pg_database AS d
                                    WHERE d.datname = current_database()))
          AND cfg LIKE setting.name || '=%'))
UNION ALL
SELECT 'login_privileged', m.rolname FROM capability_members AS m
JOIN pg_roles AS r ON r.oid = m.oid
WHERE r.rolsuper OR pg_has_role(m.oid, 'audit_relay_owner', 'MEMBER')
$posture_check$;

-- ---------------------------------------------------------------------------
-- Backfill, then the registration trigger, under the table lock (§5.1).
-- Existing attempt_count/delivered_at values are recorded, never trusted.
-- ---------------------------------------------------------------------------

INSERT INTO audit_relay.deliveries (
    event_id, registered_at, registration_kind, source_digest, commitment_salt,
    legacy_attempt_count, legacy_delivered_at, available_at)
SELECT o.event_id, clock_timestamp(), 'backfill', audit_relay.source_digest(o),
       uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid()),
       o.attempt_count, o.delivered_at, clock_timestamp()
FROM public.audit_outbox_events AS o
ORDER BY o.occurred_at, o.event_id;

RESET ROLE;

CREATE TRIGGER audit_relay_register AFTER INSERT ON public.audit_outbox_events
    FOR EACH ROW EXECUTE FUNCTION audit_relay.register_staged();
CREATE TRIGGER audit_relay_append_only BEFORE UPDATE OR DELETE ON public.audit_outbox_events
    FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_staging();
CREATE TRIGGER audit_relay_no_truncate BEFORE TRUNCATE ON public.audit_outbox_events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_relay.guard_staging();

-- ---------------------------------------------------------------------------
-- PUBLIC holds nothing (design §7.3 item 4)
-- ---------------------------------------------------------------------------

REVOKE ALL ON ALL FUNCTIONS IN SCHEMA audit_relay FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA audit_relay FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA audit_relay FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE audit_relay_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
