-- Audit Store v1 (design 2026-10-07 revision 2, §7–§11; decision record D2/D3).
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
CREATE TABLE audit_store.publication_head (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    last_seq BIGINT NOT NULL,
    last_chain BYTEA NOT NULL,
    recovery_epoch BIGINT NOT NULL,
    fp_system_identifier TEXT NOT NULL,
    fp_database_oid OID NOT NULL,
    fp_timeline TEXT NOT NULL,
    recovery_pending BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_head_singleton CHECK (singleton),
    CONSTRAINT ck_head_seq CHECK (last_seq >= 0),
    CONSTRAINT ck_head_chain CHECK (octet_length(last_chain) = 32),
    CONSTRAINT ck_head_epoch CHECK (recovery_epoch >= 1),
    CONSTRAINT ck_head_genesis CHECK (
        last_seq > 0
        OR last_chain = decode('9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344', 'hex')
    )
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
    CONSTRAINT ck_events_expirable CHECK (expired_at IS NULL OR origin = 'relay')
);

CREATE INDEX events_expired_by ON audit_store.events (expired_by_seq)
    WHERE expired_by_seq IS NOT NULL;
CREATE INDEX events_source_seq ON audit_store.events (source, seq);

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

-- Committed disclosure intents (design §10.3). Append-only.
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

-- (source, type, adapter_version) the Store accepts from the relay. Seeded
-- from spec/telemetry/audit-event-catalog.json; a test pins the equality.
CREATE TABLE audit_store.registered_types (
    source TEXT NOT NULL,
    event_type TEXT NOT NULL,
    adapter_version INTEGER NOT NULL,
    CONSTRAINT registered_types_pkey PRIMARY KEY (source, event_type, adapter_version),
    CONSTRAINT ck_registered_adapter CHECK (adapter_version >= 1),
    CONSTRAINT ck_registered_not_control CHECK (
        event_type NOT LIKE 'audit.%'
        AND source NOT IN ('urn:knowledge-platform:audit-store',
                           'urn:knowledge-platform:audit-relay')
    )
);

INSERT INTO audit_store.registered_types (source, event_type, adapter_version) VALUES
    ('urn:knowledge-platform:document-platform', 'document.created', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.created', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.updated', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.rebased', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.published', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.publication.scheduled', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.publication.cancelled', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.publication.terminal', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.withdrawn', 1),
    ('urn:knowledge-platform:document-platform', 'document.publication.ended', 1),
    ('urn:knowledge-platform:document-platform', 'document.metadata.changed', 1),
    ('urn:knowledge-platform:document-platform', 'document.moved', 1),
    ('urn:knowledge-platform:document-platform', 'folder.created', 1),
    ('urn:knowledge-platform:document-platform', 'folder.renamed', 1),
    ('urn:knowledge-platform:document-platform', 'folder.moved', 1),
    ('urn:knowledge-platform:document-platform', 'access_policy.changed', 1),
    ('urn:knowledge-platform:document-platform', 'document.version.read_confirmed', 1),
    ('urn:knowledge-platform:document-platform', 'document.file.access_granted', 1),
    ('urn:knowledge-platform:document-platform', 'document.diff.result_access_granted', 1),
    ('urn:knowledge-platform:document-platform', 'document.revision_comparison.result_access_granted', 1),
    ('urn:knowledge-platform:document-platform', 'authorization.denied', 1);

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

