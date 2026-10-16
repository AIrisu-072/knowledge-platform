-- One non-superuser LOGIN role per serving process (run by the superuser `kp`
-- after all migrations and the Search role scripts):
--   kp_document         Document servers: row access to the Document tables only
--   kp_search_delivery  Search worker, outbox delivery: claims and settles
--                       Document outbox events only (the grants of
--                       crates/outbox-delivery/sql/least_privilege.sql, applied
--                       after this file)
--   kp_search_worker    Search worker, completion: builds and settles generations
--   kp_search_server    Search servers: register the configured Source, then read
-- No role has DDL or a superuser bypass. Passwords are read from the secret
-- files here, so they never appear on a command line.
\set ON_ERROR_STOP 1
\set document_password `cat /run/secrets/db_document_password`
\set search_delivery_password `cat /run/secrets/db_search_delivery_password`
\set search_worker_password `cat /run/secrets/db_search_worker_password`
\set search_server_password `cat /run/secrets/db_search_server_password`

DO $roles$
DECLARE
    name text;
BEGIN
    FOREACH name IN ARRAY ARRAY['kp_document', 'kp_search_delivery', 'kp_search_worker',
                               'kp_search_server'] LOOP
        IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = name) THEN
            EXECUTE format('CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE '
                           'NOREPLICATION INHERIT', name);
        END IF;
    END LOOP;
    -- The single shared role of the first stack version.
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kp_app') THEN
        DROP OWNED BY kp_app;
        DROP ROLE kp_app;
    END IF;
END
$roles$;
ALTER ROLE kp_document PASSWORD :'document_password';
ALTER ROLE kp_search_delivery PASSWORD :'search_delivery_password';
ALTER ROLE kp_search_worker PASSWORD :'search_worker_password';
ALTER ROLE kp_search_server PASSWORD :'search_server_password';

GRANT CONNECT ON DATABASE kp
    TO kp_document, kp_search_delivery, kp_search_worker, kp_search_server;
GRANT USAGE ON SCHEMA public TO kp_document, kp_search_worker, kp_search_server;

-- The Search capability roles (crates/search-runtime/sql/roles.sql and
-- crates/search-graph/sql/roles.sql) decide what each Search process may do
-- to the Search tables.
GRANT search_registration, search_builder, search_coordinator, search_gc
    TO kp_search_worker;
-- A Search server registers its configured Source when it starts.
GRANT search_registration, search_reader TO kp_search_server;

DO $grants$
DECLARE
    item record;
BEGIN
    -- Document servers: every Document table and sequence, rows only.
    FOR item IN
        SELECT tablename FROM pg_tables
        WHERE schemaname = 'public'
          AND tablename NOT LIKE 'search\_%'
          AND tablename <> '_sqlx_migrations'
    LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON public.%I TO kp_document',
                       item.tablename);
    END LOOP;
    FOR item IN
        SELECT sequencename FROM pg_sequences
        WHERE schemaname = 'public' AND sequencename NOT LIKE 'search\_%'
    LOOP
        EXECUTE format('GRANT USAGE, SELECT, UPDATE ON SEQUENCE public.%I TO kp_document',
                       item.sequencename);
    END LOOP;

    -- Search processes read the Document state that decides what a Search
    -- result may show: the current version and its files, the folder tree and
    -- the access policies. They never write a Document table; the outbox row
    -- lock the worker needs comes from search_coordinator.
    FOR item IN
        SELECT tablename FROM pg_tables
        WHERE schemaname = 'public'
          AND tablename IN ('documents', 'document_versions', 'document_revisions',
                            'version_files', 'file_objects', 'content_items',
                            'content_representations', 'folders',
                            'access_policy_bindings', 'access_policy_grants',
                            'document_access_state', 'document_publication_end_operations',
                            'document_semantic_inspections')
    LOOP
        EXECUTE format('GRANT SELECT ON public.%I TO kp_search_worker, kp_search_server',
                       item.tablename);
    END LOOP;
    -- Reads take FOR SHARE on the single access-state row, which needs an UPDATE
    -- grant. PK and CHECK (id = 1) allow only an assignment that changes nothing
    -- (as in crates/outbox-delivery/sql/least_privilege.sql).
    GRANT UPDATE (id) ON public.document_access_state TO kp_search_worker, kp_search_server;
    -- Startup checks read which migrations are applied; they never write it.
    IF to_regclass('public._sqlx_migrations') IS NOT NULL THEN
        GRANT SELECT ON public._sqlx_migrations
            TO kp_document, kp_search_worker, kp_search_server;
    END IF;
END
$grants$;
