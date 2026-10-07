-- Audit Store capability roles (design §10.1). Template applied by the
-- database owner (a superuser, or a CREATEROLE role that is a member of
-- audit_store_owner) after migration 0001. Idempotent.
--
-- Capability roles are NOLOGIN. Each operator or service gets its own LOGIN
-- role, is granted the capability roles it needs, and is bound to a principal
-- with `audit-admin bind`. Run sql/privileges.sql afterwards (and after every
-- restore or new login role) to revoke PUBLIC access and set role timeouts.
--
-- bootstrap_administrator, bind_principal and unbind_principal are not
-- granted to anyone: only members of audit_store_owner may call them.

DO $roles$
DECLARE
    r TEXT;
BEGIN
    FOREACH r IN ARRAY ARRAY['audit_store_ingest', 'audit_store_relay_control',
                             'audit_store_reader', 'audit_store_verifier',
                             'audit_store_admin', 'audit_store_maintainer'] LOOP
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

GRANT USAGE ON SCHEMA audit_store TO audit_store_ingest, audit_store_relay_control,
    audit_store_reader, audit_store_verifier, audit_store_admin, audit_store_maintainer;

-- relay (service/audit-relay): idempotent ingest and content-free receipts.
GRANT EXECUTE ON FUNCTION
    audit_store.ingest(jsonb),
    audit_store.probe(),
    audit_store.lookup_receipts(uuid[]),
    audit_store.list_source_receipts(text, bigint, integer),
    audit_store.lookup_control_receipts(bigint[])
    TO audit_store_ingest;

-- relay control events (replay, reconciliation, source mismatch).
GRANT EXECUTE ON FUNCTION
    audit_store.record_relay_control(text, uuid, text, jsonb)
    TO audit_store_relay_control;

-- two-phase disclosure: investigate/export (reader), verify/identity_chain (verifier).
GRANT EXECUTE ON FUNCTION
    audit_store.open_access(text, jsonb, integer, integer),
    audit_store.read_page(text, bigint),
    audit_store.close_access(text, bigint, text[])
    TO audit_store_reader, audit_store_verifier;

GRANT EXECUTE ON FUNCTION
    audit_store.verify(bigint, bigint),
    audit_store.checkpoint()
    TO audit_store_verifier;

GRANT EXECUTE ON FUNCTION
    audit_store.verify_recovery(),
    audit_store.identity_chain_recovery_page(bigint, integer)
    TO audit_store_verifier, audit_store_maintainer;

GRANT EXECUTE ON FUNCTION
    audit_store.change_access(text, text, text, text),
    audit_store.set_retention_policy(text, jsonb, integer)
    TO audit_store_admin;

GRANT EXECUTE ON FUNCTION
    audit_store.expire(text, integer, timestamptz, integer),
    audit_store.purge_body(uuid, text),
    audit_store.begin_recovery_epoch(bigint, bigint, text, bigint),
    audit_store.rebind_fingerprint()
    TO audit_store_maintainer;

-- Content-free status and posture for operators and health.
GRANT EXECUTE ON FUNCTION audit_store.store_status()
    TO audit_store_ingest, audit_store_relay_control, audit_store_reader,
       audit_store_verifier, audit_store_admin, audit_store_maintainer;
GRANT EXECUTE ON FUNCTION audit_store.posture_check()
    TO audit_store_verifier, audit_store_admin, audit_store_maintainer;
