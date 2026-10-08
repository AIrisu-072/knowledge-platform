-- Audit Store v1 (design 2026-10-07 revision 3, §7–§11; decision record D2/D3).
--
-- Separate database, schema audit_store, migration ledger
-- audit_store_sqlx_migrations. Run by a superuser migrator: every object is
-- created here and then owned by the NOLOGIN, NOSUPERUSER role
-- audit_store_owner. Nobody receives table privileges; every change goes
-- through SECURITY DEFINER functions that pin search_path to
-- "pg_catalog, pg_temp" and schema-qualify audit_store objects. EXECUTE is
-- granted per capability role by sql/roles.sql (design §10.1).
--
-- Guard triggers are accident prevention. The boundary is privileges: only
-- the owner (and superusers) can touch the tables at all.
--
-- Every writing function sets synchronous_commit transaction-locally in its
-- body (audit_store.lock_head or an explicit set_config): a function-level
-- SET clause would be reverted before COMMIT. No function carries
-- synchronous_commit in its proconfig (posture_check verifies it).

-- ---------------------------------------------------------------------------
-- Owner role and schema
-- ---------------------------------------------------------------------------

DO $owner$
DECLARE
    v_role record;
BEGIN
    SELECT r.rolsuper, r.rolcanlogin INTO v_role
    FROM pg_catalog.pg_roles AS r WHERE r.rolname = 'audit_store_owner';
    IF NOT FOUND THEN
        BEGIN
            CREATE ROLE audit_store_owner
                NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
        EXCEPTION WHEN duplicate_object THEN
            NULL;
        END;
    ELSIF v_role.rolsuper OR v_role.rolcanlogin THEN
        RAISE EXCEPTION 'audit_store_owner must be NOLOGIN and NOSUPERUSER';
    END IF;
END
$owner$;

CREATE SCHEMA audit_store AUTHORIZATION audit_store_owner;
REVOKE ALL ON SCHEMA audit_store FROM PUBLIC;

-- ---------------------------------------------------------------------------
-- Tables (design §7.1)
-- ---------------------------------------------------------------------------

-- The single publication lock. last_chain starts at
-- GENESIS = sha256('kp-audit-chain-genesis-v1').
--
-- recovery_pending is set by report_regression (reason 'regression', with the
-- reported identity) or declare_recovery_pending (reason 'declared', with an
-- incident code) and cleared only by begin_recovery_epoch. The evidence
-- (reporter, time, head at the report) is kept until the epoch records it.
--
-- access_reapply_pending is set by begin_recovery_epoch. While it is set,
-- open_access refuses investigate/export. It clears once an administrator
-- recorded the re-application of access (record_access_reapplied) and the
-- current retention was re-run by a maintainer (expire / confirm_retention_
-- reapplied) after the epoch started (design §7.1, §11).
CREATE TABLE audit_store.publication_head (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    last_seq BIGINT NOT NULL,
    last_chain BYTEA NOT NULL,
    recovery_epoch BIGINT NOT NULL,
    fp_system_identifier TEXT NOT NULL,
    fp_database_oid OID NOT NULL,
    fp_timeline TEXT NOT NULL,
    recovery_pending BOOLEAN NOT NULL DEFAULT FALSE,
    pending_reason TEXT NULL,
    pending_seq BIGINT NULL,
    pending_event_id UUID NULL,
    pending_digest BYTEA NULL,
    pending_by TEXT NULL,
    pending_at TIMESTAMPTZ NULL,
    pending_head_seq BIGINT NULL,
    pending_incident_code TEXT NULL,
    access_reapply_pending BOOLEAN NOT NULL DEFAULT FALSE,
    reapply_from_seq BIGINT NULL,
    access_reapplied_seq BIGINT NULL,
    retention_reapplied_seq BIGINT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_head_singleton CHECK (singleton),
    CONSTRAINT ck_head_seq CHECK (last_seq >= 0),
    CONSTRAINT ck_head_chain CHECK (octet_length(last_chain) = 32),
    CONSTRAINT ck_head_epoch CHECK (recovery_epoch >= 1),
    CONSTRAINT ck_head_genesis CHECK (
        last_seq > 0
        OR last_chain = decode('9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344', 'hex')
    ),
    CONSTRAINT ck_head_pending CHECK (
        recovery_pending = (pending_reason IS NOT NULL)
        AND (pending_reason IS NULL OR pending_reason IN ('regression', 'declared'))
        AND (pending_reason IS DISTINCT FROM 'regression'
             OR (pending_seq IS NOT NULL AND pending_event_id IS NOT NULL
                 AND octet_length(pending_digest) = 32))
        AND (pending_reason IS DISTINCT FROM 'declared'
             OR coalesce(pending_incident_code ~ '^[a-z0-9_]{1,64}$', FALSE))
    ),
    CONSTRAINT ck_head_reapply CHECK (NOT access_reapply_pending OR reapply_from_seq IS NOT NULL)
);

-- Durable identity of every stored event. Never deleted. origin is set by the
-- definer functions, never taken from the envelope.
CREATE TABLE audit_store.events (
    seq BIGINT PRIMARY KEY,
    event_id UUID NOT NULL,
    origin TEXT NOT NULL,
    source TEXT NOT NULL,
    event_type TEXT NOT NULL,
    event_class TEXT NOT NULL,
    subject TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    stored_at TIMESTAMPTZ NOT NULL,
    actor_issuer TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    resource_version_id UUID NULL,
    result TEXT NOT NULL,
    envelope_digest BYTEA NOT NULL,
    digest_algorithm TEXT NOT NULL,
    source_commitment BYTEA NULL,
    adapter_version INTEGER NOT NULL,
    prev_chain BYTEA NOT NULL,
    chain BYTEA NOT NULL,
    recovery_epoch BIGINT NOT NULL,
    expired_at TIMESTAMPTZ NULL,
    expired_by_seq BIGINT NULL,
    -- Informational (the login that called the definer function); not chained.
    ingested_by_db_role TEXT NOT NULL,
    CONSTRAINT uq_events_event_id UNIQUE (event_id),
    CONSTRAINT ck_events_seq CHECK (seq >= 1),
    CONSTRAINT ck_events_origin CHECK (origin IN ('relay', 'store', 'relay_control')),
    CONSTRAINT ck_events_origin_source CHECK (
        (origin = 'store' AND source = 'urn:knowledge-platform:audit-store'
            AND event_type LIKE 'audit.%')
        OR (origin = 'relay_control' AND source = 'urn:knowledge-platform:audit-relay'
            AND event_type LIKE 'audit.%')
        OR (origin = 'relay'
            AND source NOT IN ('urn:knowledge-platform:audit-store',
                               'urn:knowledge-platform:audit-relay')
            AND event_type NOT LIKE 'audit.%'
            AND resource_type <> 'AuditStore')
    ),
    CONSTRAINT ck_events_digest CHECK (octet_length(envelope_digest) = 32),
    CONSTRAINT ck_events_digest_algorithm CHECK (digest_algorithm = 'kp-audit-jsonb-sha256-v1'),
    -- Control events and commitment-less adapters have no source commitment.
    CONSTRAINT ck_events_commitment CHECK (
        source_commitment IS NULL OR octet_length(source_commitment) = 32
    ),
    CONSTRAINT ck_events_document_commitment CHECK (
        source <> 'urn:knowledge-platform:document-platform' OR source_commitment IS NOT NULL
    ),
    CONSTRAINT ck_events_adapter_version CHECK (adapter_version >= 1),
    CONSTRAINT ck_events_prev_chain CHECK (octet_length(prev_chain) = 32),
    CONSTRAINT ck_events_chain CHECK (octet_length(chain) = 32),
    CONSTRAINT ck_events_epoch CHECK (recovery_epoch >= 1),
    CONSTRAINT ck_events_genesis CHECK (
        seq <> 1
        OR prev_chain = decode('9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344', 'hex')
    ),
    CONSTRAINT ck_events_expiry_pair CHECK ((expired_at IS NULL) = (expired_by_seq IS NULL)),
    CONSTRAINT ck_events_expiry_order CHECK (expired_by_seq IS NULL OR expired_by_seq > seq),
    -- Control events are the evidence for verify and recovery: never expired.
    CONSTRAINT ck_events_expirable CHECK (expired_at IS NULL OR origin = 'relay'),
    CONSTRAINT ck_events_db_role CHECK (octet_length(ingested_by_db_role) BETWEEN 1 AND 63)
);

CREATE INDEX events_expired_by ON audit_store.events (expired_by_seq)
    WHERE expired_by_seq IS NOT NULL;
CREATE INDEX events_source_seq ON audit_store.events (source, seq);
CREATE INDEX events_type_seq ON audit_store.events (event_type, seq);

-- Bodies are deleted only by retention/purge, after the identity row is marked.
CREATE TABLE audit_store.event_bodies (
    seq BIGINT PRIMARY KEY,
    envelope JSONB NOT NULL,
    CONSTRAINT fk_event_bodies_event FOREIGN KEY (seq)
        REFERENCES audit_store.events (seq) ON DELETE RESTRICT,
    CONSTRAINT ck_event_bodies_object CHECK (jsonb_typeof(envelope) = 'object'),
    CONSTRAINT ck_event_bodies_size CHECK (octet_length(envelope::text) <= 32768)
);

-- DB login role -> principal (design §10.2). Append-only history: a binding is
-- never overwritten or deleted; unbind_principal records unbound_seq.
CREATE TABLE audit_store.principal_bindings (
    bound_seq BIGINT PRIMARY KEY,
    db_role TEXT NOT NULL,
    issuer TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    unbound_seq BIGINT NULL,
    CONSTRAINT fk_bindings_bound FOREIGN KEY (bound_seq) REFERENCES audit_store.events (seq),
    CONSTRAINT fk_bindings_unbound FOREIGN KEY (unbound_seq) REFERENCES audit_store.events (seq),
    CONSTRAINT ck_bindings_role CHECK (octet_length(db_role) BETWEEN 1 AND 63),
    CONSTRAINT ck_bindings_issuer CHECK (
        octet_length(issuer) BETWEEN 1 AND 256 AND issuer !~ '[\u0001-\u001f\u007f-\u009f]'
    ),
    CONSTRAINT ck_bindings_principal CHECK (
        octet_length(principal_id) BETWEEN 1 AND 256
        AND principal_id !~ '[\u0001-\u001f\u007f-\u009f]'
    ),
    CONSTRAINT ck_bindings_order CHECK (unbound_seq IS NULL OR unbound_seq > bound_seq)
);
CREATE UNIQUE INDEX principal_bindings_active ON audit_store.principal_bindings (db_role)
    WHERE unbound_seq IS NULL;

-- Audit capabilities of a principal. Append-only history: revocation records
-- revoked_seq; a later grant inserts a new row.
CREATE TABLE audit_store.access_grants (
    granted_seq BIGINT PRIMARY KEY,
    issuer TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    capability TEXT NOT NULL,
    revoked_seq BIGINT NULL,
    CONSTRAINT fk_grants_granted FOREIGN KEY (granted_seq) REFERENCES audit_store.events (seq),
    CONSTRAINT fk_grants_revoked FOREIGN KEY (revoked_seq) REFERENCES audit_store.events (seq),
    CONSTRAINT ck_grants_capability CHECK (
        capability IN ('investigate', 'export', 'verify', 'administer', 'maintain')
    ),
    CONSTRAINT ck_grants_order CHECK (revoked_seq IS NULL OR revoked_seq > granted_seq)
);
CREATE UNIQUE INDEX access_grants_active
    ON audit_store.access_grants (issuer, principal_id, capability)
    WHERE revoked_seq IS NULL;

-- Registered source-service principals (design §7.2 step 1): only a login
-- bound to one of these may ingest events of `source` (v1:
-- service/audit-relay for the Document source). Append-only; seeded here and
-- extended by owner members with register_source_service (recorded).
CREATE TABLE audit_store.source_services (
    issuer TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    source TEXT NOT NULL,
    registered_seq BIGINT NULL,
    CONSTRAINT source_services_pkey PRIMARY KEY (issuer, principal_id, source),
    CONSTRAINT fk_source_services_event FOREIGN KEY (registered_seq)
        REFERENCES audit_store.events (seq),
    CONSTRAINT ck_source_services_identity CHECK (
        octet_length(issuer) BETWEEN 1 AND 256 AND octet_length(principal_id) BETWEEN 1 AND 256
        AND octet_length(source) BETWEEN 1 AND 256
        AND source NOT IN ('urn:knowledge-platform:audit-store',
                           'urn:knowledge-platform:audit-relay')
    )
);

INSERT INTO audit_store.source_services (issuer, principal_id, source) VALUES
    ('service', 'audit-relay', 'urn:knowledge-platform:document-platform');

-- Committed disclosure intents (design §10.3). Append-only. The chained
-- audit.access.intent_opened body is authoritative; this row is a copy that
-- read_page compares with it.
CREATE TABLE audit_store.access_intents (
    intent_seq BIGINT PRIMARY KEY,
    token_digest BYTEA NOT NULL,
    session_role TEXT NOT NULL,
    issuer TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    filter JSONB NOT NULL,
    filter_digest BYTEA NOT NULL,
    watermark BIGINT NOT NULL,
    page_size INTEGER NOT NULL,
    max_pages INTEGER NOT NULL,
    include_control BOOLEAN NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    creating_xid XID8 NOT NULL DEFAULT pg_current_xact_id(),
    CONSTRAINT fk_intents_event FOREIGN KEY (intent_seq) REFERENCES audit_store.events (seq),
    CONSTRAINT uq_intents_token UNIQUE (token_digest),
    CONSTRAINT ck_intents_token CHECK (octet_length(token_digest) = 32),
    CONSTRAINT ck_intents_operation CHECK (
        operation IN ('investigate', 'export', 'verify', 'identity_chain')
    ),
    CONSTRAINT ck_intents_filter CHECK (jsonb_typeof(filter) = 'object'),
    CONSTRAINT ck_intents_filter_digest CHECK (octet_length(filter_digest) = 32),
    CONSTRAINT ck_intents_watermark CHECK (watermark >= 0 AND watermark < intent_seq),
    CONSTRAINT ck_intents_page_size CHECK (page_size BETWEEN 1 AND 1000),
    CONSTRAINT ck_intents_max_pages CHECK (max_pages BETWEEN 1 AND 100)
);

-- Versioned, immutable retention policies (design §9). No row means "never
-- expire". retain_days NULL means indefinite.
CREATE TABLE audit_store.retention_policies (
    policy_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    selector JSONB NOT NULL,
    selector_digest BYTEA NOT NULL,
    retain_days INTEGER NULL,
    created_seq BIGINT NOT NULL,
    CONSTRAINT retention_policies_pkey PRIMARY KEY (policy_id, revision),
    CONSTRAINT fk_policies_event FOREIGN KEY (created_seq) REFERENCES audit_store.events (seq),
    CONSTRAINT uq_policies_created UNIQUE (created_seq),
    CONSTRAINT ck_policies_id CHECK (policy_id ~ '^[a-z0-9_]{1,64}$'),
    CONSTRAINT ck_policies_revision CHECK (revision >= 1),
    CONSTRAINT ck_policies_selector CHECK (jsonb_typeof(selector) = 'object'),
    CONSTRAINT ck_policies_selector_digest CHECK (octet_length(selector_digest) = 32),
    CONSTRAINT ck_policies_days CHECK (retain_days IS NULL OR retain_days > 0)
);

-- Reserved for v1 (design §9.3): any active hold blocks expiry. v1 provides
-- no function to place a hold; the content rules are a future extension.
CREATE TABLE audit_store.legal_holds (
    hold_id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    released_at TIMESTAMPTZ NULL,
    CONSTRAINT ck_holds_order CHECK (released_at IS NULL OR released_at >= created_at)
);

-- (source, type, adapter_version, source_format) the Store accepts from the
-- relay: Catalog::registered_types() with each adapter's source_format.
-- Seeded from spec/telemetry/audit-event-catalog.json; a test pins the
-- equality. Changed only by later migrations (write_context 'migration').
CREATE TABLE audit_store.registered_types (
    source TEXT NOT NULL,
    event_type TEXT NOT NULL,
    adapter_version INTEGER NOT NULL,
    source_format TEXT NOT NULL,
    CONSTRAINT registered_types_pkey PRIMARY KEY (source, event_type, adapter_version),
    CONSTRAINT ck_registered_adapter CHECK (adapter_version >= 1),
    CONSTRAINT ck_registered_format CHECK (source_format ~ '^[a-z0-9-]{1,64}$'),
    CONSTRAINT ck_registered_not_control CHECK (
        event_type NOT LIKE 'audit.%'
        AND source NOT IN ('urn:knowledge-platform:audit-store',
                           'urn:knowledge-platform:audit-relay')
    )
);

INSERT INTO audit_store.registered_types (source, event_type, adapter_version, source_format)
SELECT 'urn:knowledge-platform:document-platform', t, 1, 'document-audit-outbox-v0'
FROM unnest(ARRAY[
    'document.created',
    'document.version.created',
    'document.version.updated',
    'document.version.rebased',
    'document.version.published',
    'document.version.publication.scheduled',
    'document.version.publication.cancelled',
    'document.version.publication.terminal',
    'document.version.withdrawn',
    'document.publication.ended',
    'document.metadata.changed',
    'document.moved',
    'folder.created',
    'folder.renamed',
    'folder.moved',
    'access_policy.changed',
    'document.version.read_confirmed',
    'document.file.access_granted',
    'document.diff.result_access_granted',
    'document.revision_comparison.result_access_granted',
    'authorization.denied'
]) AS t;

