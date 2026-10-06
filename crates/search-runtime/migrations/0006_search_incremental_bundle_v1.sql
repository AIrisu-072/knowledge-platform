-- B6: an INCREMENTAL target is a self-contained P7 bundle. Its payload,
-- receipt and lexical children may be written while its Graph build guard
-- (search_graph.build_guard, the incremental build authority) is live, and
-- it becomes READY only with that guard live and its copy verified. FULL
-- targets keep the exact full-guard rules. Both guard functions also pin
-- pg_temp last on the definer search path and qualify every catalog call.
-- Released migrations are never edited; these replace the functions only.
CREATE OR REPLACE FUNCTION search_guard_generation_child()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp AS $child_guard$
DECLARE
    parent public.search_generation%ROWTYPE;
    child_source UUID;
    child_generation UUID;
BEGIN
    IF TG_OP = 'DELETE' THEN
        child_source := OLD.source_id;
        child_generation := OLD.generation_id;
    ELSE
        child_source := NEW.source_id;
        child_generation := NEW.generation_id;
    END IF;
    IF TG_OP = 'UPDATE' AND
       (NEW.source_id IS DISTINCT FROM OLD.source_id
        OR NEW.generation_id IS DISTINCT FROM OLD.generation_id) THEN
        RAISE EXCEPTION 'child generation key is immutable' USING ERRCODE = '23514';
    END IF;
    SELECT * INTO parent FROM public.search_generation
    WHERE source_id = child_source AND generation_id = child_generation
    FOR UPDATE;
    IF NOT FOUND THEN
        IF TG_OP = 'DELETE' THEN
            RETURN OLD;
        END IF;
        RETURN NEW; -- composite FK reports wrong-key 23503
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF parent.state <> 'DELETING'
           OR NOT (session_user = 'postgres'
                   OR pg_catalog.pg_has_role(session_user, 'search_gc'::name, 'USAGE'::text)) THEN
            RAISE EXCEPTION 'child delete requires GC role and DELETING parent'
                USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF parent.state <> 'BUILDING' THEN
        RAISE EXCEPTION 'child write requires BUILDING parent' USING ERRCODE = '23514';
    END IF;
    IF parent.build_kind = 'FULL' AND NOT EXISTS (
           SELECT 1 FROM public.search_generation_full_guard AS g
           WHERE g.source_id = parent.source_id
             AND g.target_generation_id = parent.generation_id
             AND g.guard_token = parent.full_guard_token
             AND g.build_fence = parent.full_build_fence
             AND g.expires_at > pg_catalog.clock_timestamp()
       ) THEN
        RAISE EXCEPTION 'child write requires BUILDING parent and live exact full guard'
            USING ERRCODE = '23514';
    END IF;
    -- Nested so that a FULL-only database never plans the Graph relation.
    IF parent.build_kind = 'INCREMENTAL' THEN
        IF NOT EXISTS (
               SELECT 1 FROM search_graph.build_guard AS b
               WHERE b.source_id = parent.source_id
                 AND b.target_generation_id = parent.generation_id
                 AND b.expires_at > pg_catalog.clock_timestamp()
           ) THEN
            RAISE EXCEPTION 'child write requires BUILDING parent and live incremental build guard'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$child_guard$;

CREATE OR REPLACE FUNCTION search_guard_generation_row()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp AS $generation_guard$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'BUILDING' THEN
            RAISE EXCEPTION 'generation must begin BUILDING' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF OLD.state <> 'DELETING'
           OR NOT (session_user = 'postgres' OR pg_catalog.pg_has_role(session_user, 'search_gc'::name, 'USAGE'::text)) THEN
            RAISE EXCEPTION 'generation delete requires GC role and DELETING'
                USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF ROW(NEW.source_id,NEW.generation_id,NEW.activation_epoch,NEW.build_kind,
           NEW.stage_origin,NEW.stage_event_id,NEW.stage_source_epoch,
           NEW.full_guard_token,NEW.full_build_fence,NEW.source_snapshot,
           NEW.projection_manifest,NEW.projection_manifest_digest,
           NEW.projection_resource_count,NEW.bundle_version)
       IS DISTINCT FROM
       ROW(OLD.source_id,OLD.generation_id,OLD.activation_epoch,OLD.build_kind,
           OLD.stage_origin,OLD.stage_event_id,OLD.stage_source_epoch,
           OLD.full_guard_token,OLD.full_build_fence,OLD.source_snapshot,
           OLD.projection_manifest,OLD.projection_manifest_digest,
           OLD.projection_resource_count,OLD.bundle_version) THEN
        RAISE EXCEPTION 'generation binding is immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'READY' AND NEW.state NOT IN ('READY','DELETING') THEN
        RAISE EXCEPTION 'READY generation cannot return to build' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'FAILED' AND NEW.state NOT IN ('FAILED','DELETING') THEN
        RAISE EXCEPTION 'FAILED generation cannot return to build' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'DELETING' AND NEW.state <> 'DELETING' THEN
        RAISE EXCEPTION 'DELETING generation cannot be revived' USING ERRCODE = '23514';
    END IF;
    IF NEW.state = 'DELETING' AND OLD.state <> 'DELETING'
       AND NOT (session_user = 'postgres' OR pg_catalog.pg_has_role(session_user, 'search_gc'::name, 'USAGE'::text)) THEN
        RAISE EXCEPTION 'only GC can enter DELETING' USING ERRCODE = '23514';
    END IF;
    IF NEW.state IN ('READY','FAILED') AND NEW.state <> OLD.state
       AND NOT (session_user = 'postgres'
                OR pg_catalog.pg_has_role(session_user, 'search_coordinator'::name, 'USAGE'::text)) THEN
        RAISE EXCEPTION 'only coordinator can settle generation build'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.state IN ('READY','FAILED','DELETING')
       AND NEW.ready_at IS DISTINCT FROM OLD.ready_at THEN
        RAISE EXCEPTION 'ready timestamp is immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.state = 'READY' AND NEW.build_kind = 'FULL' AND NOT EXISTS (
        SELECT 1 FROM public.search_generation_full_guard AS g
        WHERE g.source_id = NEW.source_id
          AND g.target_generation_id = NEW.generation_id
          AND g.guard_token = NEW.full_guard_token
          AND g.build_fence = NEW.full_build_fence
          AND g.expires_at > clock_timestamp()
    ) THEN
        RAISE EXCEPTION 'READY needs live exact full guard' USING ERRCODE = '23514';
    END IF;
    -- Nested so that a FULL-only database never plans the Graph relation.
    IF NEW.state = 'READY' AND NEW.build_kind = 'INCREMENTAL' THEN
        IF NOT EXISTS (
            SELECT 1 FROM search_graph.build_guard AS b
            WHERE b.source_id = NEW.source_id
              AND b.target_generation_id = NEW.generation_id
              AND b.copy_verified_at IS NOT NULL
              AND b.expires_at > pg_catalog.clock_timestamp()
        ) THEN
            RAISE EXCEPTION 'READY needs live verified incremental build guard'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$generation_guard$;
