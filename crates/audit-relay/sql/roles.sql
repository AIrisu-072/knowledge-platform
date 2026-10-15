-- Audit relay capability roles on the Document database (design §10.1).
-- Template applied by the database owner (a superuser) after
-- `audit-relay migrate`. Idempotent: re-apply after adding a LOGIN role and
-- after every restore of the Document database.
--
-- Capability roles are NOLOGIN and hold no table privileges. Each service or
-- operator gets its own LOGIN role and is granted exactly one of them:
--   audit_relay_worker   `audit-relay run`, `health`, read-only `reconcile`
--   audit_relay_operator `audit-relay replay`, `reconcile --repair`
-- audit_relay.posture_check() reports drift from this matrix, any other
-- member of audit_relay_owner (revoke the membership a non-superuser
-- migrate used), and any capability login that can read the staging table
-- or touch the relay tables directly (grants, pg_read_all_data,
-- pg_write_all_data).

DO $roles$
DECLARE
    r TEXT;
BEGIN
    FOREACH r IN ARRAY ARRAY['audit_relay_worker', 'audit_relay_operator'] LOOP
        IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = r) THEN
            BEGIN
                EXECUTE format('CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE '
                               'NOREPLICATION NOBYPASSRLS', r);
            EXCEPTION WHEN duplicate_object THEN
                NULL;
            END;
        END IF;
    END LOOP;
END
$roles$;

REVOKE ALL ON SCHEMA audit_relay FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA audit_relay FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA audit_relay FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA audit_relay FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE audit_relay_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

GRANT USAGE ON SCHEMA audit_relay TO audit_relay_worker, audit_relay_operator;

-- Dispatcher (design §6.2) and its content-free reads.
GRANT EXECUTE ON FUNCTION
    audit_relay.claim(uuid, integer, bigint),
    audit_relay.renew(uuid, uuid, bigint),
    audit_relay.settle_success(uuid, uuid, bigint, bytea, text, bigint),
    audit_relay.settle_failure(uuid, uuid, text, boolean, bigint, boolean, boolean, boolean),
    audit_relay.reap_exhausted(integer),
    audit_relay.mismatch_seq(uuid, uuid, text),
    audit_relay.note_mismatch(uuid, uuid, text, bigint, bigint),
    -- `run` reports its circuit breaker for health (codes and counts).
    audit_relay.report_runtime(uuid, text, text, bigint, bigint, boolean),
    -- The health forecast projects staged content (as claim does).
    audit_relay.preview_pending(uuid, integer)
    TO audit_relay_worker;

-- Replay and repair transitions (design §6.4, §12).
GRANT EXECUTE ON FUNCTION
    audit_relay.replay(uuid, bigint, bigint),
    audit_relay.repair_ack_stored(uuid, bigint, bytea, bytea, text, bigint, bigint),
    audit_relay.repair_reset_missing(uuid, bigint, bigint, bigint),
    audit_relay.register_missing(integer)
    TO audit_relay_operator;

-- Status, policy, regression gate and content-free reconcile reads.
GRANT EXECUTE ON FUNCTION
    audit_relay.status(),
    audit_relay.policy(),
    audit_relay.acked_head(bigint),
    audit_relay.reconcile_page(uuid, integer),
    audit_relay.lookup_deliveries(uuid[]),
    audit_relay.reconcile_history_page(bigint, integer),
    audit_relay.posture_check()
    TO audit_relay_worker, audit_relay_operator;

-- Timeouts on every LOGIN role holding a capability role. statement_timeout
-- must be a role setting: a function-level SET cannot bound a running call.
DO $timeouts$
DECLARE
    v_db TEXT := current_database();
    r record;
BEGIN
    FOR r IN
        WITH RECURSIVE reach(member) AS (
            SELECT m.member FROM pg_catalog.pg_auth_members AS m
            JOIN pg_catalog.pg_roles AS c ON c.oid = m.roleid
            WHERE c.rolname IN ('audit_relay_worker', 'audit_relay_operator')
            UNION
            SELECT m.member FROM pg_catalog.pg_auth_members AS m
            JOIN reach ON m.roleid = reach.member
        )
        SELECT DISTINCT l.rolname FROM reach
        JOIN pg_catalog.pg_roles AS l ON l.oid = reach.member
        WHERE l.rolcanlogin
    LOOP
        EXECUTE format('ALTER ROLE %I IN DATABASE %I SET statement_timeout = %L',
                       r.rolname, v_db, '30s');
        EXECUTE format('ALTER ROLE %I IN DATABASE %I SET lock_timeout = %L',
                       r.rolname, v_db, '5s');
        EXECUTE format('ALTER ROLE %I IN DATABASE %I SET idle_in_transaction_session_timeout = %L',
                       r.rolname, v_db, '60s');
    END LOOP;
END
$timeouts$;
