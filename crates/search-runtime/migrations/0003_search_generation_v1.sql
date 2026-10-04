-- Durable Search generation identity and artifacts. No Graph backend is selected
-- here. In particular, a legacy current pointer cannot be reconstructed from
-- its two digests or the ownership proof supplied for 0002.
LOCK TABLE search_source_coordination, search_source_ownership, search_index_receipts
    IN ACCESS EXCLUSIVE MODE;

DO $legacy_generation_preflight$
DECLARE
    invalid_proof BOOLEAN;
BEGIN
    IF EXISTS (
        SELECT 1 FROM search_source_coordination
        WHERE current_generation_id IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'legacy current generation requires separately verified bundle backfill before 0003'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (SELECT 1 FROM search_index_receipts) AND
       to_regclass('public.search_legacy_generation_backfill_proof') IS NULL THEN
        RAISE EXCEPTION 'historical generation identity requires trusted host proof'
            USING ERRCODE = '23514';
    END IF;
    IF to_regclass('public.search_legacy_generation_backfill_proof') IS NOT NULL THEN
        EXECUTE $proof$
            SELECT EXISTS (
                SELECT 1 FROM public.search_legacy_generation_backfill_proof AS g
                LEFT JOIN public.search_source_ownership AS o USING (source_id)
                WHERE o.source_id IS NULL
                   OR g.tenant_owner_key IS DISTINCT FROM o.tenant_owner_key
                   OR g.source_kind IS DISTINCT FROM o.source_kind
                   OR g.activation_epoch <= 0
                   OR g.activation_epoch > o.activation_epoch
            ) OR EXISTS (
                SELECT 1 FROM public.search_index_receipts AS r
                LEFT JOIN public.search_legacy_generation_backfill_proof AS g
                  USING (source_id,generation_id)
                WHERE g.source_id IS NULL
            )
        $proof$ INTO invalid_proof;
        IF invalid_proof THEN
            RAISE EXCEPTION 'legacy generation identity proof is incomplete or inconsistent'
                USING ERRCODE = '23514';
        END IF;
    END IF;
END;
$legacy_generation_preflight$;

CREATE FUNCTION search_is_sha256_digest(value TEXT)
RETURNS BOOLEAN LANGUAGE SQL IMMUTABLE STRICT AS $digest$
    SELECT octet_length(value) = 71 AND value ~ '^sha256:[0-9a-f]{64}$'
$digest$;

CREATE FUNCTION search_valid_actor_scope_ref(value TEXT)
RETURNS BOOLEAN LANGUAGE SQL IMMUTABLE STRICT AS $scope$
    SELECT search_valid_tenant_owner_key(value)
$scope$;

CREATE TABLE search_generation_identity (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    tenant_owner_key TEXT NOT NULL,
    activation_epoch BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_generation_identity_pkey PRIMARY KEY (source_id,generation_id),
    CONSTRAINT uq_search_identity_activation UNIQUE (source_id,generation_id,activation_epoch),
    CONSTRAINT fk_search_identity_owner FOREIGN KEY (source_id,tenant_owner_key)
        REFERENCES search_source_ownership(source_id,tenant_owner_key) ON DELETE RESTRICT,
    CONSTRAINT ck_search_identity_activation CHECK (activation_epoch > 0)
);

CREATE FUNCTION search_guard_generation_identity()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $identity_guard$
DECLARE
    source_activation BIGINT;
    source_active BOOLEAN;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'generation identity is permanent' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'generation identity is immutable' USING ERRCODE = '23514';
    END IF;
    SELECT activation_epoch,registration_active INTO source_activation,source_active
    FROM public.search_source_coordination WHERE source_id = NEW.source_id;
    IF source_activation IS NULL OR source_active IS DISTINCT FROM TRUE
       OR NEW.activation_epoch <> source_activation THEN
        RAISE EXCEPTION 'new generation identity requires current active Source activation'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$identity_guard$;
DO $backfill_identity$
BEGIN
    IF to_regclass('public.search_legacy_generation_backfill_proof') IS NOT NULL THEN
        EXECUTE $proof$
            INSERT INTO public.search_generation_identity
                (source_id,generation_id,tenant_owner_key,activation_epoch,created_at)
            SELECT source_id,generation_id,tenant_owner_key,activation_epoch,clock_timestamp()
            FROM public.search_legacy_generation_backfill_proof
        $proof$;
    END IF;
END;
$backfill_identity$;
CREATE TRIGGER search_guard_generation_identity
    BEFORE INSERT OR UPDATE OR DELETE ON search_generation_identity
    FOR EACH ROW EXECUTE FUNCTION search_guard_generation_identity();

CREATE TABLE search_generation (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    activation_epoch BIGINT NOT NULL,
    state TEXT NOT NULL,
    build_kind TEXT NOT NULL,
    stage_origin TEXT NOT NULL,
    stage_event_id UUID NULL,
    stage_source_epoch BIGINT NULL,
    full_guard_token UUID NULL,
    full_build_fence BIGINT NULL,
    source_snapshot TEXT NOT NULL,
    projection_manifest JSONB NOT NULL,
    projection_manifest_digest TEXT NOT NULL,
    projection_resource_count BIGINT NOT NULL,
    bundle_version TEXT NOT NULL,
    ready_at TIMESTAMPTZ NULL,
    CONSTRAINT search_generation_pkey PRIMARY KEY (source_id,generation_id),
    CONSTRAINT uq_search_generation_full_binding
        UNIQUE (source_id,generation_id,full_guard_token,full_build_fence),
    CONSTRAINT uq_search_generation_guard_token UNIQUE (full_guard_token),
    CONSTRAINT uq_search_generation_source_fence UNIQUE (source_id,full_build_fence),
    CONSTRAINT fk_search_generation_identity
        FOREIGN KEY (source_id,generation_id,activation_epoch)
        REFERENCES search_generation_identity(source_id,generation_id,activation_epoch)
        ON DELETE RESTRICT,
    CONSTRAINT ck_search_generation_state CHECK
        (state IN ('BUILDING','READY','FAILED','DELETING')),
    CONSTRAINT ck_search_generation_kind CHECK
        (build_kind IN ('FULL','INCREMENTAL')),
    CONSTRAINT ck_search_generation_origin CHECK
        (stage_origin IN ('EVENT','MANUAL')),
    CONSTRAINT ck_search_generation_event_binding CHECK (
        (stage_origin = 'EVENT' AND stage_event_id IS NOT NULL
         AND stage_source_epoch IS NOT NULL AND stage_source_epoch > 0)
        OR (stage_origin = 'MANUAL' AND stage_event_id IS NULL AND stage_source_epoch IS NULL)
    ),
    CONSTRAINT ck_search_generation_full_binding CHECK (
        (build_kind = 'FULL' AND full_guard_token IS NOT NULL
         AND full_build_fence IS NOT NULL AND full_build_fence > 0)
        OR (build_kind = 'INCREMENTAL' AND full_guard_token IS NULL AND full_build_fence IS NULL)
    ),
    CONSTRAINT ck_search_generation_snapshot CHECK
        (octet_length(source_snapshot) BETWEEN 1 AND 1024),
    CONSTRAINT ck_search_generation_manifest CHECK (
        jsonb_typeof(projection_manifest) = 'object'
        AND projection_manifest ? 'dto_version'
        AND jsonb_typeof(projection_manifest -> 'dto_version') = 'string'
        AND COALESCE(projection_manifest ->> 'dto_version' = 'v1', FALSE)
    ),
    CONSTRAINT ck_search_generation_manifest_digest CHECK
        (search_is_sha256_digest(projection_manifest_digest)),
    CONSTRAINT ck_search_generation_resource_count CHECK
        (projection_resource_count >= 0),
    CONSTRAINT ck_search_generation_bundle_version CHECK
        (bundle_version = 'v1'),
    CONSTRAINT ck_search_generation_ready_at CHECK (
        (state = 'READY' AND ready_at IS NOT NULL)
        OR (state IN ('BUILDING','FAILED') AND ready_at IS NULL)
        OR state = 'DELETING'
    )
);

CREATE FUNCTION search_guard_generation_row()
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
           OR NOT (session_user = 'postgres' OR pg_has_role(session_user, 'search_gc', 'USAGE')) THEN
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
       AND NOT (session_user = 'postgres' OR pg_has_role(session_user, 'search_gc', 'USAGE')) THEN
        RAISE EXCEPTION 'only GC can enter DELETING' USING ERRCODE = '23514';
    END IF;
    IF NEW.state IN ('READY','FAILED') AND NEW.state <> OLD.state
       AND NOT (session_user = 'postgres'
                OR pg_has_role(session_user, 'search_coordinator', 'USAGE')) THEN
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
CREATE TRIGGER search_guard_generation_row
    BEFORE INSERT OR UPDATE OR DELETE ON search_generation
    FOR EACH ROW EXECUTE FUNCTION search_guard_generation_row();

CREATE TABLE search_full_guard_issuance (
    source_id UUID NOT NULL,
    target_generation_id UUID NOT NULL,
    guard_token UUID NOT NULL,
    build_fence BIGINT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_full_guard_issuance_pkey PRIMARY KEY (source_id,target_generation_id),
    CONSTRAINT fk_search_guard_issuance_identity FOREIGN KEY (source_id,target_generation_id)
        REFERENCES search_generation_identity(source_id,generation_id) ON DELETE RESTRICT
);
CREATE TABLE search_generation_full_guard (
    source_id UUID NOT NULL,
    target_generation_id UUID NOT NULL,
    guard_token UUID NOT NULL,
    build_fence BIGINT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_generation_full_guard_pkey PRIMARY KEY (source_id,target_generation_id),
    CONSTRAINT uq_search_guard_token UNIQUE (guard_token),
    CONSTRAINT uq_search_guard_source_fence UNIQUE (source_id,build_fence),
    CONSTRAINT ck_search_guard_fence CHECK (build_fence > 0),
    CONSTRAINT fk_search_guard_exact_target
        FOREIGN KEY (source_id,target_generation_id,guard_token,build_fence)
        REFERENCES search_generation(source_id,generation_id,full_guard_token,full_build_fence)
        ON DELETE RESTRICT
);
CREATE FUNCTION search_guard_full_guard()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $full_guard$
DECLARE
    parent_state TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        SELECT state INTO parent_state FROM public.search_generation
        WHERE source_id = NEW.source_id AND generation_id = NEW.target_generation_id;
        IF parent_state IS DISTINCT FROM 'BUILDING' THEN
            RAISE EXCEPTION 'full guard requires BUILDING target' USING ERRCODE = '23514';
        END IF;
        INSERT INTO public.search_full_guard_issuance
            (source_id,target_generation_id,guard_token,build_fence,issued_at)
        VALUES (NEW.source_id,NEW.target_generation_id,NEW.guard_token,
                NEW.build_fence,clock_timestamp())
        ON CONFLICT (source_id,target_generation_id) DO NOTHING;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'full guard cannot be reissued' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' THEN
        IF ROW(NEW.source_id,NEW.target_generation_id,NEW.guard_token,NEW.build_fence)
           IS DISTINCT FROM
           ROW(OLD.source_id,OLD.target_generation_id,OLD.guard_token,OLD.build_fence) THEN
            RAISE EXCEPTION 'full guard binding is immutable' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    SELECT state INTO parent_state FROM public.search_generation
    WHERE source_id = OLD.source_id AND generation_id = OLD.target_generation_id;
    IF parent_state = 'BUILDING' THEN
        RAISE EXCEPTION 'live build guard cannot be removed' USING ERRCODE = '23514';
    END IF;
    RETURN OLD;
END;
$full_guard$;
CREATE TRIGGER search_guard_full_guard
    BEFORE INSERT OR UPDATE OR DELETE ON search_generation_full_guard
    FOR EACH ROW EXECUTE FUNCTION search_guard_full_guard();

CREATE FUNCTION search_guard_generation_child()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $child_guard$
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
           OR NOT (session_user = 'postgres' OR pg_has_role(session_user, 'search_gc', 'USAGE')) THEN
            RAISE EXCEPTION 'child delete requires GC role and DELETING parent'
                USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF parent.state <> 'BUILDING' OR parent.build_kind <> 'FULL'
       OR NOT EXISTS (
           SELECT 1 FROM public.search_generation_full_guard AS g
           WHERE g.source_id = parent.source_id
             AND g.target_generation_id = parent.generation_id
             AND g.guard_token = parent.full_guard_token
             AND g.build_fence = parent.full_build_fence
             AND g.expires_at > clock_timestamp()
       ) THEN
        RAISE EXCEPTION 'child write requires BUILDING parent and live exact full guard'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$child_guard$;

CREATE TABLE search_generation_payload (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    kind TEXT NOT NULL,
    dto_version TEXT NOT NULL,
    payload JSONB NOT NULL,
    logical_digest TEXT NOT NULL,
    logical_count BIGINT NOT NULL,
    CONSTRAINT search_generation_payload_pkey PRIMARY KEY (source_id,generation_id,kind),
    CONSTRAINT fk_search_payload_generation FOREIGN KEY (source_id,generation_id)
        REFERENCES search_generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT ck_search_payload_kind CHECK
        (kind IN ('projection','unit_manifest','body_coverage')),
    CONSTRAINT ck_search_payload_version CHECK (dto_version = 'v1'),
    CONSTRAINT ck_search_payload_dto CHECK
        (jsonb_typeof(payload) = 'object' AND payload ? 'dto_version'
         AND jsonb_typeof(payload -> 'dto_version') = 'string'
         AND COALESCE(payload ->> 'dto_version' = dto_version, FALSE)),
    CONSTRAINT ck_search_payload_digest CHECK (search_is_sha256_digest(logical_digest)),
    CONSTRAINT ck_search_payload_count CHECK (logical_count >= 0)
);
CREATE TRIGGER search_guard_generation_payload
    BEFORE INSERT OR UPDATE OR DELETE ON search_generation_payload
    FOR EACH ROW EXECUTE FUNCTION search_guard_generation_child();

CREATE TABLE search_generation_receipt (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    source_snapshot TEXT NOT NULL,
    receipt_version TEXT NOT NULL,
    projection_digest TEXT NOT NULL,
    unit_manifest_digest TEXT NOT NULL,
    unit_count BIGINT NOT NULL,
    body_coverage_digest TEXT NOT NULL,
    body_item_count BIGINT NOT NULL,
    lexical_digest TEXT NOT NULL,
    lexical_count BIGINT NOT NULL,
    lexical_schema_version TEXT NOT NULL,
    lexical_analyzer_version TEXT NOT NULL,
    graph_input_digest TEXT NOT NULL,
    graph_input_count BIGINT NOT NULL,
    profile_set_digest TEXT NOT NULL,
    composite_digest TEXT NOT NULL,
    graph_backend TEXT NOT NULL,
    graph_schema_version TEXT NOT NULL,
    graph_mapping_digest TEXT NOT NULL,
    graph_content_digest TEXT NOT NULL,
    graph_resource_count BIGINT NOT NULL,
    graph_relation_count BIGINT NOT NULL,
    vector_receipt JSONB NULL,
    receipt_dto JSONB NOT NULL,
    CONSTRAINT search_generation_receipt_pkey PRIMARY KEY (source_id,generation_id),
    CONSTRAINT fk_search_generation_receipt FOREIGN KEY (source_id,generation_id)
        REFERENCES search_generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT ck_search_generation_receipt_version CHECK (receipt_version = 'v1'),
    CONSTRAINT ck_search_generation_receipt_snapshot CHECK
        (octet_length(source_snapshot) BETWEEN 1 AND 1024),
    CONSTRAINT ck_search_generation_receipt_digest CHECK (
        search_is_sha256_digest(projection_digest)
        AND search_is_sha256_digest(unit_manifest_digest)
        AND search_is_sha256_digest(body_coverage_digest)
        AND search_is_sha256_digest(lexical_digest)
        AND search_is_sha256_digest(graph_input_digest)
        AND search_is_sha256_digest(profile_set_digest)
        AND search_is_sha256_digest(composite_digest)
        AND search_is_sha256_digest(graph_mapping_digest)
        AND search_is_sha256_digest(graph_content_digest)
    ),
    CONSTRAINT ck_search_generation_receipt_count CHECK (
        unit_count >= 0 AND body_item_count >= 0 AND lexical_count >= 0
        AND graph_input_count >= 0 AND graph_resource_count >= 0
        AND graph_relation_count >= 0
    ),
    CONSTRAINT ck_search_generation_receipt_dto CHECK
        (jsonb_typeof(receipt_dto) = 'object'
         AND receipt_dto ? 'dto_version'
         AND jsonb_typeof(receipt_dto -> 'dto_version') = 'string'
         AND COALESCE(receipt_dto ->> 'dto_version' = receipt_version, FALSE))
);
CREATE TRIGGER search_guard_generation_receipt
    BEFORE INSERT OR UPDATE OR DELETE ON search_generation_receipt
    FOR EACH ROW EXECUTE FUNCTION search_guard_generation_child();

CREATE TABLE search_lexical_artifact (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    index_relpath TEXT NOT NULL,
    index_format_version TEXT NOT NULL,
    lexical_schema_version TEXT NOT NULL,
    tree_digest TEXT NOT NULL,
    logical_digest TEXT NOT NULL,
    searchable_doc_count BIGINT NOT NULL,
    unit_seal_digest TEXT NOT NULL,
    unit_seal_count BIGINT NOT NULL,
    finalized_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_lexical_artifact_pkey PRIMARY KEY (source_id,generation_id),
    CONSTRAINT fk_search_lexical_generation FOREIGN KEY (source_id,generation_id)
        REFERENCES search_generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT ck_search_lexical_path CHECK
        (index_relpath = 'generations/' || source_id::TEXT || '/' || generation_id::TEXT),
    CONSTRAINT ck_search_lexical_digest CHECK (
        search_is_sha256_digest(tree_digest)
        AND search_is_sha256_digest(logical_digest)
        AND search_is_sha256_digest(unit_seal_digest)
    ),
    CONSTRAINT ck_search_lexical_counts CHECK
        (searchable_doc_count >= 0 AND unit_seal_count >= 0)
);
CREATE TRIGGER search_guard_lexical_artifact
    BEFORE INSERT OR UPDATE OR DELETE ON search_lexical_artifact
    FOR EACH ROW EXECUTE FUNCTION search_guard_generation_child();

CREATE TABLE search_evaluation_lease (
    source_id UUID NOT NULL,
    lease_id UUID NOT NULL,
    evaluation_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    activation_epoch BIGINT NOT NULL,
    tenant_owner_key TEXT NOT NULL,
    actor_scope_ref TEXT NOT NULL,
    registration_revision BIGINT NOT NULL,
    visibility_revision BIGINT NOT NULL,
    access_revision BIGINT NOT NULL,
    manifest_digest TEXT NOT NULL,
    bundle_digest TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_evaluation_lease_pkey PRIMARY KEY (source_id,lease_id),
    CONSTRAINT fk_search_lease_generation FOREIGN KEY (source_id,generation_id)
        REFERENCES search_generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT fk_search_lease_owner FOREIGN KEY (source_id,tenant_owner_key)
        REFERENCES search_source_ownership(source_id,tenant_owner_key) ON DELETE RESTRICT,
    CONSTRAINT ck_search_lease_activation CHECK (activation_epoch > 0),
    CONSTRAINT ck_search_lease_owner_key CHECK
        (search_valid_tenant_owner_key(tenant_owner_key)),
    CONSTRAINT ck_search_lease_actor_ref CHECK
        (search_valid_actor_scope_ref(actor_scope_ref)),
    CONSTRAINT ck_search_lease_revisions CHECK
        (registration_revision > 0 AND visibility_revision > 0 AND access_revision > 0),
    CONSTRAINT ck_search_lease_digests CHECK
        (search_is_sha256_digest(manifest_digest)
         AND search_is_sha256_digest(bundle_digest))
);
CREATE INDEX search_evaluation_lease_by_generation
    ON search_evaluation_lease(source_id,generation_id,expires_at);
CREATE FUNCTION search_guard_evaluation_lease()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $lease_guard$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.expires_at <= clock_timestamp() OR NOT EXISTS (
            SELECT 1
            FROM public.search_generation AS g
            JOIN public.search_source_coordination AS s USING (source_id)
            JOIN public.search_source_ownership AS o USING (source_id)
            WHERE g.source_id = NEW.source_id
              AND g.generation_id = NEW.generation_id
              AND g.state = 'READY'
              AND g.activation_epoch = NEW.activation_epoch
              AND s.current_generation_id = NEW.generation_id
              AND s.current_manifest_digest = NEW.manifest_digest
              AND s.current_bundle_digest = NEW.bundle_digest
              AND s.activation_epoch = NEW.activation_epoch
              AND s.registration_active
              AND s.tenant_owner_key = NEW.tenant_owner_key
              AND s.registration_revision = NEW.registration_revision
              AND s.visibility_revision = NEW.visibility_revision
              AND o.tenant_owner_key = NEW.tenant_owner_key
              AND o.state = 'ACTIVE'
        ) THEN
            RAISE EXCEPTION 'pin requires current READY generation and active scope'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' AND
       ROW(NEW.source_id,NEW.lease_id,NEW.evaluation_id,NEW.generation_id,
           NEW.activation_epoch,NEW.tenant_owner_key,NEW.actor_scope_ref,
           NEW.registration_revision,NEW.visibility_revision,NEW.access_revision,
           NEW.manifest_digest,NEW.bundle_digest)
       IS DISTINCT FROM
       ROW(OLD.source_id,OLD.lease_id,OLD.evaluation_id,OLD.generation_id,
           OLD.activation_epoch,OLD.tenant_owner_key,OLD.actor_scope_ref,
           OLD.registration_revision,OLD.visibility_revision,OLD.access_revision,
           OLD.manifest_digest,OLD.bundle_digest) THEN
        RAISE EXCEPTION 'evaluation lease scope is immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$lease_guard$;
CREATE TRIGGER search_guard_evaluation_lease
    BEFORE INSERT OR UPDATE ON search_evaluation_lease
    FOR EACH ROW EXECUTE FUNCTION search_guard_evaluation_lease();

ALTER TABLE search_source_coordination
    ADD CONSTRAINT fk_search_source_current_generation
    FOREIGN KEY (source_id,current_generation_id)
    REFERENCES search_generation(source_id,generation_id) ON DELETE RESTRICT;

REVOKE ALL ON search_generation_identity, search_generation,
    search_full_guard_issuance, search_generation_full_guard,
    search_generation_payload, search_generation_receipt,
    search_lexical_artifact, search_evaluation_lease FROM PUBLIC;
REVOKE ALL ON FUNCTION search_is_sha256_digest(TEXT),
    search_valid_actor_scope_ref(TEXT), search_guard_generation_identity(),
    search_guard_generation_row(), search_guard_full_guard(),
    search_guard_generation_child(), search_guard_evaluation_lease()
    FROM PUBLIC;