CREATE FUNCTION audit_store.utc_text(p_at TIMESTAMPTZ)
RETURNS TEXT LANGUAGE sql IMMUTABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $utc_text$
    SELECT to_char(p_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
$utc_text$;

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

-- One export line (design §10.4), built from text without a serde round trip.
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
        || ',"envelope":' || coalesce(p_envelope::text, 'null')
        || '}'
$export_line$;

-- ---------------------------------------------------------------------------
-- Fingerprint, posture and the recovery gate (design §7.3, §11)
-- ---------------------------------------------------------------------------

-- (system_identifier, database oid, timeline). On a standby the WAL insert
-- position is unavailable; the timeline is reported as 'standby', which never
-- matches, so publication stays closed there.
CREATE FUNCTION audit_store.current_fingerprint(
    OUT system_identifier TEXT, OUT database_oid OID, OUT timeline TEXT)
LANGUAGE sql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $current_fingerprint$
    SELECT (SELECT c.system_identifier::text FROM pg_control_system() AS c),
           (SELECT d.oid FROM pg_database AS d WHERE d.datname = current_database()),
           CASE WHEN pg_is_in_recovery() THEN 'standby'
                ELSE substr(pg_walfile_name(pg_current_wal_lsn()), 1, 8) END
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
    ('audit_store_ingest'), ('audit_store_relay_control'), ('audit_store_reader'),
    ('audit_store_verifier'), ('audit_store_admin'), ('audit_store_maintainer')
),
-- Design §10.1 matrix, plus content-free status/posture for operators.
expected(proname, rolname) AS (VALUES
    ('ingest', 'audit_store_ingest'),
    ('probe', 'audit_store_ingest'),
    ('lookup_receipts', 'audit_store_ingest'),
    ('list_source_receipts', 'audit_store_ingest'),
    ('lookup_control_receipts', 'audit_store_ingest'),
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
    ('expire', 'audit_store_maintainer'),
    ('purge_body', 'audit_store_maintainer'),
    ('begin_recovery_epoch', 'audit_store_maintainer'),
    ('rebind_fingerprint', 'audit_store_maintainer'),
    ('verify_recovery', 'audit_store_maintainer'),
    ('identity_chain_recovery_page', 'audit_store_maintainer'),
    ('store_status', 'audit_store_ingest'),
    ('store_status', 'audit_store_relay_control'),
    ('store_status', 'audit_store_reader'),
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
member_edges AS (
    SELECT m.member, m.roleid FROM pg_auth_members AS m
),
capability_members AS (
    WITH RECURSIVE reach(member) AS (
        SELECT e.member FROM member_edges AS e
        JOIN pg_roles AS r ON r.oid = e.roleid
        WHERE r.rolname IN (SELECT c.rolname FROM capability AS c)
        UNION
        SELECT e.member FROM member_edges AS e JOIN reach ON e.roleid = reach.member
    )
    SELECT DISTINCT r.oid, r.rolname::text AS rolname FROM reach
    JOIN pg_roles AS r ON r.oid = reach.member
    WHERE r.rolcanlogin
),
this_db AS (
    SELECT d.oid, d.datdba, d.datacl FROM pg_database AS d WHERE d.datname = current_database()
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
SELECT 'login_timeouts_missing', m.rolname FROM capability_members AS m
WHERE EXISTS (
    SELECT 1 FROM unnest(ARRAY['statement_timeout', 'lock_timeout',
                               'idle_in_transaction_session_timeout']) AS setting(name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_db_role_setting AS s, unnest(s.setconfig) AS cfg
        WHERE s.setrole = m.oid
          AND s.setdatabase IN (0, (SELECT d.oid FROM this_db AS d))
          AND cfg LIKE setting.name || '=%'))
$posture_check$;

-- NULL when publication may proceed, else the outage code.
CREATE FUNCTION audit_store.gate_code(h audit_store.publication_head)
RETURNS TEXT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $gate_code$
DECLARE
    fp record;
BEGIN
    IF h.recovery_pending THEN
        RETURN 'store_recovery_required';
    END IF;
    SELECT * INTO fp FROM audit_store.current_fingerprint();
    IF (fp.system_identifier, fp.database_oid, fp.timeline)
        IS DISTINCT FROM (h.fp_system_identifier, h.fp_database_oid, h.fp_timeline) THEN
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

-- Locks and returns the head (lock order: head first, always). Every writer
-- calls this first. synchronous_commit is set transaction-locally because a
-- function-level SET clause would be reverted before COMMIT.
CREATE FUNCTION audit_store.lock_head()
RETURNS audit_store.publication_head LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lock_head$
DECLARE
    h audit_store.publication_head;
BEGIN
    PERFORM set_config('synchronous_commit', 'on', TRUE);
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton FOR UPDATE;
    RETURN h;
END
$lock_head$;

-- ---------------------------------------------------------------------------
-- Principals and capabilities (design §10.2)
-- ---------------------------------------------------------------------------

-- The actor of a control event: the session's bound principal, or the DB
-- login itself under the fixed issuer audit-store-db-role when unbound.
CREATE FUNCTION audit_store.session_actor(
    OUT issuer TEXT, OUT principal_id TEXT, OUT bound BOOLEAN)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $session_actor$
BEGIN
    SELECT b.issuer, b.principal_id, TRUE INTO issuer, principal_id, bound
    FROM audit_store.principal_bindings AS b
    WHERE b.db_role = session_user::text AND b.unbound_seq IS NULL;
    IF NOT FOUND THEN
        issuer := 'audit-store-db-role';
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
        adapter_version, prev_chain, chain, recovery_epoch)
    VALUES (
        v_seq, v_id, p_origin, p_envelope ->> 'source', p_envelope ->> 'type',
        v_data ->> 'event_class', p_envelope ->> 'subject', (p_envelope ->> 'time')::timestamptz,
        clock_timestamp(), v_data -> 'actor' ->> 'issuer', v_data -> 'actor' ->> 'principal_id',
        v_data -> 'resource' ->> 'type', v_data -> 'resource' ->> 'id',
        (v_data -> 'resource' ->> 'version_id')::uuid, v_data ->> 'result', v_digest,
        'kp-audit-jsonb-sha256-v1', p_commitment, p_adapter_version, h.last_chain, v_chain,
        h.recovery_epoch);
    INSERT INTO audit_store.event_bodies (seq, envelope) VALUES (v_seq, p_envelope);
    UPDATE audit_store.publication_head AS p
    SET last_seq = v_seq, last_chain = v_chain, updated_at = clock_timestamp()
    WHERE p.singleton;
    RETURN v_seq;
END
$append_event$;

-- Builds a control envelope (design §4.1, §4.5) and appends it.
CREATE FUNCTION audit_store.append_control(
    p_origin TEXT, p_type TEXT, p_class TEXT, p_result TEXT, p_details JSONB)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $append_control$
DECLARE
    v_actor record;
    v_envelope JSONB;
BEGIN
    IF p_origin NOT IN ('store', 'relay_control') THEN
        RAISE EXCEPTION 'append_control: invalid origin';
    END IF;
    SELECT * INTO v_actor FROM audit_store.session_actor();
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
            'actor', jsonb_build_object('issuer', v_actor.issuer,
                                        'principal_id', v_actor.principal_id),
            'resource', jsonb_build_object('type', 'AuditStore', 'id', 'audit-store'),
            'result', p_result,
            'correlation', '{}'::jsonb,
            'details', jsonb_build_object('session_role', session_user::text) || p_details,
            'extensions', '{}'::jsonb,
            'provenance', jsonb_build_object(
                'source_format', CASE p_origin WHEN 'store' THEN 'audit-store-control-v1'
                                               ELSE 'audit-relay-control-v1' END,
                'adapter_version', 1)));
    RETURN audit_store.append_event(p_origin, v_envelope, NULL, 1);
END
$append_control$;

-- Records audit.access.denied with a bounded shape only (design §10.3).
CREATE FUNCTION audit_store.record_denied(
    p_operation TEXT, p_denial_code TEXT, p_capability TEXT)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $record_denied$
BEGIN
    RETURN audit_store.append_control('store', 'audit.access.denied', 'SECURITY', 'denied',
        jsonb_build_object('operation', p_operation, 'denial_code', p_denial_code)
        || audit_store.opt('required_capability', to_jsonb(p_capability)));
END
$record_denied$;

-- Resolves the session principal and checks DB role + Audit capability.
-- Records audit.access.denied and returns ok = false when refused.
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
-- rejection code (audit-core names). The relay validated the full catalog.
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
           AND coalesce(v_prov ->> 'source_commitment' !~ '^[0-9a-f]{64}$', TRUE))
       OR (p ->> 'source' = 'urn:knowledge-platform:document-platform'
           AND NOT v_prov ? 'source_commitment') THEN
        RETURN 'invalid_provenance';
    END IF;
    RETURN NULL;
END
$relay_envelope_problem$;

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
    v_commitment BYTEA;
    v_digest BYTEA;
    e audit_store.events;
    v_kind TEXT;
    v_new BIGINT;
BEGIN
    h := audit_store.lock_head();
    v_problem := audit_store.gate_code(h);
    IF v_problem IS NOT NULL THEN
        RETURN QUERY SELECT 'recovery_required'::text, NULL::bigint, NULL::bytea,
                            NULL::integer, v_problem;
        RETURN;
    END IF;
    v_problem := audit_store.relay_envelope_problem(p_envelope);
    IF v_problem IS NOT NULL THEN
        RETURN QUERY SELECT 'rejected'::text, NULL::bigint, NULL::bytea, NULL::integer,
                            v_problem;
        RETURN;
    END IF;
    v_id := (p_envelope ->> 'id')::uuid;
    v_source := p_envelope ->> 'source';
    v_type := p_envelope ->> 'type';
    v_adapter := audit_store.jint(p_envelope -> 'data' -> 'provenance' -> 'adapter_version');
    v_commitment := decode(p_envelope -> 'data' -> 'provenance' ->> 'source_commitment', 'hex');
    IF NOT EXISTS (
        SELECT 1 FROM audit_store.registered_types AS r
        WHERE r.source = v_source AND r.event_type = v_type AND r.adapter_version = v_adapter
    ) THEN
        -- Catalog and Store migration disagree: an outage, never a verdict.
        RETURN QUERY SELECT 'outage'::text, NULL::bigint, NULL::bytea, NULL::integer,
                            'unregistered_type'::text;
        RETURN;
    END IF;
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

    v_new := audit_store.append_event('relay', p_envelope, v_commitment, v_adapter);
    RETURN QUERY SELECT 'stored'::text, v_new, v_digest, v_adapter, NULL::text;
