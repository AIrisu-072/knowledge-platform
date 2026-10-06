-- P3 hardening: every Search SECURITY DEFINER function created before 0006
-- lists pg_temp explicitly last, so a caller's temporary objects never shadow
-- the names it resolves. 0006 already replaced its two functions this way.
ALTER FUNCTION search_guard_owner_identity() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_guard_source_id() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_assert_source_ownership_pair() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_guard_registration_serial() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_guard_receipt_bundle_version() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_guard_generation_identity() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_guard_full_guard() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_guard_evaluation_lease() SET search_path = pg_catalog, public, pg_temp;
ALTER FUNCTION search_gc_lock_source(UUID) SET search_path = pg_catalog, public, pg_temp;
