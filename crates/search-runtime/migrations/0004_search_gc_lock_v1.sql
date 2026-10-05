-- P7-11: GC takes the Source row first (Source -> generation -> guard ->
-- lease) like publication and pinning. Row locks need UPDATE privilege, which
-- the GC capability must not hold on the coordination row, so this definer
-- function grants the lock and the pointer read only. The lock belongs to the
-- caller's transaction.
CREATE FUNCTION search_gc_lock_source(target UUID)
RETURNS TABLE (current_generation_id UUID)
LANGUAGE sql SECURITY DEFINER
SET search_path = pg_catalog, public AS $gc_lock$
    SELECT s.current_generation_id
    FROM public.search_source_coordination AS s
    WHERE s.source_id = target
    FOR UPDATE
$gc_lock$;
REVOKE ALL ON FUNCTION search_gc_lock_source(UUID) FROM PUBLIC;