END
$ingest$;

-- Admission probe for the relay circuit breaker (design §6.2).
CREATE FUNCTION audit_store.probe()
RETURNS TABLE (status TEXT, head_seq BIGINT, recovery_epoch BIGINT, writable BOOLEAN,
               code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '2s' AS $probe$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_code TEXT;
BEGIN
    IF current_setting('transaction_read_only') = 'on' OR pg_is_in_recovery() THEN
        SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
        RETURN QUERY SELECT 'read_only'::text, h.last_seq, h.recovery_epoch, FALSE,
                            'store_read_only'::text;
        RETURN;
    END IF;
    h := audit_store.lock_head();
    v_code := audit_store.gate_code(h);
    RETURN QUERY SELECT CASE WHEN v_code IS NULL THEN 'ok'
                             WHEN v_code = 'store_posture_invalid' THEN 'posture_invalid'
                             ELSE 'recovery_required' END::text,
                        h.last_seq, h.recovery_epoch, v_code IS NULL, v_code;
END
$probe$;

-- Content-free status for health (design §10.4, §12). Not audited.
CREATE FUNCTION audit_store.store_status()
RETURNS TABLE (head_seq BIGINT, recovery_epoch BIGINT, recovery_mode BOOLEAN,
               posture_ok BOOLEAN, last_verified_seq BIGINT, last_verified_at TIMESTAMPTZ,
               last_verified_outcome TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $store_status$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    fp record;
BEGIN
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
    SELECT * INTO fp FROM audit_store.current_fingerprint();
    RETURN QUERY
    SELECT h.last_seq, h.recovery_epoch,
           h.recovery_pending
             OR (fp.system_identifier, fp.database_oid, fp.timeline)
                IS DISTINCT FROM (h.fp_system_identifier, h.fp_database_oid, h.fp_timeline),
           NOT EXISTS (SELECT 1 FROM audit_store.posture_check()),
           v.seq, v.stored_at, v.outcome
    FROM (SELECT NULL::bigint AS seq, NULL::timestamptz AS stored_at, NULL::text AS outcome
          WHERE NOT EXISTS (
              SELECT 1 FROM audit_store.events AS e
              WHERE e.origin = 'store' AND e.event_type = 'audit.integrity.verified')
          UNION ALL
          (SELECT e.seq, e.stored_at, b.envelope -> 'data' -> 'details' ->> 'outcome'
           FROM audit_store.events AS e
           LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq
           WHERE e.origin = 'store' AND e.event_type = 'audit.integrity.verified'
           ORDER BY e.seq DESC LIMIT 1)) AS v;
END
$store_status$;

-- Content-free receipts for the relay (design §10.4). No control event.
CREATE FUNCTION audit_store.lookup_receipts(p_event_ids UUID[])
RETURNS TABLE (event_id UUID, seq BIGINT, origin TEXT, event_type TEXT,
               envelope_digest BYTEA, source_commitment BYTEA, adapter_version INTEGER,
               expired BOOLEAN)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lookup_receipts$
#variable_conflict use_column
BEGIN
    IF coalesce(cardinality(p_event_ids), 0) > 1000 THEN
        RAISE EXCEPTION 'lookup_receipts: at most 1000 ids' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.event_id, e.seq, e.origin, e.event_type, e.envelope_digest, e.source_commitment,
           e.adapter_version, e.expired_at IS NOT NULL
    FROM audit_store.events AS e
    WHERE e.event_id = ANY (p_event_ids)
    ORDER BY e.seq;
END
$lookup_receipts$;

CREATE FUNCTION audit_store.list_source_receipts(
    p_source TEXT, p_after_seq BIGINT, p_limit INTEGER)
RETURNS TABLE (event_id UUID, seq BIGINT, origin TEXT, event_type TEXT,
               envelope_digest BYTEA, source_commitment BYTEA, adapter_version INTEGER,
               expired BOOLEAN)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $list_source_receipts$
#variable_conflict use_column
BEGIN
    IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'list_source_receipts: limit must be 1..1000' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.event_id, e.seq, e.origin, e.event_type, e.envelope_digest, e.source_commitment,
           e.adapter_version, e.expired_at IS NOT NULL
    FROM audit_store.events AS e
    WHERE e.origin = 'relay' AND e.source = p_source AND e.seq > coalesce(p_after_seq, 0)
    ORDER BY e.seq
    LIMIT p_limit;
END
$list_source_receipts$;

CREATE FUNCTION audit_store.lookup_control_receipts(p_seqs BIGINT[])
RETURNS TABLE (seq BIGINT, event_id UUID, origin TEXT, event_type TEXT,
               envelope_digest BYTEA, target_event_id UUID)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $lookup_control_receipts$
#variable_conflict use_column
BEGIN
    IF coalesce(cardinality(p_seqs), 0) > 1000 THEN
        RAISE EXCEPTION 'lookup_control_receipts: at most 1000 seqs' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    SELECT e.seq, e.event_id, e.origin, e.event_type, e.envelope_digest,
           CASE WHEN audit_store.is_uuid_text(b.envelope -> 'data' -> 'details' ->> 'event_id')
                THEN (b.envelope -> 'data' -> 'details' ->> 'event_id')::uuid END
    FROM audit_store.events AS e
    LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq
    WHERE e.seq = ANY (p_seqs) AND e.origin IN ('store', 'relay_control')
    ORDER BY e.seq;
END
$lookup_control_receipts$;

-- ---------------------------------------------------------------------------
-- Relay control events (design §4.5, §6.4, §12)
-- ---------------------------------------------------------------------------

CREATE FUNCTION audit_store.record_relay_control(
    p_type TEXT, p_event_id UUID, p_code TEXT, p_counts JSONB)
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $record_relay_control$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_actor record;
    v_details JSONB;
    v_class TEXT;
    v_result TEXT;
    v_keys TEXT[];
    v_count_keys CONSTANT TEXT[] := ARRAY[
        'delivered_missing', 'digest_mismatch', 'id_set_digest', 'ok', 'pending',
        'quarantined', 'quarantined_conflict', 'quarantined_stored',
        'repaired_delivered_missing', 'repaired_quarantined_stored', 'repaired_unregistered',
        'source_tampered', 'store_only', 'unaudited_replay', 'unregistered', 'watermark'];
    k TEXT;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_actor FROM audit_store.session_actor();
    IF NOT v_actor.bound THEN
        PERFORM audit_store.record_denied('record_relay_control', 'unbound', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'unbound'::text;
        RETURN;
    END IF;
    IF p_event_id IS NULL THEN
        v_details := NULL;
    ELSIF p_type = 'audit.delivery.replay_requested'
          AND audit_store.is_code(p_code)
          AND (p_counts IS NULL OR p_counts = '{}'::jsonb) THEN
        v_class := 'PRIVILEGED_OPERATION';
        v_result := 'success';
        v_details := jsonb_build_object('event_id', p_event_id::text, 'quarantine_code', p_code);
    ELSIF p_type = 'audit.integrity.source_mismatch_detected'
          AND p_code IN ('source_digest_mismatch', 'actor_mismatch')
          AND (p_counts IS NULL OR p_counts = '{}'::jsonb) THEN
        v_class := 'SECURITY';
        v_result := 'failure';
        v_details := jsonb_build_object('event_id', p_event_id::text, 'mismatch_code', p_code);
    ELSIF p_type = 'audit.reconciliation.completed'
          AND p_code IN ('read_only', 'repair')
          AND jsonb_typeof(p_counts) = 'object' THEN
        SELECT array_agg(x ORDER BY x) INTO v_keys FROM jsonb_object_keys(p_counts) AS x;
        IF v_keys IS NOT DISTINCT FROM v_count_keys
           AND coalesce(p_counts ->> 'id_set_digest' ~ '^[0-9a-f]{64}$', FALSE) THEN
            v_details := jsonb_build_object(
                'run_id', p_event_id::text,
                'mode', p_code,
                'id_set_digest', p_counts ->> 'id_set_digest');
            FOREACH k IN ARRAY v_count_keys LOOP
                CONTINUE WHEN k = 'id_set_digest';
                IF audit_store.jint(p_counts -> k) IS NULL OR audit_store.jint(p_counts -> k) < 0 THEN
                    v_details := NULL;
                    EXIT;
                END IF;
                v_details := v_details || jsonb_build_object(
                    CASE WHEN k = 'watermark' OR k LIKE 'repaired_%' THEN k ELSE 'count_' || k END,
                    audit_store.jint(p_counts -> k));
            END LOOP;
            v_class := 'SYSTEM_AUDIT';
            v_result := 'success';
        END IF;
    END IF;
    IF v_details IS NULL THEN
        PERFORM audit_store.record_denied('record_relay_control', 'invalid_input', NULL);
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, 'invalid_input'::text;
        RETURN;
    END IF;
    v_seq := audit_store.append_control('relay_control', p_type, v_class, v_result, v_details);
    RETURN QUERY SELECT 'recorded'::text, v_seq, NULL::text;
END
$record_relay_control$;

-- ---------------------------------------------------------------------------
-- Two-phase disclosure (design §10.3)
-- ---------------------------------------------------------------------------

-- Validates and normalizes a filter. NULL when invalid. verify/identity_chain
-- read the contiguous chain and accept only seq_after.
CREATE FUNCTION audit_store.normalize_filter(p_filter JSONB, p_operation TEXT)
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
    v_allowed := CASE WHEN p_operation IN ('verify', 'identity_chain') THEN ARRAY['seq_after']
                      ELSE ARRAY['actor', 'event_ids', 'event_types', 'occurred_from',
                                 'occurred_to', 'resource', 'seq_after', 'source'] END;
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
        IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(f -> 'event_types') AS t
                   WHERE octet_length(t) > 128
                      OR t !~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$') THEN
            RETURN NULL;
        END IF;
        SELECT array_agg(DISTINCT t ORDER BY t) INTO v_types
        FROM jsonb_array_elements_text(f -> 'event_types') AS t;
        v_out := v_out || jsonb_build_object('event_types', to_jsonb(v_types));
    END IF;
    IF f ? 'source' THEN
        IF jsonb_typeof(f -> 'source') <> 'string'
           OR NOT audit_store.is_identifier(f ->> 'source') THEN
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
           OR NOT audit_store.is_identifier(f -> 'actor' ->> 'issuer')
           OR NOT audit_store.is_identifier(f -> 'actor' ->> 'principal_id') THEN
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
                    OR NOT audit_store.is_identifier(f -> 'resource' ->> 'id'))) THEN
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
    RETURN v_out;
END
$normalize_filter$;

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
    SELECT * INTO v_auth FROM audit_store.authorize(p_operation, v_role, v_capability);
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::text, NULL::bigint, NULL::bigint,
                            NULL::timestamptz, NULL::boolean, v_auth.denial;
        RETURN;
    END IF;
    v_filter := audit_store.normalize_filter(p_filter, p_operation);
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
        || audit_store.opt('filter_seq_after', v_filter -> 'seq_after'));
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
-- raises (design §10.3). Never writes.
CREATE FUNCTION audit_store.resolve_intent(p_token TEXT)
RETURNS audit_store.access_intents LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $resolve_intent$
DECLARE
    ai audit_store.access_intents;
    e audit_store.events;
    v_body JSONB;
    d JSONB;
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
    v_role := CASE WHEN ai.operation IN ('investigate', 'export') THEN 'audit_store_reader'
                   ELSE 'audit_store_verifier' END;
    v_capability := CASE WHEN ai.operation IN ('investigate', 'export') THEN ai.operation
                         ELSE 'verify' END;
    SELECT * INTO v_actor FROM audit_store.session_actor();
    IF NOT v_actor.bound
       OR v_actor.issuer <> ai.issuer OR v_actor.principal_id <> ai.principal_id
       OR NOT pg_has_role(session_user, v_role, 'MEMBER')
       OR NOT audit_store.has_grant(ai.issuer, ai.principal_id, v_capability) THEN
        RAISE EXCEPTION 'access_revoked' USING ERRCODE = '42501';
    END IF;
    -- The intent row must equal the intent event in the chain.
    SELECT * INTO e FROM audit_store.events AS x WHERE x.seq = ai.intent_seq;
    SELECT b.envelope INTO v_body FROM audit_store.event_bodies AS b WHERE b.seq = ai.intent_seq;
    d := v_body -> 'data' -> 'details';
    IF e.seq IS NULL OR v_body IS NULL
       OR e.origin <> 'store' OR e.event_type <> 'audit.access.intent_opened'
       OR audit_store.jsonb_digest(v_body) <> e.envelope_digest
       OR audit_store.chain_step(e.prev_chain, e.seq, e.event_id, e.envelope_digest) <> e.chain
       OR audit_store.jsonb_digest(ai.filter) <> ai.filter_digest
       OR d ->> 'filter_digest' IS DISTINCT FROM encode(ai.filter_digest, 'hex')
       OR d ->> 'token_digest' IS DISTINCT FROM encode(ai.token_digest, 'hex')
       OR d ->> 'session_role' IS DISTINCT FROM ai.session_role
       OR d ->> 'operation' IS DISTINCT FROM ai.operation
       OR audit_store.jint(d -> 'watermark') IS DISTINCT FROM ai.watermark
       OR audit_store.jint(d -> 'page_size') IS DISTINCT FROM ai.page_size::bigint
       OR audit_store.jint(d -> 'max_pages') IS DISTINCT FROM ai.max_pages::bigint
       OR d -> 'include_control' IS DISTINCT FROM to_jsonb(ai.include_control)
       OR d ->> 'expires_at' IS DISTINCT FROM audit_store.utc_text(ai.expires_at)
       OR e.actor_issuer <> ai.issuer OR e.actor_principal_id <> ai.principal_id THEN
        RAISE EXCEPTION 'intent_integrity_violation' USING ERRCODE = '42501';
    END IF;
    RETURN ai;