-- Coalescing state for repeated identity denials of the same login
-- (not_source_service / unbound on ingest, probe and report_regression): at
-- most one audit.access.denied per streak and minute. Nothing is sampled:
-- the denials coalesced since the last record are counted here and chained
-- as `suppressed_since_last` by the next record of the streak, by a flush
-- record when the denial code or actor changes, by a flush record when the
-- streak ends with a success (record_denied_coalesced, clear_denial_streak),
-- by a flush record once the streak is older than the coalescing window
-- (flush_denial_streaks, run before every append) and by a flush record of
-- every pending count before verify, checkpoint, a disclosure intent,
-- expire and purge record their evidence. Each record stands for itself
-- plus its suppressed_since_last denials of the same login, code and actor
-- (the actor resolved when the streak's record was written). store_status
-- reports the pending total (denials_pending). Operational state, not
-- evidence; written only by definer functions.
CREATE TABLE audit_store.denial_streaks (
    session_role TEXT PRIMARY KEY,
    denial_code TEXT NOT NULL,
    last_operation TEXT NOT NULL,
    last_recorded_seq BIGINT NOT NULL,
    last_recorded_at TIMESTAMPTZ NOT NULL,
    suppressed BIGINT NOT NULL DEFAULT 0,
    actor_issuer TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    CONSTRAINT ck_streak_role CHECK (octet_length(session_role) BETWEEN 1 AND 63),
    CONSTRAINT ck_streak_code CHECK (denial_code ~ '^[a-z0-9_]{1,64}$'),
    CONSTRAINT ck_streak_operation CHECK (last_operation ~ '^[a-z0-9_]{1,64}$'),
    CONSTRAINT ck_streak_suppressed CHECK (suppressed >= 0)
);

-- ---------------------------------------------------------------------------
-- Pure helpers
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_store.genesis()
RETURNS BYTEA LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $genesis$
    SELECT decode('9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344', 'hex')
$genesis$;

-- chain = sha256('kp-audit-chain-v1' || prev || int8send(seq) || uuid_send(id) || digest)
CREATE FUNCTION audit_store.chain_step(p_prev BYTEA, p_seq BIGINT, p_event_id UUID, p_digest BYTEA)
RETURNS BYTEA LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $chain_step$
    SELECT sha256('kp-audit-chain-v1'::bytea || p_prev || int8send(p_seq)
                  || uuid_send(p_event_id) || p_digest)
$chain_step$;

-- kp-audit-jsonb-sha256-v1: sha256 of the jsonb text rendering.
CREATE FUNCTION audit_store.jsonb_digest(p_value JSONB)
RETURNS BYTEA LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $jsonb_digest$
    SELECT sha256(convert_to(p_value::text, 'UTF8'))
$jsonb_digest$;

-- expired_set_digest = sha256('kp-audit-expired-set-v1' || int8send(seq)...)
-- over the seqs in ascending order (audit_core::expired_set_digest).
CREATE FUNCTION audit_store.expired_set_digest(p_seqs BIGINT[])
RETURNS BYTEA LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $expired_set_digest$
    SELECT sha256('kp-audit-expired-set-v1'::bytea
                  || coalesce((SELECT string_agg(int8send(x), ''::bytea ORDER BY x)
                               FROM unnest(p_seqs) AS x), ''::bytea))
$expired_set_digest$;

CREATE FUNCTION audit_store.utc_text(p_at TIMESTAMPTZ)
RETURNS TEXT LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $utc_text$
    SELECT to_char(p_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
$utc_text$;

-- The retention floor of a policy at p_at: p_at minus p_days whole UTC days
-- (independent of the session TimeZone, so that expire and the re-apply
-- check compute the same instant).
CREATE FUNCTION audit_store.retention_floor(p_at TIMESTAMPTZ, p_days INTEGER)
RETURNS TIMESTAMPTZ LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $retention_floor$
    SELECT ((p_at AT TIME ZONE 'UTC') - make_interval(days => p_days)) AT TIME ZONE 'UTC'
$retention_floor$;

-- {key: value} or {} when value is NULL (optional control details).
CREATE FUNCTION audit_store.opt(p_key TEXT, p_value JSONB)
RETURNS JSONB LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $opt$
    SELECT CASE WHEN p_value IS NULL OR p_value = 'null'::jsonb THEN '{}'::jsonb
                ELSE jsonb_build_object(p_key, p_value) END
$opt$;

-- A JSON integer as bigint, or NULL.
CREATE FUNCTION audit_store.jint(p_value JSONB)
RETURNS BIGINT LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $jint$
    SELECT CASE WHEN jsonb_typeof(p_value) = 'number' AND p_value::text ~ '^-?[0-9]{1,18}$'
                THEN p_value::text::bigint END
$jint$;

-- A control timestamp (YYYY-MM-DDTHH:MM:SS.ffffffZ) as timestamptz, or NULL.
CREATE FUNCTION audit_store.jts(p_value JSONB)
RETURNS TIMESTAMPTZ LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $jts$
    SELECT CASE
        WHEN jsonb_typeof(p_value) = 'string'
             AND (p_value #>> '{}') ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$'
             AND pg_input_is_valid(p_value #>> '{}', 'timestamptz')
        THEN (p_value #>> '{}')::timestamptz
    END
$jts$;

CREATE FUNCTION audit_store.is_uuid_text(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_uuid_text$
    SELECT coalesce(
        p_value ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        AND p_value <> '00000000-0000-0000-0000-000000000000', FALSE)
$is_uuid_text$;

-- Bounded identifier: 1..=256 bytes, no control characters.
CREATE FUNCTION audit_store.is_identifier(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_identifier$
    SELECT coalesce(
        octet_length(p_value) BETWEEN 1 AND 256 AND p_value !~ '[\u0001-\u001f\u007f-\u009f]',
        FALSE)
$is_identifier$;

CREATE FUNCTION audit_store.is_code(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_code$
    SELECT coalesce(p_value ~ '^[a-z0-9_]{1,64}$', FALSE)
$is_code$;

CREATE FUNCTION audit_store.is_hex64(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_hex64$
    SELECT coalesce(p_value ~ '^[0-9a-f]{64}$', FALSE)
$is_hex64$;

-- Closed control-detail grammars (no free text is ever echoed into a control
-- event). They mirror the audit-core kinds:
--   db_role       ^[a-z_][a-z0-9_$]{0,62}$   (session_role, db_role)
--   event_type    [a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+, at most 128 bytes
--   resource_ref  lowercase UUID (nil allowed) or 'audit-store'
--   principal part  audit_core::kinds::is_principal_part
--   source_urn    a catalog adapter source (registered relay source or one of
--                 the two control sources)
CREATE FUNCTION audit_store.is_db_role(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_db_role$
    SELECT coalesce(p_value ~ '^[a-z_][a-z0-9_$]{0,62}$', FALSE)
$is_db_role$;

CREATE FUNCTION audit_store.is_event_type(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_event_type$
    SELECT coalesce(octet_length(p_value) <= 128
                    AND p_value ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$', FALSE)
$is_event_type$;

CREATE FUNCTION audit_store.is_resource_ref(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_resource_ref$
    SELECT coalesce(p_value = 'audit-store'
                    OR p_value ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
                    FALSE)
$is_resource_ref$;

-- A principal part (issuer or principal id): 1..=256 UTF-8 bytes, no leading
-- or trailing Unicode White_Space, and none of: Cc, Bidi_Control, U+2028,
-- U+2029, U+FEFF, the TAG block, noncharacters (audit-core
-- kinds::is_principal_part).
CREATE FUNCTION audit_store.is_principal_part(p_value TEXT)
RETURNS BOOLEAN LANGUAGE plpgsql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_principal_part$
DECLARE
    c INTEGER;
    v_white CONSTANT INTEGER[] := ARRAY[9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194,
        8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288];
BEGIN
    IF p_value IS NULL OR octet_length(p_value) NOT BETWEEN 1 AND 256 THEN
        RETURN FALSE;
    END IF;
    IF ascii(p_value) = ANY (v_white)
       OR ascii(substr(p_value, char_length(p_value))) = ANY (v_white) THEN
        RETURN FALSE;
    END IF;
    FOR i IN 1 .. char_length(p_value) LOOP
        c := ascii(substr(p_value, i, 1));
        IF c <= 31 OR (c BETWEEN 127 AND 159)
           OR c IN (1564, 8206, 8207, 8232, 8233, 65279)
           OR c BETWEEN 8234 AND 8238 OR c BETWEEN 8294 AND 8297
           OR c BETWEEN 917504 AND 917631
           OR c BETWEEN 64976 AND 65007
           OR (c & 65534) = 65534 THEN
            RETURN FALSE;
        END IF;
    END LOOP;
    RETURN TRUE;
END
$is_principal_part$;

CREATE FUNCTION audit_store.is_source_urn(p_value TEXT, p_relay_only BOOLEAN)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_source_urn$
    SELECT coalesce(
        (NOT p_relay_only AND p_value IN ('urn:knowledge-platform:audit-store',
                                          'urn:knowledge-platform:audit-relay'))
        OR EXISTS (SELECT 1 FROM audit_store.registered_types AS r WHERE r.source = p_value),
        FALSE)
$is_source_urn$;

-- The control event types of the catalog (origin store / relay_control,
-- spec/telemetry/audit-event-catalog.json; a test pins the equality).
CREATE FUNCTION audit_store.is_control_type(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_control_type$
    SELECT coalesce(p_value IN (
        'audit.access.intent_opened', 'audit.access.denied', 'audit.access.closed',
        'audit.access_policy.changed', 'audit.retention.policy_changed',
        'audit.retention.expired', 'audit.retention.expire_refused', 'audit.body.purged',
        'audit.integrity.verified', 'audit.integrity.conflict_detected',
        'audit.recovery.epoch_started', 'audit.delivery.replay_requested',
        'audit.reconciliation.completed', 'audit.integrity.source_mismatch_detected'), FALSE)
$is_control_type$;

-- A relay event type the Store accepts (registered for some source and
-- adapter version).
CREATE FUNCTION audit_store.is_registered_type(p_value TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $is_registered_type$
    SELECT coalesce(EXISTS (SELECT 1 FROM audit_store.registered_types AS r
                            WHERE r.event_type = p_value), FALSE)
$is_registered_type$;

-- One export line (design §10.4): the 10 keys, built from text without a
-- serde round trip. `envelope` is the jsonb text verbatim or null.
CREATE FUNCTION audit_store.export_line(e audit_store.events, p_envelope JSONB)
RETURNS TEXT LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $export_line$
    SELECT '{"seq":' || e.seq::text
        || ',"event_id":"' || e.event_id::text
        || '","origin":"' || e.origin
        || '","envelope_digest":"' || encode(e.envelope_digest, 'hex')
        || '","prev_chain":"' || encode(e.prev_chain, 'hex')
        || '","chain":"' || encode(e.chain, 'hex')
        || '","recovery_epoch":' || e.recovery_epoch::text
        || ',"expired":' || CASE WHEN e.expired_at IS NULL THEN 'false' ELSE 'true' END
        || ',"expired_by_seq":' || coalesce(e.expired_by_seq::text, 'null')
        || ',"envelope":' || coalesce(p_envelope::text, 'null')
        || '}'
$export_line$;

-- ---------------------------------------------------------------------------
-- Fingerprint, posture and the recovery gate (design §7.3, §11)
-- ---------------------------------------------------------------------------

-- (system_identifier, database oid, timeline), each the canonical decimal
-- text of an int8 (the int8_text kind of audit.recovery.epoch_started). The
-- timeline is the first 8 hex digits of the current WAL file name, in
-- decimal. On a standby the WAL insert position is unavailable; the timeline
-- is reported as 'standby', which never matches, so publication stays closed
-- there (and no epoch can start: begin_recovery_epoch writes).
CREATE FUNCTION audit_store.current_fingerprint(
    OUT system_identifier TEXT, OUT database_oid OID, OUT timeline TEXT)
LANGUAGE sql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $current_fingerprint$
    SELECT (SELECT c.system_identifier::text FROM pg_control_system() AS c),
           (SELECT d.oid FROM pg_database AS d WHERE d.datname = current_database()),
           CASE WHEN pg_is_in_recovery() THEN 'standby'
                ELSE (('x' || substr(pg_walfile_name(pg_current_wal_lsn()), 1, 8))::bit(32)
                      ::bigint)::text END
$current_fingerprint$;

-- Content-free privilege posture (design §7.3). Each row is a violation.
CREATE FUNCTION audit_store.posture_check()
RETURNS TABLE (violation TEXT, object TEXT)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $posture_check$
WITH
owner_role AS (
    SELECT r.oid, r.rolsuper, r.rolcanlogin FROM pg_roles AS r
    WHERE r.rolname = 'audit_store_owner'
),
capability(rolname) AS (VALUES
    ('audit_store_ingest'), ('audit_store_reconciler'), ('audit_store_relay_control'),
    ('audit_store_reader'), ('audit_store_verifier'), ('audit_store_admin'),
    ('audit_store_maintainer')
),
-- Design §10.1 matrix, plus content-free status/posture for operator roles.
expected(proname, rolname) AS (VALUES
    ('ingest', 'audit_store_ingest'),
    ('probe', 'audit_store_ingest'),
    ('report_regression', 'audit_store_ingest'),
    ('lookup_receipts', 'audit_store_reconciler'),
    ('list_source_receipts', 'audit_store_reconciler'),
    ('lookup_control_receipts', 'audit_store_reconciler'),
    ('lookup_lost_ranges', 'audit_store_reconciler'),
    ('store_status', 'audit_store_reconciler'),
    ('record_relay_control', 'audit_store_relay_control'),
    ('open_access', 'audit_store_reader'),
    ('read_page', 'audit_store_reader'),
    ('close_access', 'audit_store_reader'),
    ('open_access', 'audit_store_verifier'),
    ('read_page', 'audit_store_verifier'),
    ('close_access', 'audit_store_verifier'),
    ('verify', 'audit_store_verifier'),
    ('checkpoint', 'audit_store_verifier'),
    ('verify_recovery', 'audit_store_verifier'),
    ('identity_chain_recovery_page', 'audit_store_verifier'),
    ('change_access', 'audit_store_admin'),
    ('set_retention_policy', 'audit_store_admin'),
    ('record_access_reapplied', 'audit_store_admin'),
    ('expire', 'audit_store_maintainer'),
    ('purge_body', 'audit_store_maintainer'),
    ('begin_recovery_epoch', 'audit_store_maintainer'),
    ('declare_recovery_pending', 'audit_store_maintainer'),
    ('confirm_retention_reapplied', 'audit_store_maintainer'),
    ('verify_recovery', 'audit_store_maintainer'),
    ('identity_chain_recovery_page', 'audit_store_maintainer'),
    ('store_status', 'audit_store_verifier'),
    ('store_status', 'audit_store_admin'),
    ('store_status', 'audit_store_maintainer'),
    ('posture_check', 'audit_store_verifier'),
    ('posture_check', 'audit_store_admin'),
    ('posture_check', 'audit_store_maintainer')
),
fns AS (
    SELECT p.oid, p.proname::text AS proname, p.proowner, p.prosecdef, p.proconfig, p.proacl
    FROM pg_proc AS p
    WHERE p.pronamespace = (SELECT n.oid FROM pg_namespace AS n WHERE n.nspname = 'audit_store')
),
fn_acl AS (
    SELECT f.proname, a.grantee, a.privilege_type
    FROM fns AS f,
         LATERAL aclexplode(coalesce(f.proacl, acldefault('f', f.proowner))) AS a
    WHERE a.grantee <> f.proowner
),
rels AS (
    SELECT c.oid, c.relname::text AS relname, c.relowner, c.relacl, c.relkind
    FROM pg_class AS c
    WHERE c.relnamespace = (SELECT n.oid FROM pg_namespace AS n WHERE n.nspname = 'audit_store')
      AND c.relkind IN ('r', 'S', 'v', 'm', 'p', 'f')
),
-- Every (login, role) membership, direct or inherited.
memberships AS (
    WITH RECURSIVE reach(member, roleid) AS (
        SELECT m.member, m.roleid FROM pg_auth_members AS m
        UNION
        SELECT m.member, reach.roleid FROM pg_auth_members AS m
        JOIN reach ON reach.member = m.roleid
    )
    SELECT DISTINCT l.oid AS login_oid, l.rolname::text AS login, g.rolname::text AS granted
    FROM reach
    JOIN pg_roles AS l ON l.oid = reach.member
    JOIN pg_roles AS g ON g.oid = reach.roleid
    WHERE l.rolcanlogin AND NOT l.rolsuper
),
capability_logins AS (
    SELECT DISTINCT m.login_oid AS oid, m.login AS rolname FROM memberships AS m
    WHERE m.granted IN (SELECT c.rolname FROM capability AS c)
),
this_db AS (
    SELECT d.oid, d.datdba, d.datacl FROM pg_database AS d WHERE d.datname = current_database()
),
role_settings AS (
    SELECT s.setrole, s.setdatabase, cfg
    FROM pg_db_role_setting AS s, unnest(s.setconfig) AS cfg
    WHERE s.setdatabase IN (0, (SELECT d.oid FROM this_db AS d))
)
SELECT 'owner_missing', 'audit_store_owner'
WHERE NOT EXISTS (SELECT 1 FROM owner_role)
UNION ALL
SELECT 'owner_privileged', 'audit_store_owner'
FROM owner_role AS o WHERE o.rolsuper OR o.rolcanlogin
UNION ALL
SELECT 'schema_owner', n.nspname::text FROM pg_namespace AS n
WHERE n.nspname = 'audit_store'
  AND n.nspowner IS DISTINCT FROM (SELECT o.oid FROM owner_role AS o)
UNION ALL
SELECT 'function_owner', f.proname FROM fns AS f
WHERE f.proowner IS DISTINCT FROM (SELECT o.oid FROM owner_role AS o)
UNION ALL
SELECT 'relation_owner', r.relname FROM rels AS r
WHERE r.relowner IS DISTINCT FROM (SELECT o.oid FROM owner_role AS o)
UNION ALL
SELECT 'not_security_definer', f.proname FROM fns AS f WHERE NOT f.prosecdef
UNION ALL
SELECT 'search_path_unpinned', f.proname FROM fns AS f
WHERE NOT (coalesce(f.proconfig, ARRAY[]::text[]) @> ARRAY['search_path=pg_catalog, pg_temp'])
UNION ALL
-- A function-level SET synchronous_commit is reverted before COMMIT.
SELECT 'function_synchronous_commit', f.proname FROM fns AS f
WHERE EXISTS (SELECT 1 FROM unnest(f.proconfig) AS c WHERE c LIKE 'synchronous_commit=%')
UNION ALL
SELECT 'public_execute', a.proname FROM fn_acl AS a WHERE a.grantee = 0
UNION ALL
SELECT 'unexpected_execute', a.proname || ':' || r.rolname FROM fn_acl AS a
JOIN pg_roles AS r ON r.oid = a.grantee
WHERE NOT EXISTS (
    SELECT 1 FROM expected AS x
    WHERE x.proname = a.proname AND x.rolname = r.rolname AND a.privilege_type = 'EXECUTE')
UNION ALL
SELECT 'missing_execute', x.proname || ':' || x.rolname FROM expected AS x
WHERE NOT EXISTS (
    SELECT 1 FROM fn_acl AS a JOIN pg_roles AS r ON r.oid = a.grantee
    WHERE a.proname = x.proname AND r.rolname = x.rolname AND a.privilege_type = 'EXECUTE')
UNION ALL
SELECT 'relation_privilege', r.relname FROM rels AS r,
     LATERAL aclexplode(coalesce(r.relacl, acldefault(
         CASE WHEN r.relkind = 'S' THEN 's' ELSE 'r' END::"char", r.relowner))) AS a
WHERE a.grantee <> r.relowner
UNION ALL
-- Column privileges are not in relacl.
SELECT DISTINCT 'column_privilege', r.relname || '.' || att.attname::text
FROM rels AS r
JOIN pg_attribute AS att ON att.attrelid = r.oid AND att.attnum > 0 AND NOT att.attisdropped
CROSS JOIN LATERAL aclexplode(att.attacl) AS a
WHERE att.attacl IS NOT NULL AND a.grantee <> r.relowner
UNION ALL
-- Predefined roles that bypass the table ACLs (design §7.3: only the owner
-- and superusers may bypass the boundary): pg_read_all_data reads every
-- body and intent, pg_write_all_data can write the tables under the
-- definer GUC (forging grants), pg_maintain can lock them. Reported for
-- every non-superuser login that can connect to this database.
SELECT 'predefined_role_member', l.rolname::text || ':' || p.rolname::text
FROM pg_roles AS l
JOIN pg_roles AS p ON p.rolname IN ('pg_read_all_data', 'pg_write_all_data', 'pg_maintain')
WHERE l.rolcanlogin AND NOT l.rolsuper
  AND pg_has_role(l.oid, p.oid, 'MEMBER')
  AND has_database_privilege(l.oid, (SELECT d.oid FROM this_db AS d), 'CONNECT')
UNION ALL
SELECT 'schema_usage', CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE g.rolname::text END
FROM pg_namespace AS n
CROSS JOIN LATERAL aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) AS a
LEFT JOIN pg_roles AS g ON g.oid = a.grantee
WHERE n.nspname = 'audit_store'
  AND a.grantee <> n.nspowner
  AND (a.grantee = 0 OR a.privilege_type <> 'USAGE'
       OR g.rolname NOT IN (SELECT c.rolname FROM capability AS c))
UNION ALL
SELECT 'default_execute_not_revoked', 'audit_store_owner'
WHERE NOT EXISTS (
    SELECT 1 FROM pg_default_acl AS d
    WHERE d.defaclrole = (SELECT o.oid FROM owner_role AS o)
      AND d.defaclnamespace = 0
      AND d.defaclobjtype = 'f'
      AND NOT EXISTS (SELECT 1 FROM aclexplode(d.defaclacl) AS a WHERE a.grantee = 0))
UNION ALL
-- Default privileges of the owner must not grant anything to anyone.
SELECT 'default_privilege_granted',
       d.defaclobjtype::text || ':' || CASE WHEN a.grantee = 0 THEN 'PUBLIC'
                                            ELSE coalesce(g.rolname::text, '?') END
FROM pg_default_acl AS d
CROSS JOIN LATERAL aclexplode(d.defaclacl) AS a
LEFT JOIN pg_roles AS g ON g.oid = a.grantee
WHERE d.defaclrole = (SELECT o.oid FROM owner_role AS o)
  AND a.grantee <> d.defaclrole
UNION ALL
SELECT 'database_public_' || lower(a.privilege_type), current_database()::text
FROM this_db AS d, LATERAL aclexplode(coalesce(d.datacl, acldefault('d', d.datdba))) AS a
WHERE a.grantee = 0 AND a.privilege_type IN ('CONNECT', 'TEMPORARY')
UNION ALL
SELECT 'capability_role_missing', c.rolname FROM capability AS c
WHERE NOT EXISTS (SELECT 1 FROM pg_roles AS r WHERE r.rolname = c.rolname)
UNION ALL
SELECT 'capability_role_privileged', r.rolname::text FROM pg_roles AS r
WHERE r.rolname IN (SELECT c.rolname FROM capability AS c)
  AND (r.rolcanlogin OR r.rolsuper OR r.rolbypassrls OR r.rolcreaterole)
UNION ALL
-- Each timeout must be set and non-zero (0 disables it); a per-database
-- setting overrides the role-wide one.
SELECT 'login_timeouts_missing', m.rolname FROM capability_logins AS m
WHERE EXISTS (
    SELECT 1 FROM unnest(ARRAY['statement_timeout', 'lock_timeout',
                               'idle_in_transaction_session_timeout']) AS setting(name)
    WHERE NOT EXISTS (
        SELECT 1 FROM (
            SELECT s.cfg FROM role_settings AS s
            WHERE s.setrole = m.oid AND s.cfg LIKE setting.name || '=%'
            ORDER BY s.setdatabase = 0
            LIMIT 1) AS effective
        WHERE effective.cfg ~ ('^' || setting.name || '=0*[1-9]')))
UNION ALL
-- synchronous_commit=on as a database default and on every capability login;
-- any weaker database or role default is a violation (sessions can still
-- override defaults: the definer bodies and the clients enforce it too).
SELECT 'database_synchronous_commit_missing', current_database()::text
WHERE NOT EXISTS (
    SELECT 1 FROM role_settings AS s
    WHERE s.setrole = 0 AND s.setdatabase = (SELECT d.oid FROM this_db AS d)
      AND s.cfg = 'synchronous_commit=on')
UNION ALL
SELECT 'login_synchronous_commit_missing', m.rolname FROM capability_logins AS m
WHERE NOT EXISTS (
    SELECT 1 FROM role_settings AS s
    WHERE s.setrole = m.oid AND s.cfg = 'synchronous_commit=on')
UNION ALL
SELECT 'synchronous_commit_weakened',
       CASE WHEN s.setrole = 0 THEN 'database' ELSE coalesce(r.rolname::text, '?') END
FROM role_settings AS s
LEFT JOIN pg_roles AS r ON r.oid = s.setrole
WHERE s.cfg LIKE 'synchronous_commit=%' AND s.cfg <> 'synchronous_commit=on'
UNION ALL
-- Membership rules (design §7.3): ingest only for logins bound to a
-- registered source-service principal; owner membership only for the
-- bootstrap (DBA) logins, which hold no capability role and no binding.
SELECT 'ingest_member_not_source_service', m.login FROM memberships AS m
WHERE m.granted = 'audit_store_ingest'
  AND NOT EXISTS (
      SELECT 1 FROM audit_store.principal_bindings AS b
      JOIN audit_store.source_services AS s
        ON s.issuer = b.issuer AND s.principal_id = b.principal_id
      WHERE b.db_role = m.login AND b.unbound_seq IS NULL)
UNION ALL
SELECT 'owner_member_has_capability', m.login FROM memberships AS m
WHERE m.granted = 'audit_store_owner'
  AND EXISTS (SELECT 1 FROM capability_logins AS c WHERE c.oid = m.login_oid)
UNION ALL
SELECT 'owner_member_bound', m.login FROM memberships AS m
WHERE m.granted = 'audit_store_owner'
  AND EXISTS (SELECT 1 FROM audit_store.principal_bindings AS b
              WHERE b.db_role = m.login AND b.unbound_seq IS NULL)
UNION ALL
-- session_role/db_role are a closed grammar in control events.
SELECT DISTINCT 'login_name_invalid', m.login FROM memberships AS m
WHERE m.granted IN (SELECT c.rolname FROM capability AS c UNION ALL SELECT 'audit_store_owner')
  AND NOT audit_store.is_db_role(m.login)
$posture_check$;

-- Fingerprint mismatch or recovery_pending: recovery mode (design §11).
CREATE FUNCTION audit_store.in_recovery(h audit_store.publication_head)
RETURNS BOOLEAN LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $in_recovery$
DECLARE
    fp record;
BEGIN
    IF h.recovery_pending THEN
        RETURN TRUE;
    END IF;
    SELECT * INTO fp FROM audit_store.current_fingerprint();
    RETURN (fp.system_identifier, fp.database_oid, fp.timeline)
        IS DISTINCT FROM (h.fp_system_identifier, h.fp_database_oid, h.fp_timeline);
END
$in_recovery$;

-- NULL when publication may proceed, else the outage code.
CREATE FUNCTION audit_store.gate_code(h audit_store.publication_head)
RETURNS TEXT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $gate_code$
BEGIN
    IF audit_store.in_recovery(h) THEN
        RETURN 'store_recovery_required';
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.posture_check()) THEN
        RETURN 'store_posture_invalid';
    END IF;
    RETURN NULL;
END
$gate_code$;

-- Raises the gate failure (publication functions other than ingest/probe).
CREATE FUNCTION audit_store.require_gate(h audit_store.publication_head)
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $require_gate$
DECLARE
    v_code TEXT := audit_store.gate_code(h);
BEGIN
    IF v_code = 'store_recovery_required' THEN
        RAISE EXCEPTION 'store_recovery_required' USING ERRCODE = 'KA001';
    ELSIF v_code = 'store_posture_invalid' THEN
        RAISE EXCEPTION 'store_posture_invalid' USING ERRCODE = 'KA002';
    END IF;
END
$require_gate$;

-- Recovery only (bind/unbind/bootstrap/register_source_service must work
-- while the posture is being repaired, e.g. to bind a new ingest login).
CREATE FUNCTION audit_store.require_not_recovery(h audit_store.publication_head)
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $require_not_recovery$
BEGIN
    IF audit_store.in_recovery(h) THEN
        RAISE EXCEPTION 'store_recovery_required' USING ERRCODE = 'KA001';
    END IF;
END
$require_not_recovery$;

-- Locks and returns the head (lock order: head first, always). Every writer
-- calls this first (verify/checkpoint only before appending their result).
-- synchronous_commit is set transaction-locally because a function-level SET
-- clause would be reverted before COMMIT.
CREATE FUNCTION audit_store.lock_head()
RETURNS audit_store.publication_head LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lock_head$
DECLARE
    h audit_store.publication_head;
BEGIN
    PERFORM pg_catalog.set_config('synchronous_commit', 'on', TRUE);
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton FOR UPDATE;
    RETURN h;
END
$lock_head$;

CREATE FUNCTION audit_store.read_head()
RETURNS audit_store.publication_head LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $read_head$
DECLARE
    h audit_store.publication_head;
BEGIN
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
    RETURN h;
END
$read_head$;

-- ---------------------------------------------------------------------------
-- Principals and capabilities (design §10.2)
-- ---------------------------------------------------------------------------

-- The actor of a control event: the session's bound principal, or the DB
-- login itself under the fixed issuer db_role when unbound (including the
-- owner members that bootstrap and bind).
CREATE FUNCTION audit_store.session_actor(
    OUT issuer TEXT, OUT principal_id TEXT, OUT bound BOOLEAN)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $session_actor$
BEGIN
    SELECT b.issuer, b.principal_id, TRUE INTO issuer, principal_id, bound
    FROM audit_store.principal_bindings AS b
    WHERE b.db_role = session_user::text AND b.unbound_seq IS NULL;
    IF NOT FOUND THEN
        issuer := 'db_role';
        principal_id := session_user::text;
        bound := FALSE;
    END IF;
END
$session_actor$;

CREATE FUNCTION audit_store.has_grant(p_issuer TEXT, p_principal_id TEXT, p_capability TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $has_grant$
    SELECT EXISTS (
        SELECT 1 FROM audit_store.access_grants AS g
        WHERE g.issuer = p_issuer AND g.principal_id = p_principal_id
          AND g.capability = p_capability AND g.revoked_seq IS NULL)
$has_grant$;

-- NULL when the session is bound to a registered source-service principal
-- (for p_source, or for any source when p_source is NULL), else the denial
-- code ('unbound' / 'not_source_service').
CREATE FUNCTION audit_store.source_service_denial(p_source TEXT)
RETURNS TEXT LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $source_service_denial$
DECLARE
    v_actor record;
BEGIN
    SELECT * INTO v_actor FROM audit_store.session_actor();
    IF NOT v_actor.bound THEN
        RETURN 'unbound';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM audit_store.source_services AS s
        WHERE s.issuer = v_actor.issuer AND s.principal_id = v_actor.principal_id
          AND (p_source IS NULL OR s.source = p_source)) THEN
        RETURN 'not_source_service';
    END IF;
    RETURN NULL;
END
$source_service_denial$;

-- ---------------------------------------------------------------------------
-- Appending (head lock held by the caller)
-- ---------------------------------------------------------------------------

-- Appends one event. Columns derive from the envelope; origin comes from the
-- calling definer function. The caller has validated the envelope.
CREATE FUNCTION audit_store.append_event(
    p_origin TEXT, p_envelope JSONB, p_commitment BYTEA, p_adapter_version INTEGER)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET audit_store.write_context = 'definer' AS $append_event$
DECLARE
    h audit_store.publication_head;
    v_seq BIGINT;
    v_id UUID := (p_envelope ->> 'id')::uuid;
    v_digest BYTEA := audit_store.jsonb_digest(p_envelope);
    v_chain BYTEA;
    v_data JSONB := p_envelope -> 'data';
BEGIN
    h := audit_store.lock_head();
    v_seq := h.last_seq + 1;
    v_chain := audit_store.chain_step(h.last_chain, v_seq, v_id, v_digest);
    INSERT INTO audit_store.events (
        seq, event_id, origin, source, event_type, event_class, subject, occurred_at,
        stored_at, actor_issuer, actor_principal_id, resource_type, resource_id,
        resource_version_id, result, envelope_digest, digest_algorithm, source_commitment,
        adapter_version, prev_chain, chain, recovery_epoch, ingested_by_db_role)
    VALUES (
        v_seq, v_id, p_origin, p_envelope ->> 'source', p_envelope ->> 'type',
        v_data ->> 'event_class', p_envelope ->> 'subject', (p_envelope ->> 'time')::timestamptz,
        clock_timestamp(), v_data -> 'actor' ->> 'issuer', v_data -> 'actor' ->> 'principal_id',
        v_data -> 'resource' ->> 'type', v_data -> 'resource' ->> 'id',
        (v_data -> 'resource' ->> 'version_id')::uuid, v_data ->> 'result', v_digest,
        'kp-audit-jsonb-sha256-v1', p_commitment, p_adapter_version, h.last_chain, v_chain,
        h.recovery_epoch, session_user::text);
    INSERT INTO audit_store.event_bodies (seq, envelope) VALUES (v_seq, p_envelope);
    UPDATE audit_store.publication_head AS p
    SET last_seq = v_seq, last_chain = v_chain, updated_at = clock_timestamp()
    WHERE p.singleton;
    RETURN v_seq;
END
$append_event$;

-- Builds a control envelope (design §4.1, §4.5) for the given session role
-- and actor and appends it. No denial flush: append_control (the session's
-- own control events) and flush_denial_streaks (a streak's login) call it.
CREATE FUNCTION audit_store.append_control_as(
    p_origin TEXT, p_type TEXT, p_class TEXT, p_result TEXT, p_details JSONB,
    p_session_role TEXT, p_actor_issuer TEXT, p_actor_principal_id TEXT)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $append_control_as$
DECLARE
    v_envelope JSONB;
BEGIN
    IF p_origin NOT IN ('store', 'relay_control') THEN
        RAISE EXCEPTION 'append_control: invalid origin';
    END IF;
    -- session_role is a closed db_role value: a login whose name does not fit
    -- cannot produce control events (posture_check reports it).
    IF NOT audit_store.is_db_role(p_session_role) THEN
        RAISE EXCEPTION 'login_name_invalid' USING ERRCODE = '42501';
    END IF;
    v_envelope := jsonb_build_object(
        'specversion', '1.0',
        'id', uuidv7()::text,
        'source', CASE p_origin WHEN 'store' THEN 'urn:knowledge-platform:audit-store'
                                ELSE 'urn:knowledge-platform:audit-relay' END,
        'type', p_type,
        'subject', 'audit-store',
        'time', audit_store.utc_text(clock_timestamp()),
        'datacontenttype', 'application/json',
        'dataschema', 'urn:knowledge-platform:audit:payload:v1',
        'data', jsonb_build_object(
            'schema_version', 1,
            'event_class', p_class,
            'action', p_type,
            'actor', jsonb_build_object('issuer', p_actor_issuer,
                                        'principal_id', p_actor_principal_id),
            'resource', jsonb_build_object('type', 'AuditStore', 'id', 'audit-store'),
            'result', p_result,
            'correlation', '{}'::jsonb,
            'details', jsonb_build_object('session_role', p_session_role) || p_details,
            'extensions', '{}'::jsonb,
            'provenance', jsonb_build_object(
                'source_format', CASE p_origin WHEN 'store' THEN 'audit-store-control-v1'
                                               ELSE 'audit-relay-control-v1' END,
                'adapter_version', 1)));
    RETURN audit_store.append_event(p_origin, v_envelope, NULL, 1);
END
$append_control_as$;

-- Appends one audit.access.denied for a login (design §10.3, bounded shape
-- only): itself plus p_suppressed coalesced denials of the same streak.
CREATE FUNCTION audit_store.append_denial(
    p_session_role TEXT, p_actor_issuer TEXT, p_actor_principal_id TEXT, p_operation TEXT,
    p_denial_code TEXT, p_capability TEXT, p_suppressed BIGINT)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $append_denial$
BEGIN
    RETURN audit_store.append_control_as('store', 'audit.access.denied', 'SECURITY', 'denied',
        jsonb_build_object('operation', p_operation, 'denial_code', p_denial_code)
        || audit_store.opt('required_capability', to_jsonb(p_capability))
        || audit_store.opt('suppressed_since_last',
                           CASE WHEN p_suppressed > 0 THEN to_jsonb(p_suppressed) END),
        p_session_role, p_actor_issuer, p_actor_principal_id);
END
$append_denial$;

-- Chains the pending count of coalesced denial streaks of every login (no
-- sampling): the streaks older than the coalescing window (no later denial
-- of theirs would carry the count), or every streak with a pending count
-- when p_all (before integrity and disclosure evidence: verify,
-- checkpoint, intents, expire, purge). Each flush is one audit.access.denied
-- of the streak's login, code and actor, standing for one of the coalesced
-- denials (so it carries count - 1); the streak then continues from that
-- record. The caller holds the head lock and is outside recovery mode.
-- Returns the number of flush records.
CREATE FUNCTION audit_store.flush_denial_streaks(p_all BOOLEAN)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET audit_store.write_context = 'definer' AS $flush_denial_streaks$
DECLARE
    s audit_store.denial_streaks;
    v_seq BIGINT;
    v_count BIGINT := 0;
BEGIN
    FOR s IN
        SELECT * FROM audit_store.denial_streaks AS d
        WHERE d.suppressed > 0
          AND (p_all OR d.last_recorded_at <= clock_timestamp() - interval '1 minute')
        ORDER BY d.session_role
        FOR UPDATE
    LOOP
        v_seq := audit_store.append_denial(s.session_role, s.actor_issuer, s.actor_principal_id,
                                           s.last_operation, s.denial_code, NULL,
                                           s.suppressed - 1);
        UPDATE audit_store.denial_streaks AS d
        SET suppressed = 0, last_recorded_seq = v_seq, last_recorded_at = clock_timestamp()
        WHERE d.session_role = s.session_role;
        v_count := v_count + 1;
    END LOOP;
    RETURN v_count;
END
$flush_denial_streaks$;

-- Builds a control envelope (design §4.1, §4.5) under the session's own role
-- and principal and appends it, after chaining the stale denial streaks of
-- every login (flush_denial_streaks). The recovery epoch record is the
-- exception: the head was just reset to the restored head, and the epoch's
-- first record must be audit.recovery.epoch_started itself.
CREATE FUNCTION audit_store.append_control(
    p_origin TEXT, p_type TEXT, p_class TEXT, p_result TEXT, p_details JSONB)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $append_control$
DECLARE
    v_actor record;
BEGIN
    IF p_type <> 'audit.recovery.epoch_started' THEN
        PERFORM audit_store.flush_denial_streaks(FALSE);
    END IF;
    SELECT * INTO v_actor FROM audit_store.session_actor();
    RETURN audit_store.append_control_as(p_origin, p_type, p_class, p_result, p_details,
                                         session_user::text, v_actor.issuer,
                                         v_actor.principal_id);
END
$append_control$;

-- Records audit.access.denied with a bounded shape only (design §10.3).
-- Nothing is appended in recovery mode (the head must not move); the caller
-- still refuses.
CREATE FUNCTION audit_store.record_denied(
    p_operation TEXT, p_denial_code TEXT, p_capability TEXT, p_suppressed BIGINT DEFAULT NULL)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $record_denied$
DECLARE
    h audit_store.publication_head := audit_store.lock_head();
    v_actor record;
BEGIN
    IF audit_store.in_recovery(h) THEN
        RETURN NULL;
    END IF;
    PERFORM audit_store.flush_denial_streaks(FALSE);
    SELECT * INTO v_actor FROM audit_store.session_actor();
    RETURN audit_store.append_denial(session_user::text, v_actor.issuer, v_actor.principal_id,
                                     p_operation, p_denial_code, p_capability, p_suppressed);
END
$record_denied$;

-- Identity denials of the service paths (ingest, probe, report_regression)
-- repeat on every relay cycle: record at most one per streak and minute per
-- login, and chain how many were coalesced (no sampling): the next record
-- of the same code and actor carries `suppressed_since_last`; a change of
-- code or actor first flushes the pending count as a record of the old
-- streak (standing for one of the coalesced denials, so it carries count -
-- 1). A success of that login ends the streak (clear_denial_streak flushes
-- the same way); a streak that simply stops is flushed by
-- flush_denial_streaks. The login's own streak is settled here and removed
-- before anything is appended, so no flush counts it twice. In recovery
-- mode nothing is recorded and the streak is left as it is.
CREATE FUNCTION audit_store.record_denied_coalesced(p_operation TEXT, p_denial_code TEXT)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET audit_store.write_context = 'definer' AS $record_denied_coalesced$
DECLARE
    h audit_store.publication_head;
    s audit_store.denial_streaks;
    v_found BOOLEAN;
    v_actor record;
    v_same BOOLEAN;
    v_carried BIGINT := 0;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    SELECT * INTO v_actor FROM audit_store.session_actor();
    SELECT * INTO s FROM audit_store.denial_streaks AS d
    WHERE d.session_role = session_user::text FOR UPDATE;
    v_found := FOUND;
    v_same := v_found AND s.denial_code = p_denial_code
              AND s.actor_issuer = v_actor.issuer
              AND s.actor_principal_id = v_actor.principal_id;
    IF v_same AND s.last_recorded_at > clock_timestamp() - interval '1 minute' THEN
        UPDATE audit_store.denial_streaks AS d
        SET suppressed = d.suppressed + 1, last_operation = p_operation
        WHERE d.session_role = s.session_role;
        RETURN NULL;
    END IF;
    IF audit_store.in_recovery(h) THEN
        RETURN NULL;
    END IF;
    IF v_found THEN
        DELETE FROM audit_store.denial_streaks AS d WHERE d.session_role = s.session_role;
        IF s.suppressed > 0 THEN
            IF v_same THEN
                v_carried := s.suppressed;
            ELSE
                PERFORM audit_store.append_denial(s.session_role, s.actor_issuer,
                                                  s.actor_principal_id, s.last_operation,
                                                  s.denial_code, NULL, s.suppressed - 1);
            END IF;
        END IF;
    END IF;
    v_seq := audit_store.record_denied(p_operation, p_denial_code, NULL, v_carried);
    INSERT INTO audit_store.denial_streaks (
        session_role, denial_code, last_operation, last_recorded_seq, last_recorded_at,
        suppressed, actor_issuer, actor_principal_id)
    VALUES (session_user::text, p_denial_code, p_operation, v_seq, clock_timestamp(), 0,
            v_actor.issuer, v_actor.principal_id);
    RETURN v_seq;
END
$record_denied_coalesced$;

-- A success of the login ends its streak. A pending count is flushed first
-- as one record of the streak's code and actor (carrying count - 1); the
-- operational row is removed in the same transaction. Called under the head
-- lock outside recovery mode (ingest, probe).
CREATE FUNCTION audit_store.clear_denial_streak()
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET audit_store.write_context = 'definer' AS $clear_denial_streak$
DECLARE
    h audit_store.publication_head := audit_store.lock_head();
    s audit_store.denial_streaks;
BEGIN
    IF audit_store.in_recovery(h) THEN
        RETURN;
    END IF;
    SELECT * INTO s FROM audit_store.denial_streaks AS d
    WHERE d.session_role = session_user::text FOR UPDATE;
    IF NOT FOUND THEN
        RETURN;
    END IF;
    DELETE FROM audit_store.denial_streaks AS d WHERE d.session_role = s.session_role;
    IF s.suppressed > 0 THEN
        PERFORM audit_store.append_denial(s.session_role, s.actor_issuer, s.actor_principal_id,
                                          s.last_operation, s.denial_code, NULL,
                                          s.suppressed - 1);
    END IF;
END
$clear_denial_streak$;

-- Resolves the session principal and checks DB role + Audit capability.
-- Records audit.access.denied (outside recovery mode) and returns
-- ok = false when refused.
CREATE FUNCTION audit_store.authorize(
    p_operation TEXT, p_db_role TEXT, p_capability TEXT,
    OUT ok BOOLEAN, OUT issuer TEXT, OUT principal_id TEXT, OUT denial TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $authorize$
DECLARE
    v_actor record;
BEGIN
    SELECT * INTO v_actor FROM audit_store.session_actor();
    issuer := v_actor.issuer;
    principal_id := v_actor.principal_id;
    IF NOT v_actor.bound THEN
        denial := 'unbound';
    ELSIF NOT pg_has_role(session_user, p_db_role, 'MEMBER')
          OR NOT audit_store.has_grant(v_actor.issuer, v_actor.principal_id, p_capability) THEN
        denial := 'insufficient_capability';
    END IF;
    ok := denial IS NULL;
    IF NOT ok THEN
        PERFORM audit_store.record_denied(p_operation, denial, p_capability);
    END IF;
END
$authorize$;

-- ---------------------------------------------------------------------------
-- Ingest (design §7.2)
-- ---------------------------------------------------------------------------

-- NULL when the relay envelope passes the Store's structural checks, else a
-- rejection code (audit-core names, [a-z0-9_]{1,64}). The relay validated
-- the full catalog.
CREATE FUNCTION audit_store.relay_envelope_problem(p JSONB)
RETURNS TEXT LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $relay_envelope_problem$
DECLARE
    v_data JSONB;
    v_keys TEXT[];
    v_prov JSONB;
BEGIN
    IF p IS NULL OR jsonb_typeof(p) <> 'object' THEN
        RETURN 'invalid_envelope';
    END IF;
    IF octet_length(p::text) > 32768 THEN
        RETURN 'envelope_too_large';
    END IF;
    SELECT array_agg(k ORDER BY k) INTO v_keys FROM jsonb_object_keys(p) AS k;
    IF v_keys IS DISTINCT FROM ARRAY['data', 'datacontenttype', 'dataschema', 'id', 'source',
                                     'specversion', 'subject', 'time', 'type'] THEN
        RETURN 'invalid_envelope';
    END IF;
    IF p -> 'specversion' IS DISTINCT FROM '"1.0"'::jsonb
       OR p -> 'datacontenttype' IS DISTINCT FROM '"application/json"'::jsonb
       OR p -> 'dataschema' IS DISTINCT FROM '"urn:knowledge-platform:audit:payload:v1"'::jsonb
       OR jsonb_typeof(p -> 'id') IS DISTINCT FROM 'string'
       OR NOT audit_store.is_uuid_text(p ->> 'id')
       OR jsonb_typeof(p -> 'source') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p -> 'type') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p -> 'subject') IS DISTINCT FROM 'string'
       OR NOT audit_store.is_identifier(p ->> 'subject')
       OR audit_store.jts(p -> 'time') IS NULL
       OR jsonb_typeof(p -> 'data') IS DISTINCT FROM 'object' THEN
        RETURN 'invalid_envelope';
    END IF;
    v_data := p -> 'data';
    IF p ->> 'source' IN ('urn:knowledge-platform:audit-store',
                          'urn:knowledge-platform:audit-relay')
       OR (p ->> 'type') LIKE 'audit.%'
       OR v_data -> 'resource' ->> 'type' = 'AuditStore' THEN
        RETURN 'control_type_forbidden';
    END IF;
    IF NOT audit_store.is_identifier(p ->> 'source')
       OR NOT audit_store.is_identifier(p ->> 'type')
       OR v_data -> 'action' IS DISTINCT FROM p -> 'type'
       OR jsonb_typeof(v_data -> 'event_class') IS DISTINCT FROM 'string'
       OR jsonb_typeof(v_data -> 'result') IS DISTINCT FROM 'string' THEN
        RETURN 'invalid_envelope';
    END IF;
    IF jsonb_typeof(v_data -> 'actor') IS DISTINCT FROM 'object'
       OR jsonb_typeof(v_data -> 'actor' -> 'issuer') IS DISTINCT FROM 'string'
       OR jsonb_typeof(v_data -> 'actor' -> 'principal_id') IS DISTINCT FROM 'string'
       OR NOT audit_store.is_identifier(v_data -> 'actor' ->> 'issuer')
       OR NOT audit_store.is_identifier(v_data -> 'actor' ->> 'principal_id') THEN
        RETURN 'invalid_actor';
    END IF;
    IF jsonb_typeof(v_data -> 'resource') IS DISTINCT FROM 'object'
       OR jsonb_typeof(v_data -> 'resource' -> 'type') IS DISTINCT FROM 'string'
       OR jsonb_typeof(v_data -> 'resource' -> 'id') IS DISTINCT FROM 'string'
       OR NOT audit_store.is_identifier(v_data -> 'resource' ->> 'id')
       OR (v_data -> 'resource' ? 'version_id'
           AND NOT audit_store.is_uuid_text(v_data -> 'resource' ->> 'version_id')) THEN
        RETURN 'invalid_resource';
    END IF;
    v_prov := v_data -> 'provenance';
    IF jsonb_typeof(v_prov) IS DISTINCT FROM 'object'
       OR jsonb_typeof(v_prov -> 'source_format') IS DISTINCT FROM 'string'
       OR audit_store.jint(v_prov -> 'adapter_version') IS NULL
       OR audit_store.jint(v_prov -> 'adapter_version') NOT BETWEEN 1 AND 2147483647
       OR (v_prov ? 'source_commitment'
           AND NOT audit_store.is_hex64(v_prov ->> 'source_commitment'))
       OR (p ->> 'source' = 'urn:knowledge-platform:document-platform'
           AND NOT v_prov ? 'source_commitment') THEN
        RETURN 'invalid_provenance';
    END IF;
    RETURN NULL;
END
$relay_envelope_problem$;

-- Structured result (status, seq, envelope_digest, adapter_version, code),
-- decoded by audit_core::IngestRow::into_result. Verdicts (rejected,
-- conflict) are rows, never exceptions; every other status is an outage.
CREATE FUNCTION audit_store.ingest(p_envelope JSONB)
RETURNS TABLE (status TEXT, seq BIGINT, envelope_digest BYTEA, adapter_version INTEGER,
               code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $ingest$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_problem TEXT;
    v_id UUID;
    v_source TEXT;
    v_type TEXT;
    v_adapter INTEGER;
    v_format TEXT;
    v_commitment BYTEA;
    v_digest BYTEA;
    e audit_store.events;
    v_kind TEXT;
    v_new BIGINT;
BEGIN
    PERFORM pg_catalog.set_config('synchronous_commit', 'on', TRUE);
    h := audit_store.lock_head();
    -- Recovery first: nothing (not even a denial) is appended in recovery mode.
    IF audit_store.in_recovery(h) THEN
        RETURN QUERY SELECT 'recovery_required'::text, NULL::bigint, NULL::bytea,
                            NULL::integer, 'store_recovery_required'::text;
        RETURN;
    END IF;
    -- Only a login bound to a registered source-service principal ingests.
    v_problem := audit_store.source_service_denial(NULL);
    IF v_problem IS NOT NULL THEN
        PERFORM audit_store.record_denied_coalesced('ingest', v_problem);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bytea, NULL::integer, v_problem;
        RETURN;
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.posture_check()) THEN
        RETURN QUERY SELECT 'outage'::text, NULL::bigint, NULL::bytea, NULL::integer,
                            'posture_invalid'::text;
        RETURN;
    END IF;
    v_problem := audit_store.relay_envelope_problem(p_envelope);
    IF v_problem IS NOT NULL THEN
        IF NOT audit_store.is_code(v_problem) THEN
            v_problem := 'invalid_envelope';
        END IF;
        RETURN QUERY SELECT 'rejected'::text, NULL::bigint, NULL::bytea, NULL::integer,
                            v_problem;
        RETURN;
    END IF;
    v_id := (p_envelope ->> 'id')::uuid;
    v_source := p_envelope ->> 'source';
    v_type := p_envelope ->> 'type';
    v_adapter := audit_store.jint(p_envelope -> 'data' -> 'provenance' -> 'adapter_version');
    v_format := p_envelope -> 'data' -> 'provenance' ->> 'source_format';
    v_commitment := decode(p_envelope -> 'data' -> 'provenance' ->> 'source_commitment', 'hex');
    -- provenance must equal a registered (catalog) adapter version and format.
    IF NOT EXISTS (
        SELECT 1 FROM audit_store.registered_types AS r
        WHERE r.source = v_source AND r.event_type = v_type AND r.adapter_version = v_adapter
          AND r.source_format = v_format
    ) THEN
        -- Catalog and Store migration disagree: an outage, never a verdict.
        RETURN QUERY SELECT 'outage'::text, NULL::bigint, NULL::bytea, NULL::integer,
                            'unregistered_type'::text;
        RETURN;
    END IF;
    -- The service principal must be registered for this source.
    v_problem := audit_store.source_service_denial(v_source);
    IF v_problem IS NOT NULL THEN
        PERFORM audit_store.record_denied_coalesced('ingest', v_problem);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bytea, NULL::integer, v_problem;
        RETURN;
    END IF;
    PERFORM audit_store.clear_denial_streak();
    v_digest := audit_store.jsonb_digest(p_envelope);

    SELECT * INTO e FROM audit_store.events AS x WHERE x.event_id = v_id;
    IF FOUND THEN
        IF e.origin <> 'relay' OR e.source <> v_source OR e.event_type <> v_type THEN
            v_kind := 'identity_mismatch';
        ELSIF e.source_commitment IS NOT NULL AND v_commitment IS NOT NULL THEN
            IF e.source_commitment <> v_commitment THEN
                v_kind := 'commitment_mismatch';
            ELSIF e.envelope_digest = v_digest THEN
                v_kind := NULL;
            ELSIF e.adapter_version <> v_adapter THEN
                RETURN QUERY SELECT 'duplicate_reprojected'::text, e.seq, e.envelope_digest,
                                    e.adapter_version, NULL::text;
                RETURN;
            ELSE
                v_kind := 'projection_mismatch';
            END IF;
        ELSIF e.envelope_digest <> v_digest THEN
            -- A commitment-less adapter: duplicates need identical envelopes.
            v_kind := 'digest_mismatch';
        END IF;
        IF v_kind IS NULL THEN
            RETURN QUERY SELECT CASE WHEN e.expired_at IS NULL THEN 'duplicate'
                                     ELSE 'duplicate_expired' END::text,
                                e.seq, e.envelope_digest, e.adapter_version, NULL::text;
            RETURN;
        END IF;
        PERFORM audit_store.append_control('store', 'audit.integrity.conflict_detected',
            'SECURITY', 'failure', jsonb_build_object(
                'event_id', v_id::text,
                'existing_seq', e.seq,
                'existing_origin', e.origin,
                'conflict_kind', v_kind,
                'commitment_match', coalesce(e.source_commitment = v_commitment, FALSE),
                'existing_adapter_version', e.adapter_version,
                'submitted_adapter_version', v_adapter));
        RETURN QUERY SELECT 'conflict'::text, e.seq, e.envelope_digest, e.adapter_version,
                            v_kind;
        RETURN;
    END IF;

    -- Stale denial streaks of every login are chained before the event.
    PERFORM audit_store.flush_denial_streaks(FALSE);
    v_new := audit_store.append_event('relay', p_envelope, v_commitment, v_adapter);
    RETURN QUERY SELECT 'stored'::text, v_new, v_digest, v_adapter, NULL::text;
END
$ingest$;

-- What the recorded verifications cover (design §8, §12 verification lag),
-- never just the newest record:
--   - violated: the latest origin=store audit.integrity.verified that found
--     violations (result 'failure');
--   - covered: the 'ok' verifications recorded after that violation that
--     chain from genesis (one starting at from_seq 1, each next one starting
--     at most one past the coverage so far and reaching further); the
--     latest 1000 such records are considered (an underestimate is the safe
--     side). verified_through is the highest seq they reach;
--   - outcome: 'violations' until one 'ok' verification after it covered
--     the whole range up to the head of its own scan (from_seq 1, to_seq =
--     watermark), else 'ok' (NULL when nothing is covered). Partial ranges,
--     even ones that chain to the head, never clear a violation.
-- verified_at and record_seq belong to the record that decides the outcome.
CREATE FUNCTION audit_store.verification_coverage(
    OUT verified_through BIGINT, OUT verified_at TIMESTAMPTZ, OUT outcome TEXT,
    OUT record_seq BIGINT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $verification_coverage$
DECLARE
    v_bad_seq BIGINT;
    v_bad_at TIMESTAMPTZ;
    v_ok_to BIGINT;
    v_ok_seq BIGINT;
    v_ok_at TIMESTAMPTZ;
    v_full BOOLEAN;
BEGIN
    SELECT e.seq, e.stored_at INTO v_bad_seq, v_bad_at
    FROM audit_store.events AS e
    WHERE e.origin = 'store' AND e.event_type = 'audit.integrity.verified'
      AND e.result = 'failure'
    ORDER BY e.seq DESC LIMIT 1;
    WITH RECURSIVE ok AS (
        SELECT e.seq, e.stored_at,
               audit_store.jint(b.envelope -> 'data' -> 'details' -> 'from_seq') AS f,
               audit_store.jint(b.envelope -> 'data' -> 'details' -> 'to_seq') AS t
        FROM audit_store.events AS e
        JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.origin = 'store' AND e.event_type = 'audit.integrity.verified'
          AND e.result = 'success' AND e.seq > coalesce(v_bad_seq, 0)
          AND b.envelope -> 'data' -> 'details' ->> 'outcome' = 'ok'
        ORDER BY e.seq DESC LIMIT 1000
    ),
    chained(t, seq, stored_at) AS (
        SELECT ok.t, ok.seq, ok.stored_at FROM ok WHERE ok.f = 1 AND ok.t >= 0
        UNION
        SELECT ok.t, ok.seq, ok.stored_at FROM ok
        JOIN chained AS c ON ok.f <= c.t + 1 AND ok.t > c.t
    )
    SELECT c.t, c.seq, c.stored_at INTO v_ok_to, v_ok_seq, v_ok_at
    FROM chained AS c ORDER BY c.t DESC, c.seq DESC LIMIT 1;
    SELECT EXISTS (
        SELECT 1 FROM audit_store.events AS e
        JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.origin = 'store' AND e.event_type = 'audit.integrity.verified'
          AND e.result = 'success' AND e.seq > coalesce(v_bad_seq, 0)
          AND b.envelope -> 'data' -> 'details' ->> 'outcome' = 'ok'
          AND audit_store.jint(b.envelope -> 'data' -> 'details' -> 'from_seq') = 1
          AND audit_store.jint(b.envelope -> 'data' -> 'details' -> 'to_seq')
              >= audit_store.jint(b.envelope -> 'data' -> 'details' -> 'watermark'))
    INTO v_full;
    verified_through := v_ok_to;
    IF v_bad_seq IS NOT NULL AND NOT coalesce(v_full, FALSE) THEN
        outcome := 'violations';
        verified_at := v_bad_at;
        record_seq := v_bad_seq;
    ELSIF v_ok_to IS NOT NULL THEN
        outcome := 'ok';
        verified_at := v_ok_at;
        record_seq := v_ok_seq;
    END IF;
END
$verification_coverage$;

-- Admission probe for the relay circuit breaker (design §6.2): the ingest
-- gate without storing anything, the registration of the expected types and
-- the identity check of the relay's last acknowledged receipt.
--
-- status 'ok' with state operational / recovery_mode / posture_invalid /
-- read_only, or status 'denied' (the login is not a source service).
CREATE FUNCTION audit_store.probe(
    p_source TEXT, p_adapter_version INTEGER, p_types TEXT[],
    p_last_seq BIGINT, p_last_event_id UUID, p_last_digest BYTEA)
RETURNS TABLE (status TEXT, state TEXT, head_seq BIGINT, recovery_epoch BIGINT,
               missing_types TEXT[], regression_detected BOOLEAN, last_verified_seq BIGINT,
               code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '2s' AS $probe$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_state TEXT;
    v_denial TEXT;
    v_missing TEXT[];
    v_regressed BOOLEAN;
    v_verified BIGINT;
BEGIN
    IF NOT audit_store.is_identifier(p_source)
       OR p_adapter_version IS NULL OR p_adapter_version < 1
       OR coalesce(cardinality(p_types), 0) > 1000
       OR EXISTS (SELECT 1 FROM unnest(p_types) AS t WHERE NOT audit_store.is_event_type(t))
       OR (p_last_seq IS NULL) <> (p_last_event_id IS NULL)
       OR (p_last_seq IS NULL) <> (p_last_digest IS NULL)
       OR (p_last_seq IS NOT NULL AND (p_last_seq < 1 OR octet_length(p_last_digest) <> 32)) THEN
        RAISE EXCEPTION 'probe: invalid expectation' USING ERRCODE = '22023';
    END IF;
    SELECT c.verified_through INTO v_verified FROM audit_store.verification_coverage() AS c;
    IF current_setting('transaction_read_only') = 'on' OR pg_is_in_recovery() THEN
        h := audit_store.read_head();
        RETURN QUERY SELECT 'ok'::text, 'read_only'::text, h.last_seq, h.recovery_epoch,
                            ARRAY[]::text[], FALSE, v_verified, 'store_read_only'::text;
        RETURN;
    END IF;
    h := audit_store.lock_head();
    IF audit_store.in_recovery(h) THEN
        v_state := 'recovery_mode';
    ELSE
        v_denial := audit_store.source_service_denial(p_source);
        IF v_denial IS NOT NULL THEN
            PERFORM audit_store.record_denied_coalesced('ingest', v_denial);
            RETURN QUERY SELECT 'denied'::text, NULL::text, h.last_seq, h.recovery_epoch,
                                ARRAY[]::text[], FALSE, v_verified, v_denial;
            RETURN;
        END IF;
        PERFORM audit_store.clear_denial_streak();
        -- The relay probes on every cycle: a denial streak of any login that
        -- simply stopped is chained within the coalescing window plus one
        -- cycle, even when nothing else is appended.
        PERFORM audit_store.flush_denial_streaks(FALSE);
        v_state := CASE WHEN EXISTS (SELECT 1 FROM audit_store.posture_check())
                        THEN 'posture_invalid' ELSE 'operational' END;
    END IF;
    SELECT coalesce(array_agg(t ORDER BY t), ARRAY[]::text[]) INTO v_missing
    FROM (SELECT DISTINCT t FROM unnest(p_types) AS t) AS x(t)
    WHERE NOT EXISTS (
        SELECT 1 FROM audit_store.registered_types AS r
        WHERE r.source = p_source AND r.event_type = x.t AND r.adapter_version = p_adapter_version);
    -- Identity, never seq comparison: the acknowledged receipt must still
    -- resolve to the same (seq, event_id, envelope digest).
    v_regressed := p_last_seq IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM audit_store.events AS e
        WHERE e.seq = p_last_seq AND e.event_id = p_last_event_id
          AND e.envelope_digest = p_last_digest);
    RETURN QUERY SELECT 'ok'::text, v_state, h.last_seq, h.recovery_epoch, v_missing,
                        v_regressed, v_verified,
                        CASE v_state WHEN 'recovery_mode' THEN 'store_recovery_required'
                                     WHEN 'posture_invalid' THEN 'store_posture_invalid'
                                     ELSE NULL END::text;
END
$probe$;

-- The relay reports that its acknowledged receipt no longer resolves
-- (design §11). Re-checked under the head lock; only a missing identity sets
-- recovery_pending (reason 'regression') with the reported evidence. Allowed
-- in recovery mode (it only sets the flag).
CREATE FUNCTION audit_store.report_regression(
    p_seq BIGINT, p_event_id UUID, p_envelope_digest BYTEA)
RETURNS TABLE (status TEXT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $report_regression$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_denial TEXT;
BEGIN
    h := audit_store.lock_head();
    v_denial := audit_store.source_service_denial(NULL);
    IF v_denial IS NOT NULL THEN
        PERFORM audit_store.record_denied_coalesced('report_regression', v_denial);
        RETURN QUERY SELECT 'denied'::text, v_denial;
        RETURN;
    END IF;
    IF p_seq IS NULL OR p_seq < 1 OR p_event_id IS NULL
       OR p_envelope_digest IS NULL OR octet_length(p_envelope_digest) <> 32 THEN
        RETURN QUERY SELECT 'invalid_input'::text, 'invalid_input'::text;
        RETURN;
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.events AS e
               WHERE e.seq = p_seq AND e.event_id = p_event_id
                 AND e.envelope_digest = p_envelope_digest) THEN
        RETURN QUERY SELECT 'intact'::text, NULL::text;
        RETURN;
    END IF;
    IF h.recovery_pending THEN
        RETURN QUERY SELECT 'already_pending'::text, h.pending_reason;
        RETURN;
    END IF;
    UPDATE audit_store.publication_head AS x
    SET recovery_pending = TRUE, pending_reason = 'regression', pending_seq = p_seq,
        pending_event_id = p_event_id, pending_digest = p_envelope_digest,
        pending_by = session_user::text, pending_at = clock_timestamp(),
        pending_head_seq = h.last_seq, pending_incident_code = NULL,
        updated_at = clock_timestamp()
    WHERE x.singleton;
    RETURN QUERY SELECT 'recovery_pending'::text, 'regression'::text;
END
$report_regression$;

-- Content-free status for health and operators (design §10.4, §12). Not
-- audited; works in recovery mode. last_verified_seq / _at / _outcome are
-- the verification coverage (verification_coverage), so that head_seq -
-- last_verified_seq is the verification lag and a violation stays reported
-- until a later verification from genesis covers it. denials_pending is the
-- number of coalesced denials not chained yet (every one is chained by the
-- next record of its streak, by flush_denial_streaks within the coalescing
-- window plus the next append, and before any verify, checkpoint, intent,
-- expire or purge).
CREATE FUNCTION audit_store.store_status()
RETURNS TABLE (head_seq BIGINT, recovery_epoch BIGINT, recovery_mode BOOLEAN,
               posture_ok BOOLEAN, recovery_pending BOOLEAN, recovery_pending_reason TEXT,
               access_reapply_pending BOOLEAN, last_verified_seq BIGINT,
               last_verified_at TIMESTAMPTZ, last_verified_outcome TEXT,
               denials_pending BIGINT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $store_status$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head := audit_store.read_head();
BEGIN
    RETURN QUERY
    SELECT h.last_seq, h.recovery_epoch, audit_store.in_recovery(h),
           NOT EXISTS (SELECT 1 FROM audit_store.posture_check()),
           h.recovery_pending, h.pending_reason, h.access_reapply_pending,
           c.verified_through, c.verified_at, c.outcome,
           (SELECT coalesce(sum(d.suppressed), 0)::bigint FROM audit_store.denial_streaks AS d)
    FROM audit_store.verification_coverage() AS c;
END
$store_status$;

-- Content-free receipts for the relay (design §10.4). No control event.
CREATE FUNCTION audit_store.lookup_receipts(p_event_ids UUID[])
RETURNS TABLE (event_id UUID, seq BIGINT, origin TEXT, event_type TEXT,
               envelope_digest BYTEA, source_commitment BYTEA, adapter_version INTEGER,
               expired BOOLEAN, recovery_epoch BIGINT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lookup_receipts$
#variable_conflict use_column
BEGIN
    IF coalesce(cardinality(p_event_ids), 0) > 1000 THEN
        RAISE EXCEPTION 'lookup_receipts: at most 1000 ids' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.event_id, e.seq, e.origin, e.event_type, e.envelope_digest, e.source_commitment,
           e.adapter_version, e.expired_at IS NOT NULL, e.recovery_epoch
    FROM audit_store.events AS e
    WHERE e.event_id = ANY (p_event_ids)
    ORDER BY e.seq;
END
$lookup_receipts$;

CREATE FUNCTION audit_store.list_source_receipts(
    p_source TEXT, p_after_seq BIGINT, p_limit INTEGER)
RETURNS TABLE (event_id UUID, seq BIGINT, origin TEXT, event_type TEXT,
               envelope_digest BYTEA, source_commitment BYTEA, adapter_version INTEGER,
               expired BOOLEAN, recovery_epoch BIGINT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $list_source_receipts$
#variable_conflict use_column
BEGIN
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'list_source_receipts: limit must be 1..1000' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.event_id, e.seq, e.origin, e.event_type, e.envelope_digest, e.source_commitment,
           e.adapter_version, e.expired_at IS NOT NULL, e.recovery_epoch
    FROM audit_store.events AS e
    WHERE e.origin = 'relay' AND e.source = p_source AND e.seq > coalesce(p_after_seq, 0)
    ORDER BY e.seq
    LIMIT p_limit;
END
$list_source_receipts$;

-- Control receipts by seq (design §6.4): content-free, plus the event a
-- relay control event is about and its machine code (replay_requested:
-- quarantine_code; source_mismatch_detected: mismatch_code;
-- reconciliation.completed: mode).
CREATE FUNCTION audit_store.lookup_control_receipts(p_seqs BIGINT[])
RETURNS TABLE (seq BIGINT, recovery_epoch BIGINT, origin TEXT, event_type TEXT,
               target_event_id UUID, code TEXT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lookup_control_receipts$
#variable_conflict use_column
BEGIN
    IF coalesce(cardinality(p_seqs), 0) > 1000 THEN
        RAISE EXCEPTION 'lookup_control_receipts: at most 1000 seqs' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.seq, e.recovery_epoch, e.origin, e.event_type,
           CASE WHEN audit_store.is_uuid_text(d.details ->> 'event_id')
                THEN (d.details ->> 'event_id')::uuid
                WHEN audit_store.is_uuid_text(d.details ->> 'target_event_id')
                THEN (d.details ->> 'target_event_id')::uuid END,
           CASE e.event_type
               WHEN 'audit.delivery.replay_requested' THEN d.details ->> 'quarantine_code'
               WHEN 'audit.integrity.source_mismatch_detected' THEN d.details ->> 'mismatch_code'
               WHEN 'audit.reconciliation.completed' THEN d.details ->> 'mode'
           END
    FROM audit_store.events AS e
    LEFT JOIN LATERAL (
        SELECT b.envelope -> 'data' -> 'details' AS details
        FROM audit_store.event_bodies AS b WHERE b.seq = e.seq) AS d ON TRUE
    WHERE e.seq = ANY (p_seqs) AND e.origin IN ('store', 'relay_control')
    ORDER BY e.seq;
END
$lookup_control_receipts$;

-- Declared lost ranges of every recovery epoch (content-free, from the
-- chained audit.recovery.epoch_started bodies). Reconcile classifies relay
-- control seqs inside them as replay_record_lost (design §6.4).
CREATE FUNCTION audit_store.lookup_lost_ranges()
RETURNS TABLE (epoch_seq BIGINT, old_epoch BIGINT, new_epoch BIGINT, classification TEXT,
               restored_head_seq BIGINT, lost_from_seq BIGINT, lost_upper_seq BIGINT,
               lost_upper_known BOOLEAN)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lookup_lost_ranges$
#variable_conflict use_column
BEGIN
    RETURN QUERY
    SELECT e.seq,
           audit_store.jint(d -> 'old_epoch'), audit_store.jint(d -> 'new_epoch'),
           d ->> 'classification',
           audit_store.jint(d -> 'restored_head_seq'),
           audit_store.jint(d -> 'lost_from_seq'), audit_store.jint(d -> 'lost_upper_seq'),
           coalesce((d -> 'lost_upper_known')::text = 'true', FALSE)
    FROM audit_store.events AS e
    JOIN audit_store.event_bodies AS b ON b.seq = e.seq
    CROSS JOIN LATERAL (SELECT b.envelope -> 'data' -> 'details') AS x(d)
    WHERE e.origin = 'store' AND e.event_type = 'audit.recovery.epoch_started'
    ORDER BY e.seq;
END
$lookup_lost_ranges$;

-- ---------------------------------------------------------------------------
-- Relay control events (design §4.5, §6.4, §12)
-- ---------------------------------------------------------------------------

-- Validates the details built by audit_core::RelayControl::details() (the
-- catalog fields except session_role) and normalizes them; NULL if invalid.
CREATE FUNCTION audit_store.relay_control_details(p_type TEXT, p_details JSONB)
RETURNS JSONB LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $relay_control_details$
DECLARE
    v_keys TEXT[];
    v_counts TEXT[] := ARRAY[
        'count_delivered_missing', 'count_digest_mismatch', 'count_ok', 'count_pending',
        'count_quarantined', 'count_quarantined_conflict', 'count_quarantined_stored',
        'count_relay_catalog_skew', 'count_replay_record_lost', 'count_source_tampered',
        'count_store_only', 'count_unaudited_replay', 'count_unregistered',
        'repaired_delivered_missing', 'repaired_quarantined_stored', 'repaired_unregistered'];
    v_out JSONB;
    k TEXT;
BEGIN
    IF jsonb_typeof(p_details) IS DISTINCT FROM 'object' THEN
        RETURN NULL;
    END IF;
    SELECT array_agg(x ORDER BY x) INTO v_keys FROM jsonb_object_keys(p_details) AS x;
    IF p_type = 'audit.delivery.replay_requested' THEN
        IF v_keys IS DISTINCT FROM ARRAY['event_id', 'quarantine_code']
           OR NOT audit_store.is_uuid_text(p_details ->> 'event_id')
           OR jsonb_typeof(p_details -> 'quarantine_code') IS DISTINCT FROM 'string'
           OR NOT audit_store.is_code(p_details ->> 'quarantine_code') THEN
            RETURN NULL;
        END IF;
        RETURN jsonb_build_object('event_id', p_details ->> 'event_id',
                                  'quarantine_code', p_details ->> 'quarantine_code');
    ELSIF p_type = 'audit.integrity.source_mismatch_detected' THEN
        IF v_keys IS DISTINCT FROM ARRAY['event_id', 'mismatch_code']
           OR NOT audit_store.is_uuid_text(p_details ->> 'event_id')
           OR coalesce(p_details ->> 'mismatch_code', '')
              NOT IN ('source_digest_mismatch', 'actor_mismatch') THEN
            RETURN NULL;
        END IF;
        RETURN jsonb_build_object('event_id', p_details ->> 'event_id',
                                  'mismatch_code', p_details ->> 'mismatch_code');
    ELSIF p_type = 'audit.reconciliation.completed' THEN
        IF v_keys IS DISTINCT FROM (
               SELECT array_agg(x ORDER BY x) FROM unnest(
                   v_counts || ARRAY['id_set_digest', 'mode', 'run_id', 'watermark']) AS x)
           OR NOT audit_store.is_uuid_text(p_details ->> 'run_id')
           OR coalesce(p_details ->> 'mode', '') NOT IN ('read_only', 'repair')
           OR jsonb_typeof(p_details -> 'id_set_digest') IS DISTINCT FROM 'string'
           OR NOT audit_store.is_hex64(p_details ->> 'id_set_digest')
           OR coalesce(audit_store.jint(p_details -> 'watermark') < 0, TRUE) THEN
            RETURN NULL;
        END IF;
        v_out := jsonb_build_object(
            'run_id', p_details ->> 'run_id',
            'mode', p_details ->> 'mode',
            'watermark', audit_store.jint(p_details -> 'watermark'),
            'id_set_digest', p_details ->> 'id_set_digest');
        FOREACH k IN ARRAY v_counts LOOP
            IF coalesce(audit_store.jint(p_details -> k) < 0, TRUE) THEN
                RETURN NULL;
            END IF;
            v_out := v_out || jsonb_build_object(k, audit_store.jint(p_details -> k));
        END LOOP;
        RETURN v_out;
    END IF;
    RETURN NULL;
END
$relay_control_details$;

-- Records a relay control event under the caller's bound principal and
-- returns its (seq, epoch). source_mismatch_detected is idempotent on
-- (event_id, mismatch_code): a retry returns the original receipt.
CREATE FUNCTION audit_store.record_relay_control(p_type TEXT, p_details JSONB)
RETURNS TABLE (status TEXT, seq BIGINT, recovery_epoch BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $record_relay_control$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_actor record;
    v_details JSONB;
    v_existing audit_store.events;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_actor FROM audit_store.session_actor();
    IF NOT v_actor.bound THEN
        PERFORM audit_store.record_denied('record_relay_control', 'unbound', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, 'unbound'::text;
        RETURN;
    END IF;
    v_details := audit_store.relay_control_details(p_type, p_details);
    IF v_details IS NULL THEN
        PERFORM audit_store.record_denied('record_relay_control', 'invalid_input', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    -- A replay and a repair run are operator decisions (design §6.4, §10.1):
    -- never recorded under an ingest-capable (source service) login, so the
    -- privileged control event names the operator who acted.
    IF (p_type = 'audit.delivery.replay_requested'
        OR (p_type = 'audit.reconciliation.completed' AND v_details ->> 'mode' = 'repair'))
       AND pg_has_role(session_user, 'audit_store_ingest', 'MEMBER') THEN
        PERFORM audit_store.record_denied('record_relay_control', 'insufficient_capability',
                                          NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint,
                            'insufficient_capability'::text;
        RETURN;
    END IF;
    IF p_type = 'audit.integrity.source_mismatch_detected' THEN
        SELECT e.* INTO v_existing FROM audit_store.events AS e
        JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.event_type = p_type AND e.origin = 'relay_control'
          AND b.envelope -> 'data' -> 'details' ->> 'event_id' = v_details ->> 'event_id'
          AND b.envelope -> 'data' -> 'details' ->> 'mismatch_code'
              = v_details ->> 'mismatch_code'
        ORDER BY e.seq LIMIT 1;
        IF v_existing.seq IS NOT NULL THEN
            RETURN QUERY SELECT 'recorded'::text, v_existing.seq, v_existing.recovery_epoch,
                                'duplicate'::text;
            RETURN;
        END IF;
    END IF;
    v_seq := audit_store.append_control('relay_control', p_type,
        CASE p_type WHEN 'audit.delivery.replay_requested' THEN 'PRIVILEGED_OPERATION'
                    WHEN 'audit.integrity.source_mismatch_detected' THEN 'SECURITY'
                    ELSE 'SYSTEM_AUDIT' END,
        CASE p_type WHEN 'audit.integrity.source_mismatch_detected' THEN 'failure'
                    ELSE 'success' END,
        v_details);
    RETURN QUERY SELECT 'recorded'::text, v_seq, h.recovery_epoch, NULL::text;
END
$record_relay_control$;

-- ---------------------------------------------------------------------------
-- Two-phase disclosure (design §10.3)
-- ---------------------------------------------------------------------------

-- Validates and normalizes a filter. NULL when invalid. verify and
-- identity_chain read the contiguous chain (all origins) and accept only
-- seq_after / seq_through. seq_through must not exceed the watermark.
CREATE FUNCTION audit_store.normalize_filter(
    p_filter JSONB, p_operation TEXT, p_watermark BIGINT)
RETURNS JSONB LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $normalize_filter$
DECLARE
    f JSONB := coalesce(p_filter, '{}'::jsonb);
    v_out JSONB := '{}'::jsonb;
    v_allowed TEXT[];
    v_types TEXT[];
    v_ids TEXT[];
    v_from TIMESTAMPTZ;
    v_to TIMESTAMPTZ;
BEGIN
    -- Types are checked before any array/object function touches a value.
    IF jsonb_typeof(f) <> 'object' THEN
        RETURN NULL;
    END IF;
    v_allowed := CASE WHEN p_operation IN ('verify', 'identity_chain')
                      THEN ARRAY['seq_after', 'seq_through']
                      ELSE ARRAY['actor', 'event_ids', 'event_types', 'occurred_from',
                                 'occurred_to', 'resource', 'seq_after', 'seq_through',
                                 'source'] END;
    IF EXISTS (SELECT 1 FROM jsonb_object_keys(f) AS k WHERE k <> ALL (v_allowed)) THEN
        RETURN NULL;
    END IF;
    IF f ? 'event_types' THEN
        IF jsonb_typeof(f -> 'event_types') <> 'array' THEN
            RETURN NULL;
        END IF;
        IF jsonb_array_length(f -> 'event_types') NOT BETWEEN 1 AND 16
           OR EXISTS (SELECT 1 FROM jsonb_array_elements(f -> 'event_types') AS t
                      WHERE jsonb_typeof(t) <> 'string') THEN
            RETURN NULL;
        END IF;
        -- Only registered relay types and catalog control types (design
        -- §10.2): an unknown name is invalid_input and is never recorded.
        IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(f -> 'event_types') AS t
                   WHERE NOT audit_store.is_event_type(t)
                      OR NOT (audit_store.is_registered_type(t)
                              OR audit_store.is_control_type(t))) THEN
            RETURN NULL;
        END IF;
        SELECT array_agg(DISTINCT t ORDER BY t) INTO v_types
        FROM jsonb_array_elements_text(f -> 'event_types') AS t;
        v_out := v_out || jsonb_build_object('event_types', to_jsonb(v_types));
    END IF;
    IF f ? 'source' THEN
        IF jsonb_typeof(f -> 'source') <> 'string'
           OR NOT audit_store.is_source_urn(f ->> 'source', FALSE) THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('source', f ->> 'source');
    END IF;
    IF f ? 'actor' THEN
        IF jsonb_typeof(f -> 'actor') <> 'object' THEN
            RETURN NULL;
        END IF;
        IF EXISTS (SELECT 1 FROM jsonb_object_keys(f -> 'actor') AS k
                   WHERE k NOT IN ('issuer', 'principal_id'))
           OR jsonb_typeof(f -> 'actor' -> 'issuer') IS DISTINCT FROM 'string'
           OR jsonb_typeof(f -> 'actor' -> 'principal_id') IS DISTINCT FROM 'string'
           OR NOT audit_store.is_principal_part(f -> 'actor' ->> 'issuer')
           OR NOT audit_store.is_principal_part(f -> 'actor' ->> 'principal_id') THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('actor', jsonb_build_object(
            'issuer', f -> 'actor' ->> 'issuer', 'principal_id', f -> 'actor' ->> 'principal_id'));
    END IF;
    IF f ? 'resource' THEN
        IF jsonb_typeof(f -> 'resource') <> 'object' THEN
            RETURN NULL;
        END IF;
        IF EXISTS (SELECT 1 FROM jsonb_object_keys(f -> 'resource') AS k
                   WHERE k NOT IN ('id', 'type'))
           OR jsonb_typeof(f -> 'resource' -> 'type') IS DISTINCT FROM 'string'
           OR coalesce(f -> 'resource' ->> 'type', '') NOT IN
              ('Document', 'Folder', 'AccessPolicy', 'AuditStore')
           OR (f -> 'resource' ? 'id'
               AND (jsonb_typeof(f -> 'resource' -> 'id') IS DISTINCT FROM 'string'
                    OR NOT audit_store.is_resource_ref(f -> 'resource' ->> 'id'))) THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('resource',
            jsonb_build_object('type', f -> 'resource' ->> 'type')
            || audit_store.opt('id', f -> 'resource' -> 'id'));
    END IF;
    IF f ? 'occurred_from' THEN
        v_from := audit_store.jts(f -> 'occurred_from');
        IF v_from IS NULL THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('occurred_from', audit_store.utc_text(v_from));
    END IF;
    IF f ? 'occurred_to' THEN
        v_to := audit_store.jts(f -> 'occurred_to');
        IF v_to IS NULL OR (v_from IS NOT NULL AND v_to <= v_from) THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('occurred_to', audit_store.utc_text(v_to));
    END IF;
    IF f ? 'event_ids' THEN
        IF jsonb_typeof(f -> 'event_ids') <> 'array' THEN
            RETURN NULL;
        END IF;
        IF jsonb_array_length(f -> 'event_ids') NOT BETWEEN 1 AND 100
           OR EXISTS (SELECT 1 FROM jsonb_array_elements(f -> 'event_ids') AS t
                      WHERE jsonb_typeof(t) <> 'string') THEN
            RETURN NULL;
        END IF;
        IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(f -> 'event_ids') AS t
                   WHERE NOT audit_store.is_uuid_text(t)) THEN
            RETURN NULL;
        END IF;
        SELECT array_agg(DISTINCT t ORDER BY t) INTO v_ids
        FROM jsonb_array_elements_text(f -> 'event_ids') AS t;
        v_out := v_out || jsonb_build_object('event_ids', to_jsonb(v_ids));
    END IF;
    IF f ? 'seq_after' THEN
        IF coalesce(audit_store.jint(f -> 'seq_after') < 0, TRUE) THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('seq_after', audit_store.jint(f -> 'seq_after'));
    END IF;
    IF f ? 'seq_through' THEN
        IF coalesce(audit_store.jint(f -> 'seq_through') < 1, TRUE)
           OR audit_store.jint(f -> 'seq_through') > p_watermark
           OR audit_store.jint(f -> 'seq_through')
              <= coalesce(audit_store.jint(v_out -> 'seq_after'), 0) THEN
            RETURN NULL;
        END IF;
        v_out := v_out || jsonb_build_object('seq_through', audit_store.jint(f -> 'seq_through'));
    END IF;
    RETURN v_out;
END
$normalize_filter$;

-- Rebuilds the normalized filter from the chained intent details (the
-- filter_* members), so read_page never trusts the access_intents copy.
CREATE FUNCTION audit_store.filter_from_details(d JSONB)
RETURNS JSONB LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $filter_from_details$
    SELECT audit_store.opt('event_types', d -> 'filter_event_types')
        || audit_store.opt('source', d -> 'filter_source')
        || CASE WHEN d ? 'filter_actor_issuer' OR d ? 'filter_actor_principal_id'
                THEN jsonb_build_object('actor', jsonb_build_object(
                         'issuer', d -> 'filter_actor_issuer',
                         'principal_id', d -> 'filter_actor_principal_id'))
                ELSE '{}'::jsonb END
        || CASE WHEN d ? 'filter_resource_type' OR d ? 'filter_resource_id'
                THEN jsonb_build_object('resource',
                         jsonb_build_object('type', d -> 'filter_resource_type')
                         || audit_store.opt('id', d -> 'filter_resource_id'))
                ELSE '{}'::jsonb END
        || audit_store.opt('occurred_from', d -> 'filter_occurred_from')
        || audit_store.opt('occurred_to', d -> 'filter_occurred_to')
        || audit_store.opt('event_ids', d -> 'filter_event_ids')
        || audit_store.opt('seq_after', d -> 'filter_seq_after')
        || audit_store.opt('seq_through', d -> 'filter_seq_through')
$filter_from_details$;

CREATE FUNCTION audit_store.open_access(
    p_operation TEXT, p_filter JSONB, p_page_size INTEGER, p_max_pages INTEGER)
RETURNS TABLE (status TEXT, token TEXT, intent_seq BIGINT, watermark BIGINT,
               expires_at TIMESTAMPTZ, include_control BOOLEAN, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $open_access$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    v_role TEXT;
    v_capability TEXT;
    v_filter JSONB;
    v_max_page_size INTEGER;
    v_filter_digest BYTEA;
    v_include_control BOOLEAN;
    v_raw BYTEA;
    v_token_digest BYTEA;
    v_expires TIMESTAMPTZ;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    IF p_operation IN ('investigate', 'export') THEN
        v_role := 'audit_store_reader';
        v_capability := p_operation;
    ELSIF p_operation IN ('verify', 'identity_chain') THEN
        v_role := 'audit_store_verifier';
        v_capability := 'verify';
    ELSE
        PERFORM audit_store.record_denied('investigate', 'invalid_input', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::text, NULL::bigint, NULL::bigint,
                            NULL::timestamptz, NULL::boolean, 'invalid_input'::text;
        RETURN;
    END IF;
    -- After a recovery epoch, content disclosure stays closed until access
    -- and retention were re-applied (design §11): investigate, export and
    -- verify (which returns every body). Only the content-free identity
    -- chain stays open.
    IF p_operation IN ('investigate', 'export', 'verify') AND h.access_reapply_pending THEN
        RAISE EXCEPTION 'access_reapply_pending' USING ERRCODE = '55000';
    END IF;
    SELECT * INTO v_auth FROM audit_store.authorize(p_operation, v_role, v_capability);
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::text, NULL::bigint, NULL::bigint,
                            NULL::timestamptz, NULL::boolean, v_auth.denial;
        RETURN;
    END IF;
    v_filter := audit_store.normalize_filter(p_filter, p_operation, h.last_seq);
    v_max_page_size := (CASE WHEN p_operation = 'investigate' THEN 100 ELSE 1000 END);
    IF v_filter IS NULL
       OR p_page_size IS NULL
       OR p_page_size NOT BETWEEN 1 AND v_max_page_size
       OR p_max_pages IS NULL OR p_max_pages NOT BETWEEN 1 AND 100 THEN
        PERFORM audit_store.record_denied(p_operation, 'invalid_input', v_capability);
        RETURN QUERY SELECT 'denied'::text, NULL::text, NULL::bigint, NULL::bigint,
                            NULL::timestamptz, NULL::boolean, 'invalid_input'::text;
        RETURN;
    END IF;
    v_filter_digest := audit_store.jsonb_digest(v_filter);
    v_include_control := p_operation IN ('verify', 'identity_chain')
        OR audit_store.has_grant(v_auth.issuer, v_auth.principal_id, 'administer');
    -- Every denied attempt is in the chain before a disclosure intent.
    PERFORM audit_store.flush_denial_streaks(TRUE);
    v_raw := uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid());
    v_token_digest := sha256(v_raw);
    v_expires := date_trunc('milliseconds', transaction_timestamp()) + interval '10 minutes';
    v_seq := audit_store.append_control('store', 'audit.access.intent_opened', 'DATA_ACCESS',
        'success',
        jsonb_build_object(
            'operation', p_operation,
            'filter_digest', encode(v_filter_digest, 'hex'),
            'watermark', h.last_seq,
            'page_size', p_page_size,
            'max_pages', p_max_pages,
            'include_control', v_include_control,
            'expires_at', audit_store.utc_text(v_expires),
            'token_digest', encode(v_token_digest, 'hex'))
        || audit_store.opt('filter_event_types', v_filter -> 'event_types')
        || audit_store.opt('filter_source', v_filter -> 'source')
        || audit_store.opt('filter_actor_issuer', v_filter -> 'actor' -> 'issuer')
        || audit_store.opt('filter_actor_principal_id', v_filter -> 'actor' -> 'principal_id')
        || audit_store.opt('filter_resource_type', v_filter -> 'resource' -> 'type')
        || audit_store.opt('filter_resource_id', v_filter -> 'resource' -> 'id')
        || audit_store.opt('filter_occurred_from', v_filter -> 'occurred_from')
        || audit_store.opt('filter_occurred_to', v_filter -> 'occurred_to')
        || audit_store.opt('filter_event_ids', v_filter -> 'event_ids')
        || audit_store.opt('filter_seq_after', v_filter -> 'seq_after')
        || audit_store.opt('filter_seq_through', v_filter -> 'seq_through'));
    INSERT INTO audit_store.access_intents (
        intent_seq, token_digest, session_role, issuer, principal_id, operation, filter,
        filter_digest, watermark, page_size, max_pages, include_control, expires_at)
    VALUES (
        v_seq, v_token_digest, session_user::text, v_auth.issuer, v_auth.principal_id,
        p_operation, v_filter, v_filter_digest, h.last_seq, p_page_size, p_max_pages,
        v_include_control, v_expires);
    RETURN QUERY SELECT 'opened'::text, encode(v_raw, 'hex'), v_seq, h.last_seq, v_expires,
                        v_include_control, NULL::text;
END
$open_access$;

-- Resolves a token to its committed, intact, still-authorized intent or
-- raises (design §10.3). Never writes. Every disclosure parameter is taken
-- from the chained audit.access.intent_opened body and must equal the
-- access_intents copy.
CREATE FUNCTION audit_store.resolve_intent(
    p_token TEXT,
    OUT intent_seq BIGINT, OUT operation TEXT, OUT filter JSONB, OUT watermark BIGINT,
    OUT page_size INTEGER, OUT max_pages INTEGER, OUT include_control BOOLEAN)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $resolve_intent$
DECLARE
    ai audit_store.access_intents;
    e audit_store.events;
    v_body JSONB;
    d JSONB;
    v_filter JSONB;
    v_actor record;
    v_role TEXT;
    v_capability TEXT;
BEGIN
    IF p_token IS NULL OR p_token !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'unknown_token' USING ERRCODE = '42501';
    END IF;
    SELECT * INTO ai FROM audit_store.access_intents AS i
    WHERE i.token_digest = sha256(decode(p_token, 'hex'));
    IF NOT FOUND THEN
        RAISE EXCEPTION 'unknown_token' USING ERRCODE = '42501';
    END IF;
    IF pg_xact_status(ai.creating_xid) IS DISTINCT FROM 'committed' THEN
        RAISE EXCEPTION 'intent_not_committed' USING ERRCODE = '42501';
    END IF;
    IF ai.session_role <> session_user::text THEN
        RAISE EXCEPTION 'token_session_mismatch' USING ERRCODE = '42501';
    END IF;
    IF ai.expires_at <= clock_timestamp() THEN
        RAISE EXCEPTION 'token_expired' USING ERRCODE = '42501';
    END IF;
    -- The intent row must equal the intent event in the chain.
    SELECT * INTO e FROM audit_store.events AS x WHERE x.seq = ai.intent_seq;
    SELECT b.envelope INTO v_body FROM audit_store.event_bodies AS b WHERE b.seq = ai.intent_seq;
    d := v_body -> 'data' -> 'details';
    v_filter := audit_store.filter_from_details(d);
    IF e.seq IS NULL OR v_body IS NULL
       OR e.origin <> 'store' OR e.event_type <> 'audit.access.intent_opened'
       OR audit_store.jsonb_digest(v_body) <> e.envelope_digest
       OR audit_store.chain_step(e.prev_chain, e.seq, e.event_id, e.envelope_digest) <> e.chain
       OR v_filter IS DISTINCT FROM ai.filter
       OR audit_store.jsonb_digest(v_filter) <> ai.filter_digest
       OR d ->> 'filter_digest' IS DISTINCT FROM encode(ai.filter_digest, 'hex')
       OR d ->> 'token_digest' IS DISTINCT FROM encode(ai.token_digest, 'hex')
       OR d ->> 'session_role' IS DISTINCT FROM ai.session_role
       OR d ->> 'operation' IS DISTINCT FROM ai.operation
       OR audit_store.jint(d -> 'watermark') IS DISTINCT FROM ai.watermark
       OR audit_store.jint(d -> 'page_size') IS DISTINCT FROM ai.page_size::bigint
       OR audit_store.jint(d -> 'max_pages') IS DISTINCT FROM ai.max_pages::bigint
       OR d -> 'include_control' IS DISTINCT FROM to_jsonb(ai.include_control)
       OR d ->> 'expires_at' IS DISTINCT FROM audit_store.utc_text(ai.expires_at)
       OR e.actor_issuer <> ai.issuer OR e.actor_principal_id <> ai.principal_id
       OR audit_store.jint(d -> 'watermark') >= e.seq
       OR audit_store.jint(d -> 'page_size') NOT BETWEEN 1 AND 1000
       OR audit_store.jint(d -> 'max_pages') NOT BETWEEN 1 AND 100 THEN
        RAISE EXCEPTION 'intent_integrity_violation' USING ERRCODE = '42501';
    END IF;
    intent_seq := e.seq;
    operation := d ->> 'operation';
    filter := v_filter;
    watermark := audit_store.jint(d -> 'watermark');
    page_size := audit_store.jint(d -> 'page_size')::integer;
    max_pages := audit_store.jint(d -> 'max_pages')::integer;
    include_control := (d -> 'include_control') = 'true'::jsonb;
    v_role := CASE WHEN operation IN ('investigate', 'export') THEN 'audit_store_reader'
                   ELSE 'audit_store_verifier' END;
    v_capability := CASE WHEN operation IN ('investigate', 'export') THEN operation
                         ELSE 'verify' END;
    SELECT * INTO v_actor FROM audit_store.session_actor();
    -- Revocation applies to open tokens, including the `administer` that
    -- made control events visible to an investigate/export intent.
    IF NOT v_actor.bound
       OR v_actor.issuer <> ai.issuer OR v_actor.principal_id <> ai.principal_id
       OR NOT pg_has_role(session_user, v_role, 'MEMBER')
       OR NOT audit_store.has_grant(ai.issuer, ai.principal_id, v_capability)
       OR (include_control AND operation IN ('investigate', 'export')
           AND NOT audit_store.has_grant(ai.issuer, ai.principal_id, 'administer')) THEN
        RAISE EXCEPTION 'access_revoked' USING ERRCODE = '42501';
    END IF;
END
$resolve_intent$;

-- Waits until all WAL inserted so far is flushed (design §10.3): X is taken
-- once after the committed check, so the intent's commit record (inserted
-- before its clog update) is at or below X. Bounded (~2 s); then raises the
-- retryable intent_not_durable (SQLSTATE 40001) and discloses nothing.
CREATE FUNCTION audit_store.await_durable()
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $await_durable$
DECLARE
    v_target pg_lsn := pg_current_wal_insert_lsn();
    v_deadline TIMESTAMPTZ := clock_timestamp() + interval '2 seconds';
BEGIN
    LOOP
        EXIT WHEN pg_current_wal_flush_lsn() >= v_target;
        IF clock_timestamp() >= v_deadline THEN
            RAISE EXCEPTION 'intent_not_durable' USING ERRCODE = '40001';
        END IF;
        PERFORM pg_sleep(0.01);
    END LOOP;
END
$await_durable$;

-- Returns at most page_size export lines of the bounded set
-- (filter ∩ seq_after < seq <= min(seq_through, W) ∩ visibility, the first
-- max_pages * page_size identity rows by seq, expired rows included).
-- Read only; refusals are errors and are not recorded (they disclose nothing).
CREATE FUNCTION audit_store.read_page(p_token TEXT, p_after_seq BIGINT)
RETURNS TABLE (seq BIGINT, line TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $read_page$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    ai record;
    v_types TEXT[];
    v_source TEXT;
    v_actor_issuer TEXT;
    v_actor_principal TEXT;
    v_resource_type TEXT;
    v_resource_id TEXT;
    v_from TIMESTAMPTZ;
    v_to TIMESTAMPTZ;
    v_ids UUID[];
    v_seq_after BIGINT;
    v_through BIGINT;
    v_bodies BOOLEAN;
BEGIN
    -- The current transaction must not have written anything, even inside a
    -- savepoint that was rolled back or released.
    IF pg_current_xact_id_if_assigned() IS NOT NULL THEN
        RAISE EXCEPTION 'read_page_requires_clean_transaction' USING ERRCODE = '42501';
    END IF;
    h := audit_store.read_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO ai FROM audit_store.resolve_intent(p_token);
    IF ai.operation <> 'identity_chain' AND h.access_reapply_pending THEN
        RAISE EXCEPTION 'access_reapply_pending' USING ERRCODE = '55000';
    END IF;
    PERFORM audit_store.await_durable();
    IF ai.filter ? 'event_types' THEN
        v_types := ARRAY(SELECT jsonb_array_elements_text(ai.filter -> 'event_types'));
    END IF;
    v_source := ai.filter ->> 'source';
    v_actor_issuer := ai.filter -> 'actor' ->> 'issuer';
    v_actor_principal := ai.filter -> 'actor' ->> 'principal_id';
    v_resource_type := ai.filter -> 'resource' ->> 'type';
    v_resource_id := ai.filter -> 'resource' ->> 'id';
    v_from := audit_store.jts(ai.filter -> 'occurred_from');
    v_to := audit_store.jts(ai.filter -> 'occurred_to');
    IF ai.filter ? 'event_ids' THEN
        v_ids := ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(ai.filter -> 'event_ids') AS x);
    END IF;
    v_seq_after := coalesce(audit_store.jint(ai.filter -> 'seq_after'), 0);
    v_through := least(coalesce(audit_store.jint(ai.filter -> 'seq_through'), ai.watermark),
                       ai.watermark);
    v_bodies := ai.operation <> 'identity_chain';
    RETURN QUERY
    WITH bound AS (
        SELECT e.seq FROM audit_store.events AS e
        WHERE e.seq > v_seq_after AND e.seq <= v_through
          AND (ai.include_control OR e.origin = 'relay')
          AND (v_types IS NULL OR e.event_type = ANY (v_types))
          AND (v_source IS NULL OR e.source = v_source)
          AND (v_actor_issuer IS NULL
               OR (e.actor_issuer = v_actor_issuer AND e.actor_principal_id = v_actor_principal))
          AND (v_resource_type IS NULL OR e.resource_type = v_resource_type)
          AND (v_resource_id IS NULL OR e.resource_id = v_resource_id)
          AND (v_from IS NULL OR e.occurred_at >= v_from)
          AND (v_to IS NULL OR e.occurred_at < v_to)
          AND (v_ids IS NULL OR e.event_id = ANY (v_ids))
        ORDER BY e.seq
        LIMIT ai.page_size::bigint * ai.max_pages::bigint
    )
    SELECT e.seq,
           audit_store.export_line(e, CASE WHEN v_bodies THEN b.envelope END)
    FROM bound
    JOIN audit_store.events AS e ON e.seq = bound.seq
    LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq
    WHERE e.seq > coalesce(p_after_seq, 0)
    ORDER BY e.seq
    LIMIT ai.page_size;
END
$read_page$;

-- Records what the client received (optional, design §10.3).
CREATE FUNCTION audit_store.close_access(
    p_token TEXT, p_returned_count BIGINT, p_page_digests TEXT[])
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $close_access$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    ai audit_store.access_intents;
    v_actor record;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_actor FROM audit_store.session_actor();
    IF NOT v_actor.bound THEN
        PERFORM audit_store.record_denied('close_access', 'unbound', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'unbound'::text;
        RETURN;
    END IF;
    IF p_token ~ '^[0-9a-f]{64}$' THEN
        SELECT * INTO ai FROM audit_store.access_intents AS i
        WHERE i.token_digest = sha256(decode(p_token, 'hex'))
          AND i.session_role = session_user::text
          AND i.issuer = v_actor.issuer AND i.principal_id = v_actor.principal_id;
    END IF;
    IF ai.intent_seq IS NULL
       OR p_returned_count IS NULL OR p_returned_count < 0
       OR p_returned_count > ai.page_size::bigint * ai.max_pages::bigint
       OR coalesce(cardinality(p_page_digests), 0) > ai.max_pages
       OR EXISTS (SELECT 1 FROM unnest(p_page_digests) AS x
                  WHERE x IS NULL OR x !~ '^[0-9a-f]{64}$') THEN
        PERFORM audit_store.record_denied('close_access', 'invalid_input', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access.closed', 'DATA_ACCESS', 'success',
        jsonb_build_object(
            'intent_seq', ai.intent_seq,
            'returned_count', p_returned_count,
            'page_count', coalesce(cardinality(p_page_digests), 0),
            'page_digests_digest', encode(sha256(convert_to(
                array_to_string(coalesce(p_page_digests, ARRAY[]::text[]), ','), 'UTF8')), 'hex')));
    RETURN QUERY SELECT 'closed'::text, v_seq, NULL::text;
END
$close_access$;

-- ---------------------------------------------------------------------------
-- Integrity (design §8)
-- ---------------------------------------------------------------------------

-- Scans seq p_from..p_to in ONE snapshot (STABLE: every query here uses the
-- calling statement's snapshot), where W is the head as seen by that
-- snapshot. Publication is in commit order, so the set seq <= W is stable.
-- p_head_check compares the head row when the range ends at W. p_to NULL
-- means W; verify refuses a p_to past W; for recovery scans the caller
-- passes max(seq). head_seq/head_epoch/head_chain describe the last row
-- that exists in the range (the row before the range when none does), so a
-- missing range end is reported as a gap, never as a chain it does not have.
CREATE FUNCTION audit_store.integrity_scan(p_from BIGINT, p_to BIGINT, p_head_check BOOLEAN)
RETURNS TABLE (from_seq BIGINT, to_seq BIGINT, watermark BIGINT, checked BIGINT,
               head_seq BIGINT, head_epoch BIGINT, head_chain BYTEA, violations JSONB)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $integrity_scan$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_from BIGINT := coalesce(p_from, 1);
    v_to BIGINT;
    v_base_chain BYTEA;
    v_base_epoch BIGINT;
    v_checked BIGINT;
    v_max BIGINT;
    v_head_seq BIGINT;
    v_head_chain BYTEA;
    v_head_epoch BIGINT;
    v JSONB;
    v_missing BIGINT;
    v_retention_missing BIGINT;
    v_retention_mismatch BIGINT;
    v_purge_mismatch BIGINT;
    v_head_mismatch BIGINT := 0;
BEGIN
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
    v_to := coalesce(p_to, h.last_seq);
    IF v_from < 1 OR v_to < v_from - 1 THEN
        RAISE EXCEPTION 'integrity_scan: invalid range' USING ERRCODE = '22023';
    END IF;
    IF v_from = 1 THEN
        v_base_chain := audit_store.genesis();
        v_base_epoch := 1;
    ELSE
        SELECT e.chain, e.recovery_epoch INTO v_base_chain, v_base_epoch
        FROM audit_store.events AS e WHERE e.seq = v_from - 1;
    END IF;

    WITH r AS (
        SELECT e.*, b.envelope,
               lag(e.seq) OVER w AS lag_seq,
               lag(e.chain) OVER w AS lag_chain,
               lag(e.recovery_epoch) OVER w AS lag_epoch
        FROM audit_store.events AS e
        LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.seq BETWEEN v_from AND v_to
        WINDOW w AS (ORDER BY e.seq)
    ),
    c AS (
        SELECT
            r.seq,
            r.chain,
            r.recovery_epoch,
            r.seq <> coalesce(r.lag_seq + 1, v_from) AS seq_gap,
            r.prev_chain IS DISTINCT FROM coalesce(r.lag_chain, v_base_chain)
                AS prev_chain_mismatch,
            r.chain IS DISTINCT FROM audit_store.chain_step(
                r.prev_chain, r.seq, r.event_id, r.envelope_digest) AS chain_mismatch,
            r.envelope IS NOT NULL
                AND audit_store.jsonb_digest(r.envelope) IS DISTINCT FROM r.envelope_digest
                AS body_digest_mismatch,
            r.envelope IS NOT NULL AND NOT coalesce(
                r.envelope ->> 'source' = r.source
                AND r.envelope ->> 'type' = r.event_type
                AND r.envelope ->> 'subject' = r.subject
                AND audit_store.jts(r.envelope -> 'time') = r.occurred_at
                AND r.envelope -> 'data' ->> 'event_class' = r.event_class
                AND r.envelope -> 'data' -> 'actor' ->> 'issuer' = r.actor_issuer
                AND r.envelope -> 'data' -> 'actor' ->> 'principal_id' = r.actor_principal_id
                AND r.envelope -> 'data' -> 'resource' ->> 'type' = r.resource_type
                AND r.envelope -> 'data' -> 'resource' ->> 'id' = r.resource_id
                AND (r.envelope -> 'data' -> 'resource' ->> 'version_id')
                    IS NOT DISTINCT FROM r.resource_version_id::text
                AND r.envelope -> 'data' ->> 'result' = r.result
                AND audit_store.jint(r.envelope -> 'data' -> 'provenance' -> 'adapter_version')
                    = r.adapter_version
                AND (r.envelope -> 'data' -> 'provenance' ->> 'source_commitment')
                    IS NOT DISTINCT FROM encode(r.source_commitment, 'hex')
                AND CASE r.origin
                        WHEN 'store' THEN r.source = 'urn:knowledge-platform:audit-store'
                        WHEN 'relay_control' THEN r.source = 'urn:knowledge-platform:audit-relay'
                        ELSE r.source NOT IN ('urn:knowledge-platform:audit-store',
                                              'urn:knowledge-platform:audit-relay')
                    END, FALSE) AS identity_mismatch,
            r.envelope IS NOT NULL AND (r.envelope ->> 'id') IS DISTINCT FROM r.event_id::text
                AS event_id_mismatch,
            ((r.expired_at IS NULL) <> (r.envelope IS NOT NULL))
                OR ((r.expired_at IS NULL) <> (r.expired_by_seq IS NULL))
                AS expiry_state_mismatch,
            r.recovery_epoch < coalesce(r.lag_epoch, v_base_epoch) AS epoch_regressed
        FROM r
    )
    SELECT count(*),
           max(c.seq),
           jsonb_build_object(
               'seq_gap', count(*) FILTER (WHERE c.seq_gap),
               'prev_chain_mismatch', count(*) FILTER (WHERE c.prev_chain_mismatch),
               'chain_mismatch', count(*) FILTER (WHERE c.chain_mismatch),
               'body_digest_mismatch', count(*) FILTER (WHERE c.body_digest_mismatch),
               'identity_mismatch', count(*) FILTER (WHERE c.identity_mismatch),
               'event_id_mismatch', count(*) FILTER (WHERE c.event_id_mismatch),
               'expiry_state_mismatch', count(*) FILTER (WHERE c.expiry_state_mismatch),
               'epoch_regressed', count(*) FILTER (WHERE c.epoch_regressed))
    INTO v_checked, v_max, v
    FROM c;

    -- Rows missing after the last present row (a truncated range end).
    v_missing := CASE WHEN v_to < v_from THEN 0
                      WHEN v_max IS NULL THEN v_to - v_from + 1
                      ELSE v_to - v_max END;
    v := jsonb_set(v, '{seq_gap}', to_jsonb(audit_store.jint(v -> 'seq_gap') + v_missing));

    IF v_max IS NOT NULL THEN
        SELECT e.chain, e.recovery_epoch INTO v_head_chain, v_head_epoch
        FROM audit_store.events AS e WHERE e.seq = v_max;
        v_head_seq := v_max;
    ELSE
        v_head_seq := v_from - 1;
        v_head_chain := v_base_chain;
        v_head_epoch := v_base_epoch;
    END IF;

    -- Retention evidence (set semantics). Marks in range point to a later
    -- origin=store event whose body must exist.
    SELECT count(*) INTO v_retention_missing
    FROM audit_store.events AS t
    LEFT JOIN audit_store.events AS ev ON ev.seq = t.expired_by_seq
    LEFT JOIN audit_store.event_bodies AS evb ON evb.seq = t.expired_by_seq
    WHERE t.seq BETWEEN v_from AND v_to
      AND t.expired_by_seq IS NOT NULL
      AND (ev.seq IS NULL OR evb.seq IS NULL OR ev.origin <> 'store' OR ev.seq <= t.seq
           OR ev.event_type NOT IN ('audit.retention.expired', 'audit.body.purged'));

    -- Evidence events referenced from the range or located in it.
    WITH evidence AS (
        SELECT DISTINCT t.expired_by_seq AS seq FROM audit_store.events AS t
        WHERE t.seq BETWEEN v_from AND v_to AND t.expired_by_seq IS NOT NULL
        UNION
        SELECT e.seq FROM audit_store.events AS e
        WHERE e.seq BETWEEN v_from AND v_to AND e.origin = 'store'
          AND e.event_type IN ('audit.retention.expired', 'audit.body.purged')
    ),
    ev AS (
        SELECT e.seq, e.event_type, b.envelope -> 'data' -> 'details' AS d
        FROM evidence
        JOIN audit_store.events AS e ON e.seq = evidence.seq
        JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.origin = 'store'
          AND e.event_type IN ('audit.retention.expired', 'audit.body.purged')
    ),
    -- Every row marked by an evidence event (whole table), with the
    -- retention predicate recorded in that event.
    marks AS (
        SELECT ev.seq AS evidence_seq, t.seq, t.event_id,
               coalesce(
                   t.origin = 'relay' AND t.seq < ev.seq
                   AND t.occurred_at < audit_store.jts(ev.d -> 'effective_cutoff')
                   AND (jsonb_typeof(ev.d -> 'selector_event_types') IS DISTINCT FROM 'array'
                        OR (ev.d -> 'selector_event_types') ? t.event_type)
                   AND (jsonb_typeof(ev.d -> 'selector_event_classes') IS DISTINCT FROM 'array'
                        OR (ev.d -> 'selector_event_classes') ? t.event_class)
                   AND (jsonb_typeof(ev.d -> 'selector_sources') IS DISTINCT FROM 'array'
                        OR (ev.d -> 'selector_sources') ? t.source),
                   FALSE) AS predicate_ok
        FROM ev
        JOIN audit_store.events AS t ON t.expired_by_seq = ev.seq
    ),
    sets AS (
        SELECT ev.seq, ev.event_type, ev.d,
               count(m.seq) AS n,
               audit_store.expired_set_digest(
                   coalesce(array_agg(m.seq) FILTER (WHERE m.seq IS NOT NULL),
                            ARRAY[]::bigint[])) AS set_digest,
               min(m.seq) AS first_seq,
               max(m.seq) AS last_seq,
               min(m.event_id::text) AS only_event_id,
               coalesce(bool_and(m.predicate_ok), TRUE) AS rows_match
        FROM ev
        LEFT JOIN marks AS m ON m.evidence_seq = ev.seq
        GROUP BY ev.seq, ev.event_type, ev.d
    )
    SELECT
        count(*) FILTER (WHERE s.event_type = 'audit.retention.expired' AND NOT (
            audit_store.jint(s.d -> 'count') IS NOT DISTINCT FROM s.n
            AND s.d ->> 'expired_set_digest' IS NOT DISTINCT FROM encode(s.set_digest, 'hex')
            AND audit_store.jint(s.d -> 'first_seq') IS NOT DISTINCT FROM s.first_seq
            AND audit_store.jint(s.d -> 'last_seq') IS NOT DISTINCT FROM s.last_seq
            AND s.rows_match)),
        count(*) FILTER (WHERE s.event_type = 'audit.body.purged' AND NOT (
            s.n = 1
            AND audit_store.jint(s.d -> 'target_seq') IS NOT DISTINCT FROM s.first_seq
            AND s.d ->> 'target_event_id' IS NOT DISTINCT FROM s.only_event_id))
    INTO v_retention_mismatch, v_purge_mismatch
    FROM sets AS s;

    IF p_head_check AND v_to = h.last_seq THEN
        IF h.last_chain IS DISTINCT FROM v_head_chain
           OR EXISTS (SELECT 1 FROM audit_store.events AS e WHERE e.seq > v_to) THEN
            v_head_mismatch := 1;
        END IF;
    END IF;

    v := v || jsonb_build_object(
        'retention_evidence_missing', v_retention_missing,
        'retention_evidence_mismatch', v_retention_mismatch,
        'purge_evidence_mismatch', v_purge_mismatch,
        'head_mismatch', v_head_mismatch);
    RETURN QUERY SELECT v_from, v_to, h.last_seq, v_checked, v_head_seq,
                        coalesce(v_head_epoch, v_base_epoch, 1::bigint),
                        coalesce(v_head_chain, v_base_chain, audit_store.genesis()), v;
END
$integrity_scan$;

-- Sum of violation counters.
CREATE FUNCTION audit_store.violations_total(p_violations JSONB)
RETURNS BIGINT LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $violations_total$
    SELECT coalesce(sum(audit_store.jint(v.value)), 0)::bigint
    FROM jsonb_each(p_violations) AS v
$violations_total$;

-- Records audit.integrity.verified for a scan result. Takes the head lock
-- only now (verify/checkpoint scan without it) and re-checks the gate under
-- the lock.
CREATE FUNCTION audit_store.record_verified(
    p_trigger TEXT, p_from BIGINT, p_to BIGINT, p_watermark BIGINT, p_checked BIGINT,
    p_head_seq BIGINT, p_head_epoch BIGINT, p_head_chain BYTEA, p_violations JSONB)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $record_verified$
DECLARE
    h audit_store.publication_head;
    v_total BIGINT := audit_store.violations_total(p_violations);
    v_details JSONB;
    k TEXT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    -- Every denied attempt is in the chain before integrity evidence.
    PERFORM audit_store.flush_denial_streaks(TRUE);
    -- The head (head_seq, head_epoch, head_chain) is the last scanned row
    -- that exists (integrity_scan).
    v_details := jsonb_build_object(
        'trigger', p_trigger,
        'from_seq', p_from,
        'to_seq', p_to,
        'watermark', p_watermark,
        'checked', p_checked,
        'head_seq', p_head_seq,
        'head_epoch', p_head_epoch,
        'head_chain', encode(p_head_chain, 'hex'),
        'outcome', CASE WHEN v_total = 0 THEN 'ok' ELSE 'violations' END,
        'violations_total', v_total);
    FOR k IN SELECT x FROM jsonb_object_keys(p_violations) AS x LOOP
        v_details := v_details || jsonb_build_object('violation_' || k,
                                                     audit_store.jint(p_violations -> k));
    END LOOP;
    RETURN audit_store.append_control('store', 'audit.integrity.verified', 'SYSTEM_AUDIT',
        CASE WHEN v_total = 0 THEN 'success' ELSE 'failure' END, v_details);
END
$record_verified$;

-- In-database verification of p_from..p_to (bounded by the head W of the
-- scan snapshot and by a maximum span), recorded as one
-- audit.integrity.verified after W (no recursion). The scan runs without the
-- head lock; only the append takes it, so ingest is not blocked by the scan.
CREATE FUNCTION audit_store.verify(p_from BIGINT, p_to BIGINT)
RETURNS TABLE (status TEXT, seq BIGINT, outcome TEXT, checked BIGINT, violations JSONB,
               from_seq BIGINT, to_seq BIGINT, watermark BIGINT, head_epoch BIGINT,
               head_chain TEXT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $verify$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    s record;
    v_seq BIGINT;
BEGIN
    PERFORM pg_catalog.set_config('synchronous_commit', 'on', TRUE);
    h := audit_store.read_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('integrity_verify', 'audit_store_verifier',
                                                    'verify');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::text, NULL::bigint, NULL::jsonb,
                            NULL::bigint, NULL::bigint, NULL::bigint, NULL::bigint, NULL::text,
                            v_auth.denial;
        RETURN;
    END IF;
    -- The range must lie within the head W: rows past W do not exist yet,
    -- and a range that verifies nothing (from past W) is not a result. h is
    -- read before the scan's snapshot; outside recovery W only grows, so a
    -- bound that holds here holds for the scan.
    IF (p_from IS NOT NULL AND p_from < 1)
       OR (p_to IS NOT NULL AND p_to < coalesce(p_from, 1))
       OR (p_to IS NOT NULL AND p_to > h.last_seq)
       OR coalesce(p_from, 1) > greatest(h.last_seq, 1)
       OR coalesce(p_to, h.last_seq) - coalesce(p_from, 1) + 1 > 10000000 THEN
        PERFORM audit_store.record_denied('integrity_verify', 'invalid_input', 'verify');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::text, NULL::bigint, NULL::jsonb,
                            NULL::bigint, NULL::bigint, NULL::bigint, NULL::bigint, NULL::text,
                            'invalid_input'::text;
        RETURN;
    END IF;
    -- One statement, one snapshot: W, the range and the head check agree.
    SELECT * INTO s FROM audit_store.integrity_scan(coalesce(p_from, 1), p_to, TRUE);
    v_seq := audit_store.record_verified('verify', s.from_seq, s.to_seq, s.watermark, s.checked,
                                         s.head_seq, s.head_epoch, s.head_chain, s.violations);
    RETURN QUERY SELECT 'verified'::text, v_seq,
                        CASE WHEN audit_store.violations_total(s.violations) = 0 THEN 'ok'
                             ELSE 'violations' END,
                        s.checked, s.violations, s.from_seq, s.to_seq, s.watermark,
                        s.head_epoch, encode(s.head_chain, 'hex'), NULL::text;
END
$verify$;

-- Verifies 1..W without the head lock and records audit.integrity.verified
-- (trigger 'checkpoint'). Returns the chain position of that record (its
-- epoch, seq and chain) for the out-of-band checkpoint log: after a planned
-- move with nothing written later, the restored head equals the checkpoint.
CREATE FUNCTION audit_store.checkpoint()
RETURNS TABLE (status TEXT, epoch BIGINT, seq BIGINT, chain TEXT, verified_through BIGINT,
               outcome TEXT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $checkpoint$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    s record;
    v_seq BIGINT;
    v_ok BOOLEAN;
    r audit_store.events;
BEGIN
    PERFORM pg_catalog.set_config('synchronous_commit', 'on', TRUE);
    h := audit_store.read_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('checkpoint', 'audit_store_verifier',
                                                    'verify');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::text,
                            NULL::bigint, NULL::text, v_auth.denial;
        RETURN;
    END IF;
    SELECT * INTO s FROM audit_store.integrity_scan(1, NULL, TRUE);
    v_ok := audit_store.violations_total(s.violations) = 0;
    v_seq := audit_store.record_verified('checkpoint', s.from_seq, s.to_seq, s.watermark,
                                         s.checked, s.head_seq, s.head_epoch, s.head_chain,
                                         s.violations);
    SELECT * INTO STRICT r FROM audit_store.events AS x WHERE x.seq = v_seq;
    RETURN QUERY SELECT CASE WHEN v_ok THEN 'checkpoint' ELSE 'violations' END::text,
                        r.recovery_epoch, r.seq, encode(r.chain, 'hex'), s.to_seq,
                        CASE WHEN v_ok THEN 'ok' ELSE 'violations' END::text, NULL::text;
END
$checkpoint$;

-- Read-only verification for recovery mode only (design §11). Records
-- nothing; outside recovery mode it raises not_in_recovery.
CREATE FUNCTION audit_store.verify_recovery()
RETURNS TABLE (outcome TEXT, checked BIGINT, head_seq BIGINT, head_epoch BIGINT,
               head_chain TEXT, recovery_mode BOOLEAN, violations JSONB)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $verify_recovery$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head := audit_store.read_head();
    v_max BIGINT;
    s record;
BEGIN
    IF NOT audit_store.in_recovery(h) THEN
        RAISE EXCEPTION 'not_in_recovery' USING ERRCODE = '55000';
    END IF;
    SELECT coalesce(max(e.seq), 0) INTO v_max FROM audit_store.events AS e;
    SELECT * INTO s FROM audit_store.integrity_scan(1, v_max, TRUE);
    IF v_max <> h.last_seq THEN
        -- The restored head row disagrees with the restored events.
        s.violations := jsonb_set(s.violations, '{head_mismatch}', '1'::jsonb);
    END IF;
    RETURN QUERY SELECT CASE WHEN audit_store.violations_total(s.violations) = 0 THEN 'ok'
                             ELSE 'violations' END::text,
                        s.checked, v_max, s.head_epoch, encode(s.head_chain, 'hex'), TRUE,
                        s.violations;
END
$verify_recovery$;

-- Content-free identity chain page for recovery mode only; no intent, no
-- body, records nothing. Outside recovery mode it raises not_in_recovery.
CREATE FUNCTION audit_store.identity_chain_recovery_page(p_after_seq BIGINT, p_limit INTEGER)
RETURNS TABLE (seq BIGINT, line TEXT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $identity_chain_recovery_page$
#variable_conflict use_column
BEGIN
    IF NOT audit_store.in_recovery(audit_store.read_head()) THEN
        RAISE EXCEPTION 'not_in_recovery' USING ERRCODE = '55000';
    END IF;
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'identity_chain_recovery_page: limit must be 1..1000'
            USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.seq, audit_store.export_line(e, NULL)
    FROM audit_store.events AS e
    WHERE e.seq > coalesce(p_after_seq, 0)
    ORDER BY e.seq
    LIMIT p_limit;
END
$identity_chain_recovery_page$;

-- ---------------------------------------------------------------------------
-- Access administration (design §10.2)
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_store.change_access(
    p_issuer TEXT, p_principal_id TEXT, p_capability TEXT, p_action TEXT)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer'
SET audit_store.maintenance_context = 'access' AS $change_access$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    v_existing BIGINT;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('change_access', 'audit_store_admin',
                                                    'administer');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, v_auth.denial;
        RETURN;
    END IF;
    -- The db_role issuer names unbound sessions in control events; it is
    -- never a principal that holds capabilities (bind refuses it too).
    IF NOT audit_store.is_principal_part(p_issuer)
       OR NOT audit_store.is_principal_part(p_principal_id)
       OR p_issuer = 'db_role'
       OR p_capability IS NULL
       OR p_capability NOT IN ('investigate', 'export', 'verify', 'administer', 'maintain')
       OR p_action IS NULL OR p_action NOT IN ('grant', 'revoke') THEN
        PERFORM audit_store.record_denied('change_access', 'invalid_input', 'administer');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    IF p_action = 'grant' AND p_issuer = v_auth.issuer AND p_principal_id = v_auth.principal_id THEN
        PERFORM audit_store.record_denied('change_access', 'self_grant', 'administer');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'self_grant'::text;
        RETURN;
    END IF;
    SELECT g.granted_seq INTO v_existing FROM audit_store.access_grants AS g
    WHERE g.issuer = p_issuer AND g.principal_id = p_principal_id
      AND g.capability = p_capability AND g.revoked_seq IS NULL;
    IF (p_action = 'grant') = (v_existing IS NOT NULL) THEN
        RETURN QUERY SELECT 'unchanged'::text, NULL::bigint, NULL::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object(
            'change', CASE WHEN p_action = 'grant' THEN 'granted' ELSE 'revoked' END,
            'target_issuer', p_issuer,
            'target_principal_id', p_principal_id,
            'capability', p_capability));
    IF p_action = 'grant' THEN
        INSERT INTO audit_store.access_grants (granted_seq, issuer, principal_id, capability)
        VALUES (v_seq, p_issuer, p_principal_id, p_capability);
    ELSE
        UPDATE audit_store.access_grants AS g SET revoked_seq = v_seq
        WHERE g.granted_seq = v_existing;
    END IF;
    RETURN QUERY SELECT CASE WHEN p_action = 'grant' THEN 'granted' ELSE 'revoked' END::text,
                        v_seq, NULL::text;
END
$change_access$;

-- Owner-membership check for bind/unbind/bootstrap. session_user, never
-- current_user (which is the owner inside definer functions).
CREATE FUNCTION audit_store.require_owner_member()
RETURNS VOID LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $require_owner_member$
BEGIN
    IF NOT pg_has_role(session_user, 'audit_store_owner', 'MEMBER') THEN
        RAISE EXCEPTION 'owner_membership_required' USING ERRCODE = '42501';
    END IF;
END
$require_owner_member$;

-- A login role that may be bound: exists, can log in, and is neither a
-- superuser nor a member of the owner role.
CREATE FUNCTION audit_store.bindable_role(p_db_role TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $bindable_role$
    SELECT coalesce((
        SELECT r.rolcanlogin AND NOT r.rolsuper
               AND NOT pg_has_role(r.oid, 'audit_store_owner', 'MEMBER')
               AND audit_store.is_db_role(r.rolname::text)
        FROM pg_roles AS r WHERE r.rolname = p_db_role), FALSE)
$bindable_role$;

CREATE FUNCTION audit_store.bind_principal(p_db_role TEXT, p_issuer TEXT, p_principal_id TEXT)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $bind_principal$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_owner_member();
    PERFORM audit_store.require_not_recovery(h);
    IF NOT audit_store.bindable_role(p_db_role)
       OR NOT audit_store.is_principal_part(p_issuer)
       OR NOT audit_store.is_principal_part(p_principal_id)
       OR p_issuer = 'db_role' THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.principal_bindings AS b
               WHERE b.db_role = p_db_role AND b.unbound_seq IS NULL) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'already_bound'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object('change', 'bound', 'target_issuer', p_issuer,
                                      'target_principal_id', p_principal_id,
                                      'db_role', p_db_role));
    INSERT INTO audit_store.principal_bindings (bound_seq, db_role, issuer, principal_id)
    VALUES (v_seq, p_db_role, p_issuer, p_principal_id);
    RETURN QUERY SELECT 'bound'::text, v_seq, NULL::text;
END
$bind_principal$;

CREATE FUNCTION audit_store.unbind_principal(p_db_role TEXT)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer'
SET audit_store.maintenance_context = 'access' AS $unbind_principal$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    b audit_store.principal_bindings;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_owner_member();
    PERFORM audit_store.require_not_recovery(h);
    SELECT * INTO b FROM audit_store.principal_bindings AS x
    WHERE x.db_role = p_db_role AND x.unbound_seq IS NULL;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'not_bound'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object('change', 'unbound', 'target_issuer', b.issuer,
                                      'target_principal_id', b.principal_id,
                                      'db_role', b.db_role));
    UPDATE audit_store.principal_bindings AS x SET unbound_seq = v_seq
    WHERE x.bound_seq = b.bound_seq;
    RETURN QUERY SELECT 'unbound'::text, v_seq, NULL::text;
END
$unbind_principal$;

-- First administrator, or audited lockout recovery when none remains.
CREATE FUNCTION audit_store.bootstrap_administrator(
    p_db_role TEXT, p_issuer TEXT, p_principal_id TEXT)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $bootstrap_administrator$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    b audit_store.principal_bindings;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_owner_member();
    PERFORM audit_store.require_not_recovery(h);
    IF EXISTS (SELECT 1 FROM audit_store.access_grants AS g
               WHERE g.capability = 'administer' AND g.revoked_seq IS NULL) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'administrator_exists'::text;
        RETURN;
    END IF;
    IF NOT audit_store.bindable_role(p_db_role)
       OR NOT audit_store.is_principal_part(p_issuer)
       OR NOT audit_store.is_principal_part(p_principal_id)
       OR p_issuer = 'db_role' THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    SELECT * INTO b FROM audit_store.principal_bindings AS x
    WHERE x.db_role = p_db_role AND x.unbound_seq IS NULL;
    IF FOUND AND (b.issuer <> p_issuer OR b.principal_id <> p_principal_id) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'already_bound'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object('change', 'bootstrap', 'target_issuer', p_issuer,
                                      'target_principal_id', p_principal_id,
                                      'capability', 'administer', 'db_role', p_db_role));
    IF b.bound_seq IS NULL THEN
        INSERT INTO audit_store.principal_bindings (bound_seq, db_role, issuer, principal_id)
        VALUES (v_seq, p_db_role, p_issuer, p_principal_id);
    END IF;
    INSERT INTO audit_store.access_grants (granted_seq, issuer, principal_id, capability)
    VALUES (v_seq, p_issuer, p_principal_id, 'administer');
    RETURN QUERY SELECT 'bootstrapped'::text, v_seq, NULL::text;
END
$bootstrap_administrator$;

-- Registers a source-service principal for one source (owner members only).
-- Recorded as audit.access_policy.changed {change: granted} without a
-- capability: the only grant that carries none.
CREATE FUNCTION audit_store.register_source_service(
    p_issuer TEXT, p_principal_id TEXT, p_source TEXT)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $register_source_service$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_owner_member();
    PERFORM audit_store.require_not_recovery(h);
    IF NOT audit_store.is_principal_part(p_issuer)
       OR NOT audit_store.is_principal_part(p_principal_id)
       OR p_issuer = 'db_role'
       OR NOT audit_store.is_source_urn(p_source, TRUE) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.source_services AS s
               WHERE s.issuer = p_issuer AND s.principal_id = p_principal_id
                 AND s.source = p_source) THEN
        RETURN QUERY SELECT 'unchanged'::text, NULL::bigint, NULL::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object('change', 'granted', 'target_issuer', p_issuer,
                                      'target_principal_id', p_principal_id));
    INSERT INTO audit_store.source_services (issuer, principal_id, source, registered_seq)
    VALUES (p_issuer, p_principal_id, p_source, v_seq);
    RETURN QUERY SELECT 'registered'::text, v_seq, NULL::text;
END
$register_source_service$;

-- Clears access_reapply_pending once both duties are recorded and every
-- active retention policy was re-run after the epoch (design §11). A run
-- counts only when it drained the policy's due set under the policy cutoff
-- (a restore can bring back bodies that had expired): an
-- audit.retention.expired of the latest revision whose count stayed below
-- its limit and whose effective cutoff is the policy floor
-- (tx_time - retain_days, i.e. the requested cutoff did not narrow it), or
-- an expire_refused 'held' (an active hold blocks every expiry).
CREATE FUNCTION audit_store.retention_reapply_satisfied(h audit_store.publication_head)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $retention_reapply_satisfied$
    SELECT NOT EXISTS (
        SELECT 1
        FROM (SELECT DISTINCT ON (p.policy_id) p.policy_id, p.revision, p.retain_days
              FROM audit_store.retention_policies AS p
              ORDER BY p.policy_id, p.revision DESC) AS latest
        WHERE latest.retain_days IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM audit_store.events AS e
              JOIN audit_store.event_bodies AS b ON b.seq = e.seq
              WHERE e.origin = 'store' AND e.seq > coalesce(h.reapply_from_seq, 0)
                AND e.event_type IN ('audit.retention.expired', 'audit.retention.expire_refused')
                AND b.envelope -> 'data' -> 'details' ->> 'policy_id' = latest.policy_id
                AND CASE e.event_type
                        WHEN 'audit.retention.expired'
                        THEN audit_store.jint(b.envelope -> 'data' -> 'details' -> 'revision')
                             = latest.revision
                             AND audit_store.jint(b.envelope -> 'data' -> 'details' -> 'count')
                                 < audit_store.jint(b.envelope -> 'data' -> 'details' -> 'limit')
                             AND audit_store.jts(
                                     b.envelope -> 'data' -> 'details' -> 'effective_cutoff')
                                 = audit_store.retention_floor(
                                     audit_store.jts(b.envelope -> 'data' -> 'details' -> 'tx_time'),
                                     latest.retain_days)
                        ELSE b.envelope -> 'data' -> 'details' ->> 'refusal' = 'held'
                             AND audit_store.jint(
                                 b.envelope -> 'data' -> 'details' -> 'current_revision')
                                 = latest.revision
                    END))
$retention_reapply_satisfied$;

CREATE FUNCTION audit_store.settle_reapply()
RETURNS VOID LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET audit_store.write_context = 'definer' AS $settle_reapply$
DECLARE
    h audit_store.publication_head := audit_store.lock_head();
BEGIN
    IF h.access_reapply_pending
       AND h.access_reapplied_seq IS NOT NULL AND h.retention_reapplied_seq IS NOT NULL
       AND audit_store.retention_reapply_satisfied(h) THEN
        UPDATE audit_store.publication_head AS x
        SET access_reapply_pending = FALSE, updated_at = clock_timestamp()
        WHERE x.singleton;
    END IF;
END
$settle_reapply$;

-- An administrator records that the out-of-band access state (revocations,
-- unbindings, retention revisions) was re-applied after a recovery epoch
-- (audit.access_policy.changed {change: reapplied, capability: administer}).
CREATE FUNCTION audit_store.record_access_reapplied()
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $record_access_reapplied$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('change_access', 'audit_store_admin',
                                                    'administer');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, v_auth.denial;
        RETURN;
    END IF;
    IF NOT h.access_reapply_pending THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'not_pending'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object('change', 'reapplied', 'target_issuer', v_auth.issuer,
                                      'target_principal_id', v_auth.principal_id,
                                      'capability', 'administer'));
    UPDATE audit_store.publication_head AS x
    SET access_reapplied_seq = v_seq, updated_at = clock_timestamp() WHERE x.singleton;
    PERFORM audit_store.settle_reapply();
    RETURN QUERY SELECT 'recorded'::text, v_seq, NULL::text;
END
$record_access_reapplied$;

-- A maintainer confirms the retention re-application after a recovery epoch
-- (audit.access_policy.changed {change: reapplied, capability: maintain}).
-- Refused unless every active policy (latest revision with retain_days) was
-- re-run by expire after the epoch; with no active policy this is the
-- recorded "no active policies" check.
CREATE FUNCTION audit_store.confirm_retention_reapplied()
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $confirm_retention_reapplied$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('expire', 'audit_store_maintainer',
                                                    'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, v_auth.denial;
        RETURN;
    END IF;
    IF NOT h.access_reapply_pending THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'not_pending'::text;
        RETURN;
    END IF;
    IF NOT audit_store.retention_reapply_satisfied(h) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'retention_not_reapplied'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('store', 'audit.access_policy.changed', 'ACCESS_POLICY',
        'success', jsonb_build_object('change', 'reapplied', 'target_issuer', v_auth.issuer,
                                      'target_principal_id', v_auth.principal_id,
                                      'capability', 'maintain'));
    UPDATE audit_store.publication_head AS x
    SET retention_reapplied_seq = v_seq, updated_at = clock_timestamp() WHERE x.singleton;
    PERFORM audit_store.settle_reapply();
    RETURN QUERY SELECT 'recorded'::text, v_seq, NULL::text;
END
$confirm_retention_reapplied$;

-- ---------------------------------------------------------------------------
-- Retention (design §9)
-- ---------------------------------------------------------------------------

-- Validates and normalizes a selector; NULL when invalid. Lists are sorted
-- and de-duplicated (1..16 items, the event_type_list / source_list bound
-- MAX_CONTROL_LIST); at least one dimension is required.
CREATE FUNCTION audit_store.normalize_selector(p_selector JSONB)
RETURNS JSONB LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $normalize_selector$
DECLARE
    v_out JSONB := '{}'::jsonb;
    k TEXT;
    v_values TEXT[];
BEGIN
    IF jsonb_typeof(p_selector) IS DISTINCT FROM 'object' THEN
        RETURN NULL;
    END IF;
    IF p_selector = '{}'::jsonb
       OR EXISTS (SELECT 1 FROM jsonb_object_keys(p_selector) AS x
                  WHERE x NOT IN ('event_classes', 'event_types', 'sources')) THEN
        RETURN NULL;
    END IF;
    FOR k IN SELECT x FROM jsonb_object_keys(p_selector) AS x LOOP
        IF jsonb_typeof(p_selector -> k) <> 'array' THEN
            RETURN NULL;
        END IF;
        IF jsonb_array_length(p_selector -> k) NOT BETWEEN 1 AND 16
           OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_selector -> k) AS t
                      WHERE jsonb_typeof(t) <> 'string') THEN
            RETURN NULL;
        END IF;
        -- Closed sets: registered relay event types (never audit.*: only
        -- relay events expire), the eight classes, and registered relay
        -- sources (never the control sources).
        IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(p_selector -> k) AS t
                   WHERE (k = 'event_types'
                          AND (NOT audit_store.is_event_type(t) OR t LIKE 'audit.%'
                               OR NOT audit_store.is_registered_type(t)))
                      OR (k = 'event_classes' AND t NOT IN (
                          'SECURITY', 'PRIVILEGED_OPERATION', 'CONTENT_LIFECYCLE',
                          'ACCESS_POLICY', 'DATA_ACCESS', 'SEARCH_ACCESS',
                          'CONFIGURATION', 'SYSTEM_AUDIT'))
                      OR (k = 'sources' AND NOT audit_store.is_source_urn(t, TRUE))) THEN
            RETURN NULL;
        END IF;
        SELECT array_agg(DISTINCT t ORDER BY t) INTO v_values
        FROM jsonb_array_elements_text(p_selector -> k) AS t;
        v_out := v_out || jsonb_build_object(k, to_jsonb(v_values));
    END LOOP;
    RETURN v_out;
END
$normalize_selector$;

CREATE FUNCTION audit_store.set_retention_policy(
    p_policy_id TEXT, p_selector JSONB, p_retain_days INTEGER)
RETURNS TABLE (status TEXT, seq BIGINT, revision INTEGER, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $set_retention_policy$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    v_selector JSONB;
    v_revision INTEGER;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('set_retention_policy', 'audit_store_admin',
                                                    'administer');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::integer, v_auth.denial;
        RETURN;
    END IF;
    v_selector := audit_store.normalize_selector(p_selector);
    IF NOT audit_store.is_code(p_policy_id) OR v_selector IS NULL
       OR (p_retain_days IS NOT NULL AND p_retain_days NOT BETWEEN 1 AND 365000) THEN
        PERFORM audit_store.record_denied('set_retention_policy', 'invalid_input', 'administer');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::integer, 'invalid_input'::text;
        RETURN;
    END IF;
    SELECT coalesce(max(p.revision), 0) + 1 INTO v_revision
    FROM audit_store.retention_policies AS p WHERE p.policy_id = p_policy_id;
    v_seq := audit_store.append_control('store', 'audit.retention.policy_changed',
        'CONFIGURATION', 'success',
        jsonb_build_object(
            'policy_id', p_policy_id,
            'revision', v_revision,
            'selector_digest', encode(audit_store.jsonb_digest(v_selector), 'hex'),
            'retain_days', to_jsonb(p_retain_days))
        || audit_store.opt('selector_event_types', v_selector -> 'event_types')
        || audit_store.opt('selector_event_classes', v_selector -> 'event_classes')
        || audit_store.opt('selector_sources', v_selector -> 'sources'));
    INSERT INTO audit_store.retention_policies (
        policy_id, revision, selector, selector_digest, retain_days, created_seq)
    VALUES (p_policy_id, v_revision, v_selector, audit_store.jsonb_digest(v_selector),
            p_retain_days, v_seq);
    RETURN QUERY SELECT 'recorded'::text, v_seq, v_revision, NULL::text;
END
$set_retention_policy$;

-- expire(policy, expected revision, cutoff, limit<=1000) (design §9). A
-- refusal (stale_revision / not_expirable / held) deletes nothing and is
-- recorded as audit.retention.expire_refused. Otherwise
-- audit.retention.expired is recorded first; its seq marks the rows, then
-- the bodies are deleted.
CREATE FUNCTION audit_store.expire(
    p_policy_id TEXT, p_expected_revision INTEGER, p_cutoff TIMESTAMPTZ, p_limit INTEGER)
RETURNS TABLE (status TEXT, seq BIGINT, expired_count BIGINT, effective_cutoff TIMESTAMPTZ,
               code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer'
SET audit_store.maintenance_context = 'retention' AS $expire$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    p audit_store.retention_policies;
    v_refusal TEXT;
    v_effective TIMESTAMPTZ;
    v_targets BIGINT[] := ARRAY[]::bigint[];
    v_seq BIGINT;
BEGIN
    -- Lock order: head -> policy -> identity.
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('expire', 'audit_store_maintainer',
                                                    'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::timestamptz,
                            v_auth.denial;
        RETURN;
    END IF;
    -- The cutoff is recorded as a utc_timestamp: years 0001-9999 AD only.
    IF NOT audit_store.is_code(p_policy_id) OR p_expected_revision IS NULL
       OR p_expected_revision < 1 OR p_cutoff IS NULL OR NOT isfinite(p_cutoff)
       OR p_cutoff < '0001-01-01 00:00:00+00'::timestamptz
       OR p_cutoff >= '10000-01-01 00:00:00+00'::timestamptz
       OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        PERFORM audit_store.record_denied('expire', 'invalid_input', 'maintain');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::timestamptz,
                            'invalid_input'::text;
        RETURN;
    END IF;
    -- Every denied attempt is in the chain before retention evidence.
    PERFORM audit_store.flush_denial_streaks(TRUE);
    SELECT * INTO p FROM audit_store.retention_policies AS x
    WHERE x.policy_id = p_policy_id ORDER BY x.revision DESC LIMIT 1;
    IF p.revision IS NULL OR p.revision <> p_expected_revision THEN
        v_refusal := 'stale_revision';
    ELSIF p.retain_days IS NULL THEN
        v_refusal := 'not_expirable';
    ELSIF EXISTS (SELECT 1 FROM audit_store.legal_holds AS l WHERE l.released_at IS NULL) THEN
        v_refusal := 'held';
    END IF;
    IF v_refusal IS NOT NULL THEN
        v_seq := audit_store.append_control('store', 'audit.retention.expire_refused',
            'PRIVILEGED_OPERATION', 'failure', jsonb_build_object(
                'policy_id', p_policy_id,
                'expected_revision', p_expected_revision,
                'current_revision', to_jsonb(p.revision),
                'refusal', v_refusal,
                'retain_days', to_jsonb(p.retain_days),
                'cutoff', audit_store.utc_text(p_cutoff),
                'tx_time', audit_store.utc_text(transaction_timestamp())));
        IF h.access_reapply_pending AND audit_store.retention_reapply_satisfied(h) THEN
            UPDATE audit_store.publication_head AS x
            SET retention_reapplied_seq = coalesce(x.retention_reapplied_seq, v_seq),
                updated_at = clock_timestamp()
            WHERE x.singleton;
            PERFORM audit_store.settle_reapply();
        END IF;
        RETURN QUERY SELECT v_refusal, v_seq, 0::bigint, NULL::timestamptz, v_refusal;
        RETURN;
    END IF;
    v_effective := least(p_cutoff,
                         audit_store.retention_floor(transaction_timestamp(), p.retain_days));
    SELECT coalesce(array_agg(t.seq ORDER BY t.seq), ARRAY[]::bigint[]) INTO v_targets
    FROM (
        SELECT e.seq FROM audit_store.events AS e
        JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.origin = 'relay'
          AND e.occurred_at < v_effective
          AND e.expired_at IS NULL
          AND (NOT p.selector ? 'event_types' OR e.event_type IN (
               SELECT jsonb_array_elements_text(p.selector -> 'event_types')))
          AND (NOT p.selector ? 'event_classes' OR e.event_class IN (
               SELECT jsonb_array_elements_text(p.selector -> 'event_classes')))
          AND (NOT p.selector ? 'sources' OR e.source IN (
               SELECT jsonb_array_elements_text(p.selector -> 'sources')))
        ORDER BY e.seq
        LIMIT p_limit
        FOR UPDATE OF e
    ) AS t;
    -- The evidence is recorded first; its seq marks the expired rows.
    v_seq := audit_store.append_control('store', 'audit.retention.expired',
        'PRIVILEGED_OPERATION', 'success',
        jsonb_build_object(
            'policy_id', p_policy_id,
            'revision', p.revision,
            'selector_digest', encode(p.selector_digest, 'hex'),
            'retain_days', p.retain_days,
            'cutoff', audit_store.utc_text(p_cutoff),
            'effective_cutoff', audit_store.utc_text(v_effective),
            'tx_time', audit_store.utc_text(transaction_timestamp()),
            'limit', p_limit,
            'count', cardinality(v_targets),
            'first_seq', to_jsonb(v_targets[1]),
            'last_seq', to_jsonb(v_targets[cardinality(v_targets)]),
            'expired_set_digest', encode(audit_store.expired_set_digest(v_targets), 'hex'))
        || audit_store.opt('selector_event_types', p.selector -> 'event_types')
        || audit_store.opt('selector_event_classes', p.selector -> 'event_classes')
        || audit_store.opt('selector_sources', p.selector -> 'sources'));
    IF cardinality(v_targets) > 0 THEN
        UPDATE audit_store.events AS e
        SET expired_at = transaction_timestamp(), expired_by_seq = v_seq
        WHERE e.seq = ANY (v_targets);
        DELETE FROM audit_store.event_bodies AS b WHERE b.seq = ANY (v_targets);
    END IF;
    IF h.access_reapply_pending AND audit_store.retention_reapply_satisfied(h) THEN
        UPDATE audit_store.publication_head AS x
        SET retention_reapplied_seq = coalesce(x.retention_reapplied_seq, v_seq),
            updated_at = clock_timestamp()
        WHERE x.singleton;
        PERFORM audit_store.settle_reapply();
    END IF;
    RETURN QUERY SELECT 'expired'::text, v_seq, cardinality(v_targets)::bigint, v_effective,
                        NULL::text;
END
$expire$;

CREATE FUNCTION audit_store.purge_body(p_event_id UUID, p_reason_code TEXT)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer'
SET audit_store.maintenance_context = 'retention' AS $purge_body$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    e audit_store.events;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('purge_body', 'audit_store_maintainer',
                                                    'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, v_auth.denial;
        RETURN;
    END IF;
    SELECT * INTO e FROM audit_store.events AS x WHERE x.event_id = p_event_id FOR UPDATE;
    IF NOT FOUND OR e.origin <> 'relay' OR e.expired_at IS NOT NULL
       OR p_reason_code IS NULL
       OR p_reason_code NOT IN ('minimization_failure', 'prohibited_content', 'adapter_defect')
       OR NOT EXISTS (SELECT 1 FROM audit_store.event_bodies AS b WHERE b.seq = e.seq) THEN
        PERFORM audit_store.record_denied('purge_body', 'invalid_input', 'maintain');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    PERFORM audit_store.flush_denial_streaks(TRUE);
    v_seq := audit_store.append_control('store', 'audit.body.purged', 'PRIVILEGED_OPERATION',
        'success', jsonb_build_object('target_seq', e.seq, 'target_event_id', e.event_id::text,
                                      'purge_reason_code', p_reason_code));
    UPDATE audit_store.events AS x SET expired_at = transaction_timestamp(), expired_by_seq = v_seq
    WHERE x.seq = e.seq;
    DELETE FROM audit_store.event_bodies AS b WHERE b.seq = e.seq;
    RETURN QUERY SELECT 'purged'::text, v_seq, NULL::text;
END
$purge_body$;

-- ---------------------------------------------------------------------------
-- Restore and recovery (design §11)
-- ---------------------------------------------------------------------------

-- A maintainer declares that the Store must enter recovery mode (e.g. a
-- known restore the fingerprint cannot see). Only sets recovery_pending with
-- the incident code and evidence; allowed in recovery mode.
CREATE FUNCTION audit_store.declare_recovery_pending(p_incident_code TEXT)
RETURNS TABLE (status TEXT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $declare_recovery_pending$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
BEGIN
    h := audit_store.lock_head();
    SELECT * INTO v_auth FROM audit_store.authorize('declare_recovery_pending',
                                                    'audit_store_maintainer', 'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, v_auth.denial;
        RETURN;
    END IF;
    IF NOT audit_store.is_code(p_incident_code) THEN
        PERFORM audit_store.record_denied('declare_recovery_pending', 'invalid_input', 'maintain');
        RETURN QUERY SELECT 'denied'::text, 'invalid_input'::text;
        RETURN;
    END IF;
    IF h.recovery_pending THEN
        RETURN QUERY SELECT 'already_pending'::text, h.pending_reason;
        RETURN;
    END IF;
    UPDATE audit_store.publication_head AS x
    SET recovery_pending = TRUE, pending_reason = 'declared', pending_seq = NULL,
        pending_event_id = NULL, pending_digest = NULL, pending_by = session_user::text,
        pending_at = clock_timestamp(), pending_head_seq = h.last_seq,
        pending_incident_code = p_incident_code, updated_at = clock_timestamp()
    WHERE x.singleton;
    RETURN QUERY SELECT 'recovery_pending'::text, 'declared'::text;
END
$declare_recovery_pending$;

-- Starts a new recovery epoch (design §11). Precondition: fingerprint
-- mismatch or recovery_pending (the only way to clear it). Re-verifies the
-- restored chain under the head lock, records audit.recovery.epoch_started
-- (classification restore / planned_move / regression, checkpoint and its
-- classification, identity range digest, the lost range, the regression
-- evidence, old/new fingerprint), sets the new fingerprint and
-- access_reapply_pending. A planned move is an epoch whose checkpoint equals
-- the restored head and whose lost range is empty.
--
-- The operator states what the out-of-band recovery record says (design
-- §8): the old epoch, the restored head (seq and chain) and the claimed
-- upper bound of the lost range, or NULL for a bound the Store cannot know
-- (lost_upper_known false: no checkpoint, no relay seq, no regression
-- report). The epoch starts only when all four equal the Store's actual
-- values (a NULL bound only when the actual bound is unknown); otherwise it
-- is refused ('expectation_mismatch') and nothing changes. Without any
-- expectation the call is a preview ('expectation_required'): it returns the
-- actual values and changes nothing. Refusals in recovery mode are not
-- recorded (the head must not move).
CREATE FUNCTION audit_store.begin_recovery_epoch(
    p_checkpoint_epoch BIGINT, p_checkpoint_seq BIGINT, p_checkpoint_chain TEXT,
    p_relay_max_seq BIGINT, p_expected_old_epoch BIGINT, p_expected_head_seq BIGINT,
    p_expected_head_chain TEXT, p_expected_lost_upper BIGINT)
RETURNS TABLE (status TEXT, seq BIGINT, old_epoch BIGINT, new_epoch BIGINT,
               restored_head_seq BIGINT, restored_head_chain TEXT, classification TEXT,
               checkpoint_classification TEXT, lost_from_seq BIGINT, lost_upper_seq BIGINT,
               lost_upper_known BOOLEAN, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer'
SET audit_store.maintenance_context = 'recovery' AS $begin_recovery_epoch$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    fp record;
    v_auth record;
    v_head BIGINT;
    s record;
    v_has_checkpoint BOOLEAN;
    v_preview BOOLEAN;
    v_chain_at BYTEA;
    v_epoch_at BIGINT;
    v_cp_class TEXT;
    v_upper BIGINT;
    v_known BOOLEAN;
    v_class TEXT;
    v_identity BYTEA;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    SELECT * INTO fp FROM audit_store.current_fingerprint();
    IF NOT audit_store.in_recovery(h) THEN
        RAISE EXCEPTION 'not_in_recovery' USING ERRCODE = '55000';
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.posture_check()) THEN
        RAISE EXCEPTION 'store_posture_invalid' USING ERRCODE = 'KA002';
    END IF;
    -- In recovery mode a refusal is returned but not recorded.
    SELECT * INTO v_auth FROM audit_store.authorize('begin_recovery_epoch',
                                                    'audit_store_maintainer', 'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::bigint,
                            NULL::bigint, NULL::text, NULL::text, NULL::text, NULL::bigint,
                            NULL::bigint, NULL::boolean, v_auth.denial;
        RETURN;
    END IF;
    v_has_checkpoint := p_checkpoint_seq IS NOT NULL;
    v_preview := p_expected_old_epoch IS NULL AND p_expected_head_seq IS NULL
                 AND p_expected_head_chain IS NULL AND p_expected_lost_upper IS NULL;
    IF (p_checkpoint_epoch IS NULL) <> (p_checkpoint_seq IS NULL)
       OR (p_checkpoint_chain IS NULL) <> (p_checkpoint_seq IS NULL)
       OR (v_has_checkpoint AND (p_checkpoint_epoch < 1 OR p_checkpoint_seq < 0
                                 OR NOT audit_store.is_hex64(p_checkpoint_chain)))
       OR coalesce(p_relay_max_seq, 0) < 0
       OR (NOT v_preview
           AND (p_expected_old_epoch IS NULL OR p_expected_old_epoch < 1
                OR p_expected_head_seq IS NULL OR p_expected_head_seq < 0
                OR NOT audit_store.is_hex64(p_expected_head_chain)
                OR coalesce(p_expected_lost_upper < p_expected_head_seq, FALSE))) THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::bigint,
                            NULL::bigint, NULL::text, NULL::text, NULL::text, NULL::bigint,
                            NULL::bigint, NULL::boolean, 'invalid_input'::text;
        RETURN;
    END IF;
    -- Re-verify the restored chain under the head lock.
    SELECT coalesce(max(e.seq), 0) INTO v_head FROM audit_store.events AS e;
    SELECT * INTO s FROM audit_store.integrity_scan(1, v_head, FALSE);
    IF audit_store.violations_total(s.violations) > 0 THEN
        RAISE EXCEPTION 'restored_chain_invalid' USING ERRCODE = '55000';
    END IF;
    IF v_has_checkpoint THEN
        IF p_checkpoint_seq > v_head THEN
            v_cp_class := 'store_behind';
        ELSE
            IF p_checkpoint_seq = 0 THEN
                v_chain_at := audit_store.genesis();
                v_epoch_at := 1;
            ELSE
                SELECT e.chain, e.recovery_epoch INTO v_chain_at, v_epoch_at
                FROM audit_store.events AS e WHERE e.seq = p_checkpoint_seq;
            END IF;
            -- As audit_core::compare_checkpoint: a different chain is a
            -- rewrite; the same chain under another epoch is an altered
            -- epoch column or checkpoint log.
            v_cp_class := CASE WHEN v_chain_at IS DISTINCT FROM decode(p_checkpoint_chain, 'hex')
                               THEN 'mismatch'
                               WHEN v_epoch_at IS DISTINCT FROM p_checkpoint_epoch
                               THEN 'epoch_mismatch'
                               WHEN p_checkpoint_seq = v_head THEN 'match'
                               ELSE 'ahead' END;
        END IF;
    END IF;
    -- Claimed upper bound of the lost range (restored_head, upper]: never
    -- below the restored head.
    v_upper := greatest(v_head, coalesce(p_checkpoint_seq, 0), coalesce(p_relay_max_seq, 0),
                        CASE WHEN h.pending_reason = 'regression'
                             THEN coalesce(h.pending_seq, 0) ELSE 0 END);
    -- Known only from a checkpoint, the relay's highest referenced seq or a
    -- regression report. pending_reason is NULL after a restore into a new
    -- database (fingerprint mismatch): never let NULL leak into the record.
    v_known := v_has_checkpoint OR p_relay_max_seq IS NOT NULL
               OR coalesce(h.pending_reason = 'regression', FALSE);
    v_class := CASE WHEN h.pending_reason = 'regression' THEN 'regression'
                    WHEN v_cp_class = 'match' AND v_upper = v_head THEN 'planned_move'
                    ELSE 'restore' END;
    IF v_preview
       OR p_expected_old_epoch <> h.recovery_epoch
       OR p_expected_head_seq <> v_head
       OR decode(p_expected_head_chain, 'hex') <> s.head_chain
       OR p_expected_lost_upper IS DISTINCT FROM (CASE WHEN v_known THEN v_upper END) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, h.recovery_epoch,
                            h.recovery_epoch + 1, v_head, encode(s.head_chain, 'hex'), v_class,
                            v_cp_class, v_head + 1, v_upper, v_known,
                            CASE WHEN v_preview THEN 'expectation_required'
                                 ELSE 'expectation_mismatch' END::text;
        RETURN;
    END IF;
    SELECT sha256('kp-audit-identity-range-v1'::bytea
                  || coalesce(string_agg(int8send(e.seq) || uuid_send(e.event_id)
                                         || e.envelope_digest || e.chain, ''::bytea
                                         ORDER BY e.seq), ''::bytea))
    INTO v_identity FROM audit_store.events AS e WHERE e.seq <= v_head;
    UPDATE audit_store.publication_head AS x
    SET last_seq = v_head, last_chain = s.head_chain, recovery_epoch = h.recovery_epoch + 1,
        fp_system_identifier = fp.system_identifier, fp_database_oid = fp.database_oid,
        fp_timeline = fp.timeline, recovery_pending = FALSE, pending_reason = NULL,
        pending_seq = NULL, pending_event_id = NULL, pending_digest = NULL, pending_by = NULL,
        pending_at = NULL, pending_head_seq = NULL, pending_incident_code = NULL,
        access_reapply_pending = FALSE, reapply_from_seq = NULL, access_reapplied_seq = NULL,
        retention_reapplied_seq = NULL, updated_at = clock_timestamp()
    WHERE x.singleton;
    v_seq := audit_store.append_control('store', 'audit.recovery.epoch_started', 'SYSTEM_AUDIT',
        'success', jsonb_build_object(
            'classification', v_class,
            'old_epoch', h.recovery_epoch,
            'new_epoch', h.recovery_epoch + 1,
            'restored_head_seq', v_head,
            'restored_head_chain', encode(s.head_chain, 'hex'),
            'checkpoint_epoch', to_jsonb(p_checkpoint_epoch),
            'checkpoint_seq', to_jsonb(p_checkpoint_seq),
            'checkpoint_chain', to_jsonb(p_checkpoint_chain),
            'checkpoint_classification', to_jsonb(v_cp_class),
            'identity_range_digest', encode(v_identity, 'hex'),
            'relay_max_seq', to_jsonb(p_relay_max_seq),
            'lost_from_seq', v_head + 1,
            'lost_upper_seq', v_upper,
            'lost_upper_known', v_known,
            'regression_reported_seq', to_jsonb(h.pending_seq),
            'regression_reported_event_id', to_jsonb(h.pending_event_id::text),
            'regression_reported_digest', to_jsonb(encode(h.pending_digest, 'hex')),
            'regression_reported_by', to_jsonb(h.pending_by),
            'regression_reported_at', to_jsonb(audit_store.utc_text(h.pending_at)),
            'regression_head_seq', to_jsonb(h.pending_head_seq),
            'old_system_identifier', h.fp_system_identifier,
            'old_database_oid', h.fp_database_oid::text,
            'old_timeline', h.fp_timeline,
            'new_system_identifier', fp.system_identifier,
            'new_database_oid', fp.database_oid::text,
            'new_timeline', fp.timeline));
    UPDATE audit_store.publication_head AS x
    SET access_reapply_pending = TRUE, reapply_from_seq = v_seq, updated_at = clock_timestamp()
    WHERE x.singleton;
    RETURN QUERY SELECT 'epoch_started'::text, v_seq, h.recovery_epoch, h.recovery_epoch + 1,
                        v_head, encode(s.head_chain, 'hex'), v_class, v_cp_class, v_head + 1,
                        v_upper, v_known, NULL::text;
END
$begin_recovery_epoch$;

-- ---------------------------------------------------------------------------
-- Guard triggers (design §7.3). Accident prevention; the boundary is that no
-- role other than the owner has table privileges.
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_store.in_context(p_name TEXT, p_values TEXT[])
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $in_context$
    SELECT coalesce(current_setting(p_name, TRUE), '') = ANY (p_values)
$in_context$;

CREATE FUNCTION audit_store.refuse_mutation()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $refuse_mutation$
BEGIN
    RAISE EXCEPTION 'audit_store is append-only: % on % refused', TG_OP, TG_TABLE_NAME
        USING ERRCODE = '55000';
END
$refuse_mutation$;

-- events: INSERT by definer functions; UPDATE only sets the expiry mark from
-- NULL under the retention context; DELETE never (TRUNCATE: refuse_mutation).
CREATE FUNCTION audit_store.guard_events()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_events$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT audit_store.in_context('audit_store.write_context', ARRAY['definer']) THEN
            RAISE EXCEPTION 'audit_store.events: direct INSERT refused' USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        IF NOT audit_store.in_context('audit_store.maintenance_context', ARRAY['retention'])
           OR OLD.expired_at IS NOT NULL OR OLD.expired_by_seq IS NOT NULL
           OR NEW.expired_at IS NULL OR NEW.expired_by_seq IS NULL
           OR (to_jsonb(NEW) - 'expired_at' - 'expired_by_seq')
              IS DISTINCT FROM (to_jsonb(OLD) - 'expired_at' - 'expired_by_seq') THEN
            RAISE EXCEPTION 'audit_store.events: UPDATE refused' USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_store.events: DELETE refused' USING ERRCODE = '55000';
END
$guard_events$;

-- event_bodies: INSERT by definer functions; DELETE only under the retention
-- context and only after the identity row carries its expiry mark.
CREATE FUNCTION audit_store.guard_event_bodies()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_event_bodies$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT audit_store.in_context('audit_store.write_context', ARRAY['definer']) THEN
            RAISE EXCEPTION 'audit_store.event_bodies: direct INSERT refused'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        IF NOT audit_store.in_context('audit_store.maintenance_context', ARRAY['retention'])
           OR NOT EXISTS (SELECT 1 FROM audit_store.events AS e
                          WHERE e.seq = OLD.seq AND e.expired_at IS NOT NULL) THEN
            RAISE EXCEPTION 'audit_store.event_bodies: DELETE refused' USING ERRCODE = '55000';
        END IF;
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'audit_store.event_bodies: UPDATE refused' USING ERRCODE = '55000';
END
$guard_event_bodies$;

-- publication_head: UPDATE by definer functions; seq and epoch never move
-- backwards and recovery_pending is never cleared, except when
-- begin_recovery_epoch (recovery context) recomputes the restored head.
CREATE FUNCTION audit_store.guard_publication_head()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_publication_head$
DECLARE
    v_recovery BOOLEAN := audit_store.in_context('audit_store.maintenance_context',
                                                 ARRAY['recovery']);
BEGIN
    IF TG_OP = 'UPDATE'
       AND audit_store.in_context('audit_store.write_context', ARRAY['definer'])
       AND NEW.recovery_epoch >= OLD.recovery_epoch
       AND (NEW.last_seq >= OLD.last_seq OR v_recovery)
       AND (NEW.recovery_pending OR NOT OLD.recovery_pending OR v_recovery) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_store.publication_head: % refused', TG_OP USING ERRCODE = '55000';
END
$guard_publication_head$;

-- Append-only tables written by definer functions (intents, policies,
-- source services).
CREATE FUNCTION audit_store.guard_append_only()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_append_only$
BEGIN
    IF TG_OP = 'INSERT'
       AND audit_store.in_context('audit_store.write_context', ARRAY['definer']) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_store.%: % refused', TG_TABLE_NAME, TG_OP USING ERRCODE = '55000';
END
$guard_append_only$;

-- Bindings and grants: append-only history; the only UPDATE sets the
-- end-of-validity seq (unbound_seq / revoked_seq) from NULL.
CREATE FUNCTION audit_store.guard_history()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_history$
DECLARE
    v_end TEXT := CASE TG_TABLE_NAME WHEN 'principal_bindings' THEN 'unbound_seq'
                                     ELSE 'revoked_seq' END;
BEGIN
    IF TG_OP = 'INSERT'
       AND audit_store.in_context('audit_store.write_context', ARRAY['definer'])
       AND to_jsonb(NEW) -> v_end = 'null'::jsonb THEN
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE'
       AND audit_store.in_context('audit_store.maintenance_context', ARRAY['access'])
       AND to_jsonb(OLD) -> v_end = 'null'::jsonb
       AND to_jsonb(NEW) -> v_end <> 'null'::jsonb
       AND (to_jsonb(NEW) - v_end) = (to_jsonb(OLD) - v_end) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_store.%: % refused', TG_TABLE_NAME, TG_OP USING ERRCODE = '55000';
END
$guard_history$;

-- registered_types change only in migrations.
CREATE FUNCTION audit_store.guard_registered_types()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_registered_types$
BEGIN
    IF audit_store.in_context('audit_store.write_context', ARRAY['migration']) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;
    RAISE EXCEPTION 'audit_store.registered_types: % outside a migration', TG_OP
        USING ERRCODE = '55000';
END
$guard_registered_types$;

-- Operational state (denial_streaks): any row change by definer functions.
CREATE FUNCTION audit_store.guard_state()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_state$
BEGIN
    IF audit_store.in_context('audit_store.write_context', ARRAY['definer']) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;
    RAISE EXCEPTION 'audit_store.%: % refused', TG_TABLE_NAME, TG_OP USING ERRCODE = '55000';
END
$guard_state$;

CREATE TRIGGER events_guard BEFORE INSERT OR UPDATE OR DELETE ON audit_store.events
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_events();
CREATE TRIGGER events_no_truncate BEFORE TRUNCATE ON audit_store.events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER event_bodies_guard BEFORE INSERT OR UPDATE OR DELETE ON audit_store.event_bodies
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_event_bodies();
CREATE TRIGGER event_bodies_no_truncate BEFORE TRUNCATE ON audit_store.event_bodies
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();

-- The singleton head exists before its guard.
INSERT INTO audit_store.publication_head (
    singleton, last_seq, last_chain, recovery_epoch, fp_system_identifier, fp_database_oid,
    fp_timeline, recovery_pending, updated_at)
SELECT TRUE, 0, audit_store.genesis(), 1, fp.system_identifier, fp.database_oid, fp.timeline,
       FALSE, clock_timestamp()
FROM audit_store.current_fingerprint() AS fp;

CREATE TRIGGER publication_head_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.publication_head
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_publication_head();
CREATE TRIGGER publication_head_no_truncate BEFORE TRUNCATE ON audit_store.publication_head
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER access_intents_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.access_intents
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_append_only();
CREATE TRIGGER access_intents_no_truncate BEFORE TRUNCATE ON audit_store.access_intents
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER retention_policies_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.retention_policies
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_append_only();
CREATE TRIGGER retention_policies_no_truncate BEFORE TRUNCATE ON audit_store.retention_policies
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER source_services_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.source_services
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_append_only();
CREATE TRIGGER source_services_no_truncate BEFORE TRUNCATE ON audit_store.source_services
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER principal_bindings_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.principal_bindings
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_history();
CREATE TRIGGER principal_bindings_no_truncate BEFORE TRUNCATE ON audit_store.principal_bindings
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER access_grants_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.access_grants
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_history();
CREATE TRIGGER access_grants_no_truncate BEFORE TRUNCATE ON audit_store.access_grants
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER registered_types_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.registered_types
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_registered_types();
CREATE TRIGGER registered_types_no_truncate BEFORE TRUNCATE ON audit_store.registered_types
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER denial_streaks_guard
    BEFORE INSERT OR UPDATE OR DELETE ON audit_store.denial_streaks
    FOR EACH ROW EXECUTE FUNCTION audit_store.guard_state();
CREATE TRIGGER denial_streaks_no_truncate BEFORE TRUNCATE ON audit_store.denial_streaks
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER legal_holds_guard BEFORE UPDATE OR DELETE ON audit_store.legal_holds
    FOR EACH ROW EXECUTE FUNCTION audit_store.refuse_mutation();
CREATE TRIGGER legal_holds_no_truncate BEFORE TRUNCATE ON audit_store.legal_holds
    FOR EACH STATEMENT EXECUTE FUNCTION audit_store.refuse_mutation();

-- ---------------------------------------------------------------------------
-- Ownership and PUBLIC revocation (design §7.3 items 3–4)
-- ---------------------------------------------------------------------------

DO $ownership$
DECLARE
    r record;
BEGIN
    FOR r IN
        SELECT c.relname FROM pg_catalog.pg_class AS c
        JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
        WHERE n.nspname = 'audit_store' AND c.relkind IN ('r', 'S')
    LOOP
        EXECUTE format('ALTER TABLE audit_store.%I OWNER TO audit_store_owner', r.relname);
    END LOOP;
    FOR r IN
        SELECT p.oid::regprocedure AS signature FROM pg_catalog.pg_proc AS p
        JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace
        WHERE n.nspname = 'audit_store'
    LOOP
        EXECUTE format('ALTER FUNCTION %s OWNER TO audit_store_owner', r.signature);
    END LOOP;
END
$ownership$;

REVOKE ALL ON SCHEMA audit_store FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA audit_store FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA audit_store FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA audit_store FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE audit_store_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
