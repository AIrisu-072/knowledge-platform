-- P3-G03: durable typed n-ary Graph generations. PostgreSQL is the provisional
-- backend (owner decision 2026-10-05; re-selected before production). Separate
-- schema and migration ledger; applied after Search runtime migration 0003.
-- Every Graph parent is bound to its P7 target row. Children are written only
-- while the parent is BUILDING under a live guard, and removed only under a
-- live guard (incremental delta) or by GC under DELETING. READY is immutable.

CREATE SCHEMA search_graph;

CREATE TABLE search_graph.generation (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    graph_schema_version TEXT NOT NULL,
    build_kind TEXT NOT NULL,
    full_guard_token UUID NULL,
    full_build_fence BIGINT NULL,
    incremental_base_generation_id UUID NULL,
    build_guard_token UUID NULL,
    build_fence BIGINT NULL,
    source_snapshot TEXT NOT NULL,
    projection_manifest_digest TEXT NOT NULL,
    source_mapping_digest TEXT NOT NULL,
    state TEXT NOT NULL,
    graph_content_digest TEXT NULL,
    resource_count BIGINT NULL,
    relation_count BIGINT NULL,
    batch_phase TEXT NULL,
    batch_sequence BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    ready_at TIMESTAMPTZ NULL,
    CONSTRAINT graph_generation_pkey PRIMARY KEY (source_id,generation_id),
    CONSTRAINT fk_graph_generation_p7_target FOREIGN KEY (source_id,generation_id)
        REFERENCES public.search_generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT fk_graph_generation_p7_full_binding
        FOREIGN KEY (source_id,generation_id,full_guard_token,full_build_fence)
        REFERENCES public.search_generation(source_id,generation_id,full_guard_token,full_build_fence)
        ON DELETE RESTRICT,
    CONSTRAINT uq_graph_generation_build_binding
        UNIQUE (source_id,generation_id,build_guard_token,build_fence),
    CONSTRAINT ck_graph_generation_schema CHECK (graph_schema_version = 'search-graph-v1'),
    CONSTRAINT ck_graph_generation_state CHECK
        (state IN ('BUILDING','READY','FAILED','DELETING')),
    CONSTRAINT ck_graph_generation_kind CHECK (
        (build_kind = 'FULL' AND full_guard_token IS NOT NULL AND full_build_fence > 0
         AND incremental_base_generation_id IS NULL AND build_guard_token IS NULL
         AND build_fence IS NULL)
        OR (build_kind = 'INCREMENTAL' AND full_guard_token IS NULL
            AND full_build_fence IS NULL AND incremental_base_generation_id IS NOT NULL
            AND incremental_base_generation_id <> generation_id
            AND build_guard_token IS NOT NULL AND build_fence > 0)
    ),
    CONSTRAINT ck_graph_generation_snapshot CHECK
        (octet_length(source_snapshot) BETWEEN 1 AND 1024),
    CONSTRAINT ck_graph_generation_digests CHECK (
        public.search_is_sha256_digest(projection_manifest_digest)
        AND public.search_is_sha256_digest(source_mapping_digest)
        AND (graph_content_digest IS NULL
             OR public.search_is_sha256_digest(graph_content_digest))
    ),
    CONSTRAINT ck_graph_generation_counts CHECK (
        (resource_count IS NULL OR resource_count >= 0)
        AND (relation_count IS NULL OR relation_count >= 0)
    ),
    CONSTRAINT ck_graph_generation_ready CHECK (
        (state = 'READY' AND ready_at IS NOT NULL AND graph_content_digest IS NOT NULL
         AND resource_count IS NOT NULL AND relation_count IS NOT NULL)
        OR (state IN ('BUILDING','FAILED') AND ready_at IS NULL)
        OR state = 'DELETING'
    ),
    CONSTRAINT ck_graph_generation_batch CHECK (
        batch_sequence >= 0 AND (batch_phase IS NULL OR batch_phase IN ('COPY','DELTA'))
    )
);

