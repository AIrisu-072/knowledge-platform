-- Audit Store database privileges (design §10.1, §11). Idempotent: apply as
-- the database owner after sql/roles.sql, after adding a LOGIN role, and
-- after every pg_restore (database ACLs and per-database role settings are
-- not part of a pg_dump of the database). posture_check() reports what is
-- still missing; begin_recovery_epoch refuses until it is clean.
--
-- Timeouts are set on each LOGIN role that holds a capability role, because
-- role settings apply only to the role that logs in. statement_timeout must
-- be a role setting: a function-level SET cannot bound a running call.
-- Adjust the values for the deployment; posture_check only requires them set.

DO $privileges$
DECLARE
    v_db TEXT := current_database();
    r record;
BEGIN
    EXECUTE format('REVOKE CONNECT, TEMPORARY ON DATABASE %I FROM PUBLIC', v_db);
    EXECUTE format('GRANT CONNECT ON DATABASE %I TO audit_store_owner, audit_store_ingest, '
                   'audit_store_relay_control, audit_store_reader, audit_store_verifier, '
                   'audit_store_admin, audit_store_maintainer', v_db);
    FOR r IN
        WITH RECURSIVE reach(member) AS (
            SELECT m.member FROM pg_catalog.pg_auth_members AS m
            JOIN pg_catalog.pg_roles AS c ON c.oid = m.roleid
            WHERE c.rolname IN ('audit_store_ingest', 'audit_store_relay_control',
                                'audit_store_reader', 'audit_store_verifier',
                                'audit_store_admin', 'audit_store_maintainer')
            UNION
            SELECT m.member FROM pg_catalog.pg_auth_members AS m
            JOIN reach ON m.roleid = reach.member
        )
        SELECT DISTINCT l.rolname FROM reach
        JOIN pg_catalog.pg_roles AS l ON l.oid = reach.member
        WHERE l.rolcanlogin
    LOOP
        EXECUTE format('ALTER ROLE %I IN DATABASE %I SET statement_timeout = %L',
                       r.rolname, v_db, '60s');
        EXECUTE format('ALTER ROLE %I IN DATABASE %I SET lock_timeout = %L',
                       r.rolname, v_db, '5s');
        EXECUTE format('ALTER ROLE %I IN DATABASE %I SET idle_in_transaction_session_timeout = %L',
                       r.rolname, v_db, '60s');
    END LOOP;
END
$privileges$;

REVOKE ALL ON SCHEMA audit_store FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA audit_store FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA audit_store FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA audit_store FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE audit_store_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