END
$resolve_intent$;

-- Returns at most page_size export lines of the bounded set
-- (filter ∩ seq <= W ∩ visibility, first max_pages * page_size rows by seq).
-- Read only; refusals are errors and are not recorded (they disclose nothing).
CREATE FUNCTION audit_store.read_page(p_token TEXT, p_after_seq BIGINT)
RETURNS TABLE (seq BIGINT, line TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $read_page$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    ai audit_store.access_intents;
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
    v_bodies BOOLEAN;
BEGIN
    -- The current transaction must not have written anything, even inside a
    -- savepoint that was rolled back or released.
    IF pg_current_xact_id_if_assigned() IS NOT NULL THEN
        RAISE EXCEPTION 'read_page_requires_clean_transaction' USING ERRCODE = '42501';
    END IF;
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
    PERFORM audit_store.require_gate(h);
    ai := audit_store.resolve_intent(p_token);
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
    v_seq_after := audit_store.jint(ai.filter -> 'seq_after');
    v_bodies := ai.operation <> 'identity_chain';
    RETURN QUERY
    WITH bound AS (
        SELECT e.seq FROM audit_store.events AS e
        WHERE e.seq <= ai.watermark
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
          AND (v_seq_after IS NULL OR e.seq > v_seq_after)
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

-- Scans seq p_from..p_to in one snapshot (STABLE) and counts violations by
-- code. p_head_check compares the head row when p_to is the head.
CREATE FUNCTION audit_store.integrity_scan(p_from BIGINT, p_to BIGINT, p_head_check BOOLEAN)
RETURNS TABLE (checked BIGINT, head_epoch BIGINT, head_chain BYTEA, violations JSONB)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $integrity_scan$
#variable_conflict use_column
DECLARE
    v_base_chain BYTEA;
    v_base_epoch BIGINT;
    v_checked BIGINT;
    v_max BIGINT;
    v_head_chain BYTEA;
    v_head_epoch BIGINT;
    v JSONB;
    v_missing BIGINT;
    v_retention_missing BIGINT;
    v_retention_mismatch BIGINT;
    v_purge_mismatch BIGINT;
    v_head_mismatch BIGINT := 0;
    h audit_store.publication_head;
BEGIN
    IF p_from < 1 OR p_to < p_from - 1 THEN
        RAISE EXCEPTION 'integrity_scan: invalid range' USING ERRCODE = '22023';
    END IF;
    IF p_from = 1 THEN
        v_base_chain := audit_store.genesis();
        v_base_epoch := 1;
    ELSE
        SELECT e.chain, e.recovery_epoch INTO v_base_chain, v_base_epoch
        FROM audit_store.events AS e WHERE e.seq = p_from - 1;
    END IF;

    WITH r AS (
        SELECT e.*, b.envelope,
               lag(e.seq) OVER w AS lag_seq,
               lag(e.chain) OVER w AS lag_chain,
               lag(e.recovery_epoch) OVER w AS lag_epoch
        FROM audit_store.events AS e
        LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq
        WHERE e.seq BETWEEN p_from AND p_to
        WINDOW w AS (ORDER BY e.seq)
    ),
    c AS (
        SELECT
            r.seq,
            r.chain,
            r.recovery_epoch,
            r.seq <> coalesce(r.lag_seq + 1, p_from) AS seq_gap,
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
    v_missing := CASE WHEN p_to < p_from THEN 0
                      WHEN v_max IS NULL THEN p_to - p_from + 1
                      ELSE p_to - v_max END;
    v := jsonb_set(v, '{seq_gap}', to_jsonb(audit_store.jint(v -> 'seq_gap') + v_missing));

    IF p_to >= p_from THEN
        SELECT e.chain, e.recovery_epoch INTO v_head_chain, v_head_epoch
        FROM audit_store.events AS e WHERE e.seq = p_to;
    ELSE
        v_head_chain := v_base_chain;
        v_head_epoch := v_base_epoch;
    END IF;

    -- Retention evidence (set semantics). Marks in range point to a later
    -- origin=store event whose body must exist.
    SELECT count(*) INTO v_retention_missing
    FROM audit_store.events AS t
    LEFT JOIN audit_store.events AS ev ON ev.seq = t.expired_by_seq
    LEFT JOIN audit_store.event_bodies AS evb ON evb.seq = t.expired_by_seq
    WHERE t.seq BETWEEN p_from AND p_to
      AND t.expired_by_seq IS NOT NULL
      AND (ev.seq IS NULL OR evb.seq IS NULL OR ev.origin <> 'store' OR ev.seq <= t.seq
           OR ev.event_type NOT IN ('audit.retention.expired', 'audit.body.purged'));

    -- Evidence events referenced from the range or located in it.
    WITH evidence AS (
        SELECT DISTINCT t.expired_by_seq AS seq FROM audit_store.events AS t
        WHERE t.seq BETWEEN p_from AND p_to AND t.expired_by_seq IS NOT NULL
        UNION
        SELECT e.seq FROM audit_store.events AS e
        WHERE e.seq BETWEEN p_from AND p_to AND e.origin = 'store'
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
               sha256(coalesce(string_agg(int8send(m.seq), ''::bytea ORDER BY m.seq),
                               ''::bytea)) AS set_digest,
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
            AND (s.n = 0 OR (
                s.d ->> 'outcome' = 'expired'
                AND s.rows_match
                AND audit_store.jint(s.d -> 'first_seq') IS NOT DISTINCT FROM s.first_seq
                AND audit_store.jint(s.d -> 'last_seq') IS NOT DISTINCT FROM s.last_seq)))),
        count(*) FILTER (WHERE s.event_type = 'audit.body.purged' AND NOT (
            s.n = 1
            AND audit_store.jint(s.d -> 'target_seq') IS NOT DISTINCT FROM s.first_seq
            AND s.d ->> 'target_event_id' IS NOT DISTINCT FROM s.only_event_id))
    INTO v_retention_mismatch, v_purge_mismatch
    FROM sets AS s;

    IF p_head_check THEN
        SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
        IF h.last_seq <> p_to
           OR h.last_chain IS DISTINCT FROM v_head_chain
           OR EXISTS (SELECT 1 FROM audit_store.events AS e WHERE e.seq > p_to) THEN
            v_head_mismatch := 1;
        END IF;
    END IF;

    v := v || jsonb_build_object(
        'retention_evidence_missing', v_retention_missing,
        'retention_evidence_mismatch', v_retention_mismatch,
        'purge_evidence_mismatch', v_purge_mismatch,
        'head_mismatch', v_head_mismatch);
    RETURN QUERY SELECT v_checked, coalesce(v_head_epoch, v_base_epoch, 1::bigint),
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

-- Records audit.integrity.verified for a scan result.
CREATE FUNCTION audit_store.record_verified(
    p_trigger TEXT, p_from BIGINT, p_to BIGINT, p_watermark BIGINT, p_checked BIGINT,
    p_head_epoch BIGINT, p_head_chain BYTEA, p_violations JSONB)
RETURNS BIGINT LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $record_verified$
DECLARE
    v_total BIGINT := audit_store.violations_total(p_violations);
    v_details JSONB;
    k TEXT;
BEGIN
    v_details := jsonb_build_object(
        'trigger', p_trigger,
        'from_seq', p_from,
        'to_seq', p_to,
        'watermark', p_watermark,
        'checked', p_checked,
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

-- In-database verification of p_from..p_to (bounded by the head W at start),
-- recorded as one audit.integrity.verified after W (no recursion).
CREATE FUNCTION audit_store.verify(p_from BIGINT, p_to BIGINT)
RETURNS TABLE (status TEXT, seq BIGINT, outcome TEXT, checked BIGINT, violations JSONB,
               to_seq BIGINT, head_epoch BIGINT, head_chain TEXT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s' AS $verify$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_auth record;
    v_from BIGINT;
    v_to BIGINT;
    s record;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('integrity_verify', 'audit_store_verifier',
                                                    'verify');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::text, NULL::bigint, NULL::jsonb,
                            NULL::bigint, NULL::bigint, NULL::text, v_auth.denial;
        RETURN;
    END IF;
    v_from := coalesce(p_from, 1);
    v_to := least(coalesce(p_to, h.last_seq), h.last_seq);
    IF v_from < 1 OR v_to < v_from - 1 THEN
        PERFORM audit_store.record_denied('integrity_verify', 'invalid_input', 'verify');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::text, NULL::bigint, NULL::jsonb,
                            NULL::bigint, NULL::bigint, NULL::text, 'invalid_input'::text;
        RETURN;
    END IF;
    SELECT * INTO s FROM audit_store.integrity_scan(v_from, v_to, v_to = h.last_seq);
    v_seq := audit_store.record_verified('verify', v_from, v_to, h.last_seq, s.checked,
                                         s.head_epoch, s.head_chain, s.violations);
    RETURN QUERY SELECT 'verified'::text, v_seq,
                        CASE WHEN audit_store.violations_total(s.violations) = 0 THEN 'ok'
                             ELSE 'violations' END,
                        s.checked, s.violations, v_to, s.head_epoch, encode(s.head_chain, 'hex'),
                        NULL::text;
END
$verify$;

-- Verifies 1..W and returns (epoch, W, chain) for the out-of-band checkpoint
-- log; recorded as audit.integrity.verified with trigger=checkpoint.
CREATE FUNCTION audit_store.checkpoint()
RETURNS TABLE (status TEXT, epoch BIGINT, seq BIGINT, chain TEXT, verified_seq BIGINT,
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
BEGIN
    h := audit_store.lock_head();
    PERFORM audit_store.require_gate(h);
    SELECT * INTO v_auth FROM audit_store.authorize('checkpoint', 'audit_store_verifier',
                                                    'verify');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::text,
                            NULL::bigint, NULL::text, v_auth.denial;
        RETURN;
    END IF;
    SELECT * INTO s FROM audit_store.integrity_scan(1, h.last_seq, TRUE);
    v_ok := audit_store.violations_total(s.violations) = 0;
    v_seq := audit_store.record_verified('checkpoint', 1, h.last_seq, h.last_seq, s.checked,
                                         s.head_epoch, s.head_chain, s.violations);
    RETURN QUERY SELECT CASE WHEN v_ok THEN 'checkpoint' ELSE 'violations' END::text,
                        s.head_epoch, h.last_seq, encode(s.head_chain, 'hex'), v_seq,
                        CASE WHEN v_ok THEN 'ok' ELSE 'violations' END::text, NULL::text;
END
$checkpoint$;

-- Read-only verification that also works in recovery mode. Records nothing.
CREATE FUNCTION audit_store.verify_recovery()
RETURNS TABLE (outcome TEXT, checked BIGINT, head_seq BIGINT, head_epoch BIGINT,
               head_chain TEXT, recovery_mode BOOLEAN, violations JSONB)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $verify_recovery$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    v_max BIGINT;
    s record;
BEGIN
    SELECT * INTO STRICT h FROM audit_store.publication_head AS p WHERE p.singleton;
    SELECT coalesce(max(e.seq), 0) INTO v_max FROM audit_store.events AS e;
    SELECT * INTO s FROM audit_store.integrity_scan(1, v_max, TRUE);
    RETURN QUERY SELECT CASE WHEN audit_store.violations_total(s.violations) = 0 THEN 'ok'
                             ELSE 'violations' END::text,
                        s.checked, v_max, s.head_epoch, encode(s.head_chain, 'hex'),
                        audit_store.gate_code(h) IS NOT DISTINCT FROM 'store_recovery_required',
                        s.violations;
END
$verify_recovery$;

-- Content-free identity chain page; works in recovery mode, records nothing.
CREATE FUNCTION audit_store.identity_chain_recovery_page(p_after_seq BIGINT, p_limit INTEGER)
RETURNS TABLE (seq BIGINT, line TEXT)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $identity_chain_recovery_page$
#variable_conflict use_column
BEGIN
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
    IF NOT audit_store.is_identifier(p_issuer) OR NOT audit_store.is_identifier(p_principal_id)
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
    PERFORM audit_store.require_gate(h);
    IF NOT audit_store.bindable_role(p_db_role)
       OR NOT audit_store.is_identifier(p_issuer)
       OR NOT audit_store.is_identifier(p_principal_id) THEN
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
    PERFORM audit_store.require_gate(h);
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
    PERFORM audit_store.require_gate(h);
    IF EXISTS (SELECT 1 FROM audit_store.access_grants AS g
               WHERE g.capability = 'administer' AND g.revoked_seq IS NULL) THEN
        RETURN QUERY SELECT 'refused'::text, NULL::bigint, 'administrator_exists'::text;
        RETURN;
    END IF;
    IF NOT audit_store.bindable_role(p_db_role)
       OR NOT audit_store.is_identifier(p_issuer)
       OR NOT audit_store.is_identifier(p_principal_id) THEN
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

-- ---------------------------------------------------------------------------
-- Retention (design §9)
-- ---------------------------------------------------------------------------

-- Validates and normalizes a selector; NULL when invalid. Lists are sorted
-- and de-duplicated; at least one dimension is required.
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
        IF jsonb_array_length(p_selector -> k) NOT BETWEEN 1 AND 32
           OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_selector -> k) AS t
                      WHERE jsonb_typeof(t) <> 'string') THEN
            RETURN NULL;
        END IF;
        IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(p_selector -> k) AS t
                   WHERE NOT audit_store.is_identifier(t)
                      OR (k = 'event_types' AND t LIKE 'audit.%')
                      OR (k = 'event_classes' AND t NOT IN (
                          'SECURITY', 'PRIVILEGED_OPERATION', 'CONTENT_LIFECYCLE',
                          'ACCESS_POLICY', 'DATA_ACCESS', 'SEARCH_ACCESS',
                          'CONFIGURATION', 'SYSTEM_AUDIT'))
                      OR (k = 'sources' AND t IN (
                          'urn:knowledge-platform:audit-store',
                          'urn:knowledge-platform:audit-relay'))) THEN
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
    v_outcome TEXT;
    v_effective TIMESTAMPTZ;
    v_targets BIGINT[] := ARRAY[]::bigint[];
    v_set_digest BYTEA;
    v_details JSONB;
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
    IF NOT audit_store.is_code(p_policy_id) OR p_expected_revision IS NULL
       OR p_expected_revision < 1 OR p_cutoff IS NULL OR NOT isfinite(p_cutoff)
       OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN
        PERFORM audit_store.record_denied('expire', 'invalid_input', 'maintain');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::timestamptz,
                            'invalid_input'::text;
        RETURN;
    END IF;
    SELECT * INTO p FROM audit_store.retention_policies AS x
    WHERE x.policy_id = p_policy_id ORDER BY x.revision DESC LIMIT 1;
    IF p.revision IS NULL OR p.revision <> p_expected_revision THEN
        v_outcome := 'stale_revision';
    ELSIF p.retain_days IS NULL THEN
        v_outcome := 'not_expirable';
    ELSIF EXISTS (SELECT 1 FROM audit_store.legal_holds AS l WHERE l.released_at IS NULL) THEN
        v_outcome := 'held';
    ELSE
        v_outcome := 'expired';
        v_effective := least(p_cutoff,
                             transaction_timestamp() - make_interval(days => p.retain_days));
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
    END IF;
    SELECT sha256(coalesce(string_agg(int8send(x), ''::bytea ORDER BY x), ''::bytea))
    INTO v_set_digest FROM unnest(v_targets) AS x;
    v_details := jsonb_build_object(
            'outcome', v_outcome,
            'policy_id', p_policy_id,
            'expected_revision', p_expected_revision,
            'current_revision', to_jsonb(p.revision),
            'requested_cutoff', audit_store.utc_text(p_cutoff),
            'transaction_time', audit_store.utc_text(transaction_timestamp()),
            'limit', p_limit,
            'count', cardinality(v_targets),
            'expired_set_digest', encode(v_set_digest, 'hex'))
        || CASE WHEN p.revision IS NULL THEN '{}'::jsonb ELSE
               jsonb_build_object('selector_digest', encode(p.selector_digest, 'hex'),
                                  'retain_days', to_jsonb(p.retain_days)) END
        || audit_store.opt('selector_event_types', p.selector -> 'event_types')
        || audit_store.opt('selector_event_classes', p.selector -> 'event_classes')
        || audit_store.opt('selector_sources', p.selector -> 'sources')
        || audit_store.opt('effective_cutoff', to_jsonb(audit_store.utc_text(v_effective)))
        || audit_store.opt('first_seq', to_jsonb(v_targets[1]))
        || audit_store.opt('last_seq', to_jsonb(v_targets[cardinality(v_targets)]));
    -- The evidence is recorded first; its seq marks the expired rows.
    v_seq := audit_store.append_control('store', 'audit.retention.expired',
        'PRIVILEGED_OPERATION', CASE WHEN v_outcome = 'expired' THEN 'success' ELSE 'failure' END,
        v_details);
    IF cardinality(v_targets) > 0 THEN
        UPDATE audit_store.events AS e
        SET expired_at = transaction_timestamp(), expired_by_seq = v_seq
        WHERE e.seq = ANY (v_targets);
        DELETE FROM audit_store.event_bodies AS b WHERE b.seq = ANY (v_targets);
    END IF;
    RETURN QUERY SELECT v_outcome, v_seq, cardinality(v_targets)::bigint, v_effective,
                        CASE WHEN v_outcome = 'expired' THEN NULL ELSE v_outcome END;
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

CREATE FUNCTION audit_store.begin_recovery_epoch(
    p_checkpoint_epoch BIGINT, p_checkpoint_seq BIGINT, p_checkpoint_chain TEXT,
    p_relay_max_seq BIGINT)
RETURNS TABLE (status TEXT, seq BIGINT, new_epoch BIGINT, restored_head BIGINT,
               classification TEXT, code TEXT)
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
    v_reason TEXT;
    v_head BIGINT;
    s record;
    v_chain_at BYTEA;
    v_class TEXT;
    v_lost_to BIGINT;
    v_identity BYTEA;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    SELECT * INTO fp FROM audit_store.current_fingerprint();
    SELECT coalesce(max(e.seq), 0) INTO v_head FROM audit_store.events AS e;
    IF (fp.system_identifier, fp.database_oid, fp.timeline)
        IS DISTINCT FROM (h.fp_system_identifier, h.fp_database_oid, h.fp_timeline) THEN
        v_reason := 'fingerprint_mismatch';
    ELSIF h.recovery_pending THEN
        v_reason := 'recovery_pending';
    ELSIF coalesce(p_relay_max_seq, 0) > v_head THEN
        v_reason := 'regressed';
    ELSE
        RAISE EXCEPTION 'not_in_recovery' USING ERRCODE = '55000';
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.posture_check()) THEN
        RAISE EXCEPTION 'store_posture_invalid' USING ERRCODE = 'KA002';
    END IF;
    SELECT * INTO v_auth FROM audit_store.authorize('begin_recovery_epoch',
                                                    'audit_store_maintainer', 'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::bigint,
                            NULL::text, v_auth.denial;
        RETURN;
    END IF;
    IF p_checkpoint_epoch IS NULL OR p_checkpoint_epoch < 1
       OR p_checkpoint_seq IS NULL OR p_checkpoint_seq < 0
       OR p_checkpoint_chain IS NULL OR p_checkpoint_chain !~ '^[0-9a-f]{64}$'
       OR coalesce(p_relay_max_seq, 0) < 0 THEN
        PERFORM audit_store.record_denied('begin_recovery_epoch', 'invalid_input', 'maintain');
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, NULL::bigint, NULL::bigint,
                            NULL::text, 'invalid_input'::text;
        RETURN;
    END IF;
    -- Re-verify the restored chain under the head lock.
    SELECT * INTO s FROM audit_store.integrity_scan(1, v_head, FALSE);
    IF audit_store.violations_total(s.violations) > 0 THEN
        RAISE EXCEPTION 'restored_chain_invalid' USING ERRCODE = '55000';
    END IF;
    IF p_checkpoint_seq > v_head THEN
        v_class := 'store_behind';
    ELSE
        v_chain_at := CASE WHEN p_checkpoint_seq = 0 THEN audit_store.genesis() ELSE
            (SELECT e.chain FROM audit_store.events AS e WHERE e.seq = p_checkpoint_seq) END;
        v_class := CASE WHEN v_chain_at <> decode(p_checkpoint_chain, 'hex') THEN 'mismatch'
                        WHEN p_checkpoint_seq = v_head THEN 'match'
                        ELSE 'ahead' END;
    END IF;
    v_lost_to := greatest(p_checkpoint_seq, coalesce(p_relay_max_seq, 0), v_head);
    SELECT sha256(coalesce(string_agg(int8send(e.seq) || uuid_send(e.event_id)
                                      || e.envelope_digest || e.chain, ''::bytea ORDER BY e.seq),
                           ''::bytea))
    INTO v_identity FROM audit_store.events AS e WHERE e.seq <= v_head;
    UPDATE audit_store.publication_head AS x
    SET last_seq = v_head, last_chain = s.head_chain, recovery_epoch = h.recovery_epoch + 1,
        fp_system_identifier = fp.system_identifier, fp_database_oid = fp.database_oid,
        fp_timeline = fp.timeline, recovery_pending = FALSE, updated_at = clock_timestamp()
    WHERE x.singleton;
    v_seq := audit_store.append_control('store', 'audit.recovery.epoch_started', 'SYSTEM_AUDIT',
        'success', jsonb_build_object(
            'reason_kind', v_reason,
            'old_epoch', h.recovery_epoch,
            'new_epoch', h.recovery_epoch + 1,
            'restored_head', v_head,
            'restored_head_chain', encode(s.head_chain, 'hex'),
            'recovery_identity_digest', encode(v_identity, 'hex'),
            'checkpoint_epoch', p_checkpoint_epoch,
            'checkpoint_seq', p_checkpoint_seq,
            'checkpoint_chain', p_checkpoint_chain,
            'checkpoint_classification', v_class,
            'relay_max_seq', coalesce(p_relay_max_seq, 0),
            'lost_after_seq', v_head,
            'lost_to_seq_claimed', v_lost_to,
            'old_system_identifier', h.fp_system_identifier,
            'old_database_oid', h.fp_database_oid::text,
            'old_timeline', h.fp_timeline,
            'new_system_identifier', fp.system_identifier,
            'new_database_oid', fp.database_oid::text,
            'new_timeline', fp.timeline));
    RETURN QUERY SELECT 'epoch_started'::text, v_seq, h.recovery_epoch + 1, v_head, v_class,
                        NULL::text;
END
$begin_recovery_epoch$;

-- Planned moves (pg_upgrade, dump/restore migration, planned promotion):
-- audited rebinding of the fingerprint without a new epoch.
CREATE FUNCTION audit_store.rebind_fingerprint()
RETURNS TABLE (status TEXT, seq BIGINT, code TEXT)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
SET lock_timeout = '5s'
SET audit_store.write_context = 'definer' AS $rebind_fingerprint$
#variable_conflict use_column
DECLARE
    h audit_store.publication_head;
    fp record;
    v_auth record;
    v_head BIGINT;
    s record;
    v_seq BIGINT;
BEGIN
    h := audit_store.lock_head();
    SELECT * INTO fp FROM audit_store.current_fingerprint();
    IF (fp.system_identifier, fp.database_oid, fp.timeline)
        IS NOT DISTINCT FROM (h.fp_system_identifier, h.fp_database_oid, h.fp_timeline) THEN
        RETURN QUERY SELECT 'unchanged'::text, NULL::bigint, NULL::text;
        RETURN;
    END IF;
    IF h.recovery_pending THEN
        RAISE EXCEPTION 'store_recovery_required' USING ERRCODE = 'KA001';
    END IF;
    IF EXISTS (SELECT 1 FROM audit_store.posture_check()) THEN
        RAISE EXCEPTION 'store_posture_invalid' USING ERRCODE = 'KA002';
    END IF;
    SELECT * INTO v_auth FROM audit_store.authorize('rebind_fingerprint',
                                                    'audit_store_maintainer', 'maintain');
    IF NOT v_auth.ok THEN
        RETURN QUERY SELECT 'denied'::text, NULL::bigint, v_auth.denial;
        RETURN;
    END IF;
    -- A planned move keeps every row: the head and the chain must be intact.
    SELECT coalesce(max(e.seq), 0) INTO v_head FROM audit_store.events AS e;
    SELECT * INTO s FROM audit_store.integrity_scan(1, v_head, TRUE);
    IF audit_store.violations_total(s.violations) > 0 THEN
        RAISE EXCEPTION 'store_recovery_required' USING ERRCODE = 'KA001';
    END IF;
    UPDATE audit_store.publication_head AS x
    SET fp_system_identifier = fp.system_identifier, fp_database_oid = fp.database_oid,
        fp_timeline = fp.timeline, updated_at = clock_timestamp()
    WHERE x.singleton;
    v_seq := audit_store.append_control('store', 'audit.recovery.fingerprint_rebound',
        'SYSTEM_AUDIT', 'success', jsonb_build_object(
            'epoch', h.recovery_epoch,
            'head_seq', v_head,
            'old_system_identifier', h.fp_system_identifier,
            'old_database_oid', h.fp_database_oid::text,
            'old_timeline', h.fp_timeline,
            'new_system_identifier', fp.system_identifier,
            'new_database_oid', fp.database_oid::text,
            'new_timeline', fp.timeline));
    RETURN QUERY SELECT 'rebound'::text, v_seq, NULL::text;
END
$rebind_fingerprint$;

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
-- backwards except when begin_recovery_epoch recomputes the restored head.
CREATE FUNCTION audit_store.guard_publication_head()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_publication_head$
BEGIN
    IF TG_OP = 'UPDATE'
       AND audit_store.in_context('audit_store.write_context', ARRAY['definer'])
       AND NEW.recovery_epoch >= OLD.recovery_epoch
       AND (NEW.last_seq >= OLD.last_seq
            OR audit_store.in_context('audit_store.maintenance_context', ARRAY['recovery'])) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_store.publication_head: % refused', TG_OP USING ERRCODE = '55000';
END
$guard_publication_head$;

-- Append-only tables written by definer functions (intents, policies).
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

-- legal_holds (reserved): rows are never updated or deleted.
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