CREATE TABLE search_graph.resource (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    resource_id UUID NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_version_id UUID NULL,
    mapping_kind TEXT NOT NULL,
    owner_document_id UUID NULL,
    folder_id UUID NULL,
    version_id UUID NULL,
    adapter_id TEXT NULL,
    native_id TEXT NULL,
    valid_from_nanos NUMERIC(30,0) NULL,
    valid_from_offset INTEGER NULL,
    valid_to_nanos NUMERIC(30,0) NULL,
    valid_to_offset INTEGER NULL,
    freshness_anchor_nanos NUMERIC(30,0) NULL,
    freshness_anchor_offset INTEGER NULL,
    freshness_basis TEXT NULL,
    effective_from_nanos NUMERIC(30,0) NULL,
    effective_from_offset INTEGER NULL,
    effective_to_nanos NUMERIC(30,0) NULL,
    effective_to_offset INTEGER NULL,
    CONSTRAINT graph_resource_pkey PRIMARY KEY (source_id,generation_id,resource_id),
    CONSTRAINT fk_graph_resource_generation FOREIGN KEY (source_id,generation_id)
        REFERENCES search_graph.generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT ck_graph_resource_kind CHECK (resource_kind IN
        ('KNOWLEDGE','DOCUMENT','FOLDER_PLACEMENT','SEMANTIC','CAPABILITY',
         'AGENT_SKILL','WORKFLOW','POLICY')),
    CONSTRAINT ck_graph_resource_mapping CHECK (
        (mapping_kind = 'DOCUMENT' AND owner_document_id IS NOT NULL AND folder_id IS NULL
         AND version_id IS NULL AND adapter_id IS NULL AND native_id IS NULL)
        OR (mapping_kind = 'FOLDER_PLACEMENT' AND owner_document_id IS NOT NULL
            AND folder_id IS NOT NULL AND version_id IS NULL AND adapter_id IS NULL
            AND native_id IS NULL)
        OR (mapping_kind = 'VERSION' AND owner_document_id IS NOT NULL AND folder_id IS NULL
            AND version_id IS NOT NULL AND adapter_id IS NULL AND native_id IS NULL)
        OR (mapping_kind = 'REGISTERED' AND owner_document_id IS NULL AND folder_id IS NULL
            AND version_id IS NULL AND btrim(COALESCE(adapter_id,'')) <> ''
            AND btrim(COALESCE(native_id,'')) <> '')
    ),
    CONSTRAINT ck_graph_resource_instants CHECK (
        (valid_from_nanos IS NULL) = (valid_from_offset IS NULL)
        AND (valid_to_nanos IS NULL) = (valid_to_offset IS NULL)
        AND (freshness_anchor_nanos IS NULL) = (freshness_anchor_offset IS NULL)
        AND (effective_from_nanos IS NULL) = (effective_from_offset IS NULL)
        AND (effective_to_nanos IS NULL) = (effective_to_offset IS NULL)
    ),
    CONSTRAINT ck_graph_resource_offsets CHECK (
        COALESCE(valid_from_offset,0) BETWEEN -93599 AND 93599
        AND COALESCE(valid_to_offset,0) BETWEEN -93599 AND 93599
        AND COALESCE(freshness_anchor_offset,0) BETWEEN -93599 AND 93599
        AND COALESCE(effective_from_offset,0) BETWEEN -93599 AND 93599
        AND COALESCE(effective_to_offset,0) BETWEEN -93599 AND 93599
    ),
    CONSTRAINT ck_graph_resource_half_open CHECK (
        (valid_from_nanos IS NULL OR valid_to_nanos IS NULL OR valid_from_nanos < valid_to_nanos)
        AND (effective_from_nanos IS NULL OR effective_to_nanos IS NULL
             OR effective_from_nanos < effective_to_nanos)
    )
);

CREATE TABLE search_graph.relation (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    relation_id UUID NOT NULL,
    namespace TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    canonical_digest TEXT NOT NULL,
    CONSTRAINT graph_relation_pkey PRIMARY KEY (source_id,generation_id,relation_id),
    CONSTRAINT fk_graph_relation_generation FOREIGN KEY (source_id,generation_id)
        REFERENCES search_graph.generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT ck_graph_relation_namespace CHECK
        (namespace IN ('DISCOVERY','SEMANTIC','EVIDENCE')),
    CONSTRAINT ck_graph_relation_type CHECK (btrim(relation_type) <> ''),
    CONSTRAINT ck_graph_relation_payload CHECK (
        jsonb_typeof(payload) = 'object'
        AND COALESCE(payload ->> 'dto_version' = 'v1', FALSE)
    ),
    CONSTRAINT ck_graph_relation_digest CHECK (public.search_is_sha256_digest(canonical_digest))
);
CREATE INDEX graph_relation_by_type
    ON search_graph.relation(source_id,generation_id,namespace,relation_type,relation_id);

