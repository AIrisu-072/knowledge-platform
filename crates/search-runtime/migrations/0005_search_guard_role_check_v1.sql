-- The generation guard's role checks name pg_catalog and typed literals, so
-- no function created in another schema on the search path can answer them.
-- Same body as 0003 otherwise; 0003 itself is never edited after release.
CREATE OR REPLACE FUNCTION search_guard_generation_row()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $generation_guard$
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
    RETURN NEW;
END;
$generation_guard$;
