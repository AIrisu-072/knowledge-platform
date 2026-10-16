-- The non-superuser role every serving process connects as (run by the
-- superuser `kp` after all migrations and the Search role scripts).
-- Search tables keep the privileges the Search roles grant; Document tables
-- get row access only. No DDL, no superuser bypass.
-- The password is read from the db_app_password secret file here, so it
-- never appears on a command line.
\set ON_ERROR_STOP 1
\set app_password `cat /run/secrets/db_app_password`

SELECT NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kp_app') AS create_app \gset
\if :create_app
CREATE ROLE kp_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION INHERIT;
\endif
ALTER ROLE kp_app PASSWORD :'app_password';

GRANT CONNECT ON DATABASE kp TO kp_app;
GRANT search_registration, search_builder, search_coordinator, search_gc,
      search_host_publisher, search_reader TO kp_app;
GRANT USAGE ON SCHEMA public TO kp_app;

DO $$
DECLARE
    item record;
BEGIN
    FOR item IN
        SELECT tablename FROM pg_tables
        WHERE schemaname = 'public'
          AND tablename NOT LIKE 'search\_%'
          AND tablename <> '_sqlx_migrations'
    LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON public.%I TO kp_app', item.tablename);
    END LOOP;
    FOR item IN
        SELECT sequencename FROM pg_sequences
        WHERE schemaname = 'public' AND sequencename NOT LIKE 'search\_%'
    LOOP
        EXECUTE format('GRANT USAGE, SELECT, UPDATE ON SEQUENCE public.%I TO kp_app', item.sequencename);
    END LOOP;
    -- Startup checks read which migrations are applied; they never write it.
    IF to_regclass('public._sqlx_migrations') IS NOT NULL THEN
        GRANT SELECT ON public._sqlx_migrations TO kp_app;
    END IF;
END
$$;