CREATE TABLE search_graph.participant (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    relation_id UUID NOT NULL,
    ordinal INTEGER NOT NULL,
    role TEXT NOT NULL,
    resource_id UUID NOT NULL,
    CONSTRAINT graph_participant_pkey
        PRIMARY KEY (source_id,generation_id,relation_id,ordinal),
    CONSTRAINT uq_graph_participant_role
        UNIQUE (source_id,generation_id,relation_id,role,resource_id),
    CONSTRAINT fk_graph_participant_relation FOREIGN KEY (source_id,generation_id,relation_id)
        REFERENCES search_graph.relation(source_id,generation_id,relation_id) ON DELETE RESTRICT,
    CONSTRAINT fk_graph_participant_resource FOREIGN KEY (source_id,generation_id,resource_id)
        REFERENCES search_graph.resource(source_id,generation_id,resource_id) ON DELETE RESTRICT,
    CONSTRAINT ck_graph_participant_ordinal CHECK (ordinal >= 0),
    CONSTRAINT ck_graph_participant_role CHECK (btrim(role) <> '')
);
CREATE INDEX graph_participant_incidence
    ON search_graph.participant(source_id,generation_id,resource_id,role,relation_id);

CREATE TABLE search_graph.build_guard (
    source_id UUID NOT NULL,
    base_generation_id UUID NOT NULL,
    target_generation_id UUID NOT NULL,
    guard_token UUID NOT NULL,
    fence BIGINT NOT NULL,
    base_manifest_digest TEXT NOT NULL,
    base_graph_content_digest TEXT NOT NULL,
    base_source_snapshot TEXT NOT NULL,
    base_source_mapping_digest TEXT NOT NULL,
    base_resource_count BIGINT NOT NULL,
    base_relation_count BIGINT NOT NULL,
    target_manifest_digest TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    copy_verified_at TIMESTAMPTZ NULL,
    CONSTRAINT graph_build_guard_pkey PRIMARY KEY (source_id,target_generation_id),
    CONSTRAINT uq_graph_build_guard_token UNIQUE (guard_token),
    CONSTRAINT uq_graph_build_guard_fence UNIQUE (source_id,fence),
    CONSTRAINT ck_graph_build_guard_keys CHECK (base_generation_id <> target_generation_id),
    CONSTRAINT ck_graph_build_guard_fence CHECK (fence > 0),
    CONSTRAINT ck_graph_build_guard_counts CHECK
        (base_resource_count >= 0 AND base_relation_count >= 0),
    CONSTRAINT fk_graph_build_guard_base FOREIGN KEY (source_id,base_generation_id)
        REFERENCES search_graph.generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT fk_graph_build_guard_target
        FOREIGN KEY (source_id,target_generation_id,guard_token,fence)
        REFERENCES search_graph.generation(source_id,generation_id,build_guard_token,build_fence)
        ON DELETE RESTRICT
);
CREATE INDEX graph_build_guard_by_base
    ON search_graph.build_guard(source_id,base_generation_id);
CREATE INDEX graph_build_guard_by_expiry
    ON search_graph.build_guard(expires_at,source_id,target_generation_id);

