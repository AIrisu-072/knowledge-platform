-- P3 hardening of the Graph guards; 0001 is released and never edited.

-- Definer functions list pg_temp explicitly last.
ALTER FUNCTION search_graph.has_capability(TEXT) SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION search_graph.live_guard(search_graph.generation) SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION search_graph.guard_generation() SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION search_graph.guard_child() SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION search_graph.guard_build_guard() SET search_path = pg_catalog, pg_temp;

-- An INCREMENTAL target becomes READY only after its closed delta applied.
CREATE FUNCTION search_graph.guard_incremental_ready()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $incremental_ready$
BEGIN
    IF NEW.state = 'READY' AND OLD.state = 'BUILDING' AND NEW.build_kind = 'INCREMENTAL'
       AND NOT (NEW.batch_phase IS NOT DISTINCT FROM 'DELTA' AND NEW.batch_sequence = 1) THEN
        RAISE EXCEPTION 'READY needs the applied delta' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$incremental_ready$;
CREATE TRIGGER graph_guard_incremental_ready
    BEFORE UPDATE ON search_graph.generation
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_incremental_ready();

-- A build guard carries the READY base's receipt and binds the registered
-- INCREMENTAL target, so a direct insert cannot open child writes.
CREATE FUNCTION search_graph.guard_build_guard_binding()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $guard_binding$
DECLARE
    base search_graph.generation%ROWTYPE;
    target search_graph.generation%ROWTYPE;
BEGIN
    SELECT * INTO base FROM search_graph.generation
    WHERE source_id = NEW.source_id AND generation_id = NEW.base_generation_id
    FOR SHARE;
    IF NOT FOUND OR base.state <> 'READY'
       OR ROW(base.projection_manifest_digest, base.graph_content_digest,
              base.source_snapshot, base.source_mapping_digest,
              base.resource_count, base.relation_count)
          IS DISTINCT FROM
          ROW(NEW.base_manifest_digest, NEW.base_graph_content_digest,
              NEW.base_source_snapshot, NEW.base_source_mapping_digest,
              NEW.base_resource_count, NEW.base_relation_count) THEN
        RAISE EXCEPTION 'a build guard needs its READY base receipt'
            USING ERRCODE = '23514';
    END IF;
    SELECT * INTO target FROM search_graph.generation
    WHERE source_id = NEW.source_id AND generation_id = NEW.target_generation_id
    FOR SHARE;
    IF NOT FOUND OR target.state <> 'BUILDING' OR target.build_kind <> 'INCREMENTAL'
       OR target.incremental_base_generation_id IS DISTINCT FROM NEW.base_generation_id
       OR target.build_guard_token IS DISTINCT FROM NEW.guard_token
       OR target.build_fence IS DISTINCT FROM NEW.fence
       OR target.projection_manifest_digest IS DISTINCT FROM NEW.target_manifest_digest THEN
        RAISE EXCEPTION 'a build guard binds its registered target'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$guard_binding$;
CREATE TRIGGER graph_guard_build_guard_binding
    BEFORE INSERT ON search_graph.build_guard
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_build_guard_binding();

-- Row guards do not see TRUNCATE; Graph tables are never truncated.
CREATE FUNCTION search_graph.refuse_truncate()
RETURNS TRIGGER LANGUAGE plpgsql
SET search_path = pg_catalog, pg_temp AS $refuse_truncate$
BEGIN
    RAISE EXCEPTION 'Graph tables are never truncated' USING ERRCODE = '42501';
END;
$refuse_truncate$;
CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.generation
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();
CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.resource
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();
CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.relation
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();
CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.participant
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();
CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.build_guard
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();

REVOKE ALL ON FUNCTION search_graph.guard_incremental_ready(),
    search_graph.guard_build_guard_binding(), search_graph.refuse_truncate() FROM PUBLIC;