-- A role check that holds for the database owner and for a login granted the
-- capability role; SET ROLE does not change session_user.
CREATE FUNCTION search_graph.has_capability(capability TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog AS $capability$
    SELECT session_user = 'postgres' OR pg_has_role(session_user, capability, 'USAGE');
$capability$;

-- The live guard of a BUILDING parent: the P7 full guard for FULL, the Graph
-- build guard for INCREMENTAL. Both compare token, fence and the DB clock.
CREATE FUNCTION search_graph.live_guard(parent search_graph.generation)
RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog AS $live_guard$
    SELECT CASE parent.build_kind
        WHEN 'FULL' THEN EXISTS (
            SELECT 1 FROM public.search_generation_full_guard AS g
            WHERE g.source_id = parent.source_id
              AND g.target_generation_id = parent.generation_id
              AND g.guard_token = parent.full_guard_token
              AND g.build_fence = parent.full_build_fence
              AND g.expires_at > clock_timestamp())
        WHEN 'INCREMENTAL' THEN EXISTS (
            SELECT 1 FROM search_graph.build_guard AS g
            WHERE g.source_id = parent.source_id
              AND g.target_generation_id = parent.generation_id
              AND g.base_generation_id = parent.incremental_base_generation_id
              AND g.guard_token = parent.build_guard_token
              AND g.fence = parent.build_fence
              AND g.expires_at > clock_timestamp())
        ELSE FALSE
    END;
$live_guard$;

CREATE FUNCTION search_graph.guard_generation()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog AS $generation$
DECLARE
    target public.search_generation%ROWTYPE;
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT (search_graph.has_capability('search_coordinator')
                OR search_graph.has_capability('search_registration')) THEN
            RAISE EXCEPTION 'only registration may create a Graph parent'
                USING ERRCODE = '42501';
        END IF;
        IF NEW.state <> 'BUILDING' OR NEW.ready_at IS NOT NULL
           OR NEW.graph_content_digest IS NOT NULL OR NEW.resource_count IS NOT NULL
           OR NEW.relation_count IS NOT NULL OR NEW.batch_phase IS NOT NULL
           OR NEW.batch_sequence <> 0 THEN
            RAISE EXCEPTION 'Graph parent must begin as an empty BUILDING row'
                USING ERRCODE = '23514';
        END IF;
        SELECT * INTO target FROM public.search_generation
        WHERE source_id = NEW.source_id AND generation_id = NEW.generation_id
        FOR SHARE;
        IF NOT FOUND OR target.state <> 'BUILDING' OR target.build_kind <> NEW.build_kind
           OR target.source_snapshot <> NEW.source_snapshot
           OR target.projection_manifest_digest <> NEW.projection_manifest_digest THEN
            RAISE EXCEPTION 'Graph parent must bind its BUILDING P7 target'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF OLD.state <> 'DELETING' OR NOT search_graph.has_capability('search_gc') THEN
            RAISE EXCEPTION 'Graph parent delete requires GC and DELETING'
                USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF ROW(NEW.source_id,NEW.generation_id,NEW.graph_schema_version,NEW.build_kind,
           NEW.full_guard_token,NEW.full_build_fence,NEW.incremental_base_generation_id,
           NEW.build_guard_token,NEW.build_fence,NEW.source_snapshot,
           NEW.projection_manifest_digest,NEW.source_mapping_digest,NEW.created_at)
       IS DISTINCT FROM
       ROW(OLD.source_id,OLD.generation_id,OLD.graph_schema_version,OLD.build_kind,
           OLD.full_guard_token,OLD.full_build_fence,OLD.incremental_base_generation_id,
           OLD.build_guard_token,OLD.build_fence,OLD.source_snapshot,
           OLD.projection_manifest_digest,OLD.source_mapping_digest,OLD.created_at) THEN
        RAISE EXCEPTION 'Graph parent binding is immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'READY' AND NEW.state NOT IN ('READY','DELETING') THEN
        RAISE EXCEPTION 'READY Graph cannot return to build' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'FAILED' AND NEW.state NOT IN ('FAILED','DELETING') THEN
        RAISE EXCEPTION 'FAILED Graph cannot return to build' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'DELETING' AND NEW.state <> 'DELETING' THEN
        RAISE EXCEPTION 'DELETING Graph cannot be revived' USING ERRCODE = '23514';
    END IF;
    IF OLD.state <> 'BUILDING'
       AND ROW(NEW.graph_content_digest,NEW.resource_count,NEW.relation_count,NEW.ready_at,
               NEW.batch_phase,NEW.batch_sequence)
           IS DISTINCT FROM
           ROW(OLD.graph_content_digest,OLD.resource_count,OLD.relation_count,OLD.ready_at,
               OLD.batch_phase,OLD.batch_sequence) THEN
        RAISE EXCEPTION 'settled Graph content is immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.state = 'DELETING' AND OLD.state <> 'DELETING'
       AND NOT search_graph.has_capability('search_gc') THEN
        RAISE EXCEPTION 'only GC can enter DELETING' USING ERRCODE = '23514';
    END IF;
    IF NEW.state IN ('READY','FAILED') AND NEW.state <> OLD.state
       AND NOT search_graph.has_capability('search_coordinator') THEN
        RAISE EXCEPTION 'only coordinator can settle a Graph build' USING ERRCODE = '23514';
    END IF;
    IF OLD.state = 'BUILDING' AND NEW.state = 'BUILDING'
       AND NOT search_graph.live_guard(NEW) THEN
        RAISE EXCEPTION 'Graph build progress needs a live guard' USING ERRCODE = '23514';
    END IF;
    IF NEW.state = 'READY' AND OLD.state = 'BUILDING' THEN
        IF NOT search_graph.live_guard(NEW) THEN
            RAISE EXCEPTION 'READY needs a live guard' USING ERRCODE = '23514';
        END IF;
        IF NEW.build_kind = 'INCREMENTAL' AND NOT EXISTS (
            SELECT 1 FROM search_graph.build_guard AS g
            WHERE g.source_id = NEW.source_id AND g.target_generation_id = NEW.generation_id
              AND g.copy_verified_at IS NOT NULL) THEN
            RAISE EXCEPTION 'READY needs a verified base copy' USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$generation$;
CREATE TRIGGER graph_guard_generation
    BEFORE INSERT OR UPDATE OR DELETE ON search_graph.generation
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_generation();

CREATE FUNCTION search_graph.guard_child()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog AS $child$
DECLARE
    parent search_graph.generation%ROWTYPE;
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
        RAISE EXCEPTION 'Graph child key is immutable' USING ERRCODE = '23514';
    END IF;
    SELECT * INTO parent FROM search_graph.generation
    WHERE source_id = child_source AND generation_id = child_generation
    FOR UPDATE;
    IF NOT FOUND THEN
        IF TG_OP = 'DELETE' THEN
            RETURN OLD;
        END IF;
        RETURN NEW; -- the composite FK reports the wrong key as 23503
    END IF;
    IF TG_OP = 'DELETE' AND parent.state = 'DELETING' THEN
        IF NOT search_graph.has_capability('search_gc') THEN
            RAISE EXCEPTION 'Graph child cleanup requires GC' USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF parent.state <> 'BUILDING' OR NOT search_graph.live_guard(parent) THEN
        RAISE EXCEPTION 'Graph child write requires BUILDING parent and live guard'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$child$;
CREATE TRIGGER graph_guard_resource
    BEFORE INSERT OR UPDATE OR DELETE ON search_graph.resource
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_child();
CREATE TRIGGER graph_guard_relation
    BEFORE INSERT OR UPDATE OR DELETE ON search_graph.relation
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_child();
CREATE TRIGGER graph_guard_participant
    BEFORE INSERT OR UPDATE OR DELETE ON search_graph.participant
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_child();

CREATE FUNCTION search_graph.guard_build_guard()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog AS $build_guard$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT (search_graph.has_capability('search_coordinator')
                OR search_graph.has_capability('search_registration')) THEN
            RAISE EXCEPTION 'only registration may issue a build guard'
                USING ERRCODE = '42501';
        END IF;
        IF NEW.copy_verified_at IS NOT NULL OR NEW.expires_at <= clock_timestamp() THEN
            RAISE EXCEPTION 'a build guard begins live and unverified'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF NOT (search_graph.has_capability('search_coordinator')
                OR search_graph.has_capability('search_gc')) THEN
            RAISE EXCEPTION 'build guard removal requires coordinator or GC'
                USING ERRCODE = '42501';
        END IF;
        RETURN OLD;
    END IF;
    IF ROW(NEW.source_id,NEW.base_generation_id,NEW.target_generation_id,NEW.guard_token,
           NEW.fence,NEW.base_manifest_digest,NEW.base_graph_content_digest,
           NEW.base_source_snapshot,NEW.base_source_mapping_digest,NEW.base_resource_count,
           NEW.base_relation_count,NEW.target_manifest_digest)
       IS DISTINCT FROM
       ROW(OLD.source_id,OLD.base_generation_id,OLD.target_generation_id,OLD.guard_token,
           OLD.fence,OLD.base_manifest_digest,OLD.base_graph_content_digest,
           OLD.base_source_snapshot,OLD.base_source_mapping_digest,OLD.base_resource_count,
           OLD.base_relation_count,OLD.target_manifest_digest) THEN
        RAISE EXCEPTION 'build guard binding is immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.expires_at <= clock_timestamp() THEN
        RAISE EXCEPTION 'an expired build guard cannot be revived' USING ERRCODE = '23514';
    END IF;
    IF OLD.copy_verified_at IS NOT NULL
       AND NEW.copy_verified_at IS DISTINCT FROM OLD.copy_verified_at THEN
        RAISE EXCEPTION 'copy verification is recorded once' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$build_guard$;
CREATE TRIGGER graph_guard_build_guard
    BEFORE INSERT OR UPDATE OR DELETE ON search_graph.build_guard
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_build_guard();

REVOKE ALL ON ALL TABLES IN SCHEMA search_graph FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA search_graph FROM PUBLIC;
