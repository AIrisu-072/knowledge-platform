-- Global Source ownership is shared by Document and Remote registration. This
-- migration is transactional. An existing 0001 Source is never assigned an
-- owner from its pointer, receipt, or SourceId.
--
-- Legacy protocol: before invoking search_runtime::migrate, a trusted host
-- authority must create and populate these two public tables on the same DB:
--   search_legacy_source_backfill_proof(
--     source_id uuid PRIMARY KEY, tenant_owner_key text, source_kind text,
--     registration_revision bigint, visibility_revision bigint,
--     activation_epoch bigint, state text, registration_dto jsonb,
--     registration_digest text)
--   search_legacy_generation_backfill_proof(
--     source_id uuid, generation_id uuid, tenant_owner_key text,
--     source_kind text, registration_revision bigint, activation_epoch bigint,
--     PRIMARY KEY(source_id,generation_id))
-- The host must derive them from its durable ownership authority, not from
-- 0001 rows. They are retained for later generation-identity reconciliation.
-- No proof, missing key, or conflicting proof aborts 0002 with no partial
-- ownership assignment, pointer clear, or migration-ledger advance.

LOCK TABLE search_source_coordination, search_index_receipts IN ACCESS EXCLUSIVE MODE;

DO $legacy_preflight$
DECLARE
    invalid_proof BOOLEAN;
BEGIN
    IF EXISTS (SELECT 1 FROM search_source_coordination) THEN
        IF to_regclass('public.search_legacy_source_backfill_proof') IS NULL
           OR to_regclass('public.search_legacy_generation_backfill_proof') IS NULL THEN
            RAISE EXCEPTION 'legacy Source ownership requires trusted host proof'
                USING ERRCODE = '23514';
        END IF;
        EXECUTE 'LOCK TABLE public.search_legacy_source_backfill_proof, public.search_legacy_generation_backfill_proof IN ACCESS EXCLUSIVE MODE';
        EXECUTE $proof$
            SELECT EXISTS (
                SELECT 1
                FROM public.search_source_coordination AS s
                FULL JOIN public.search_legacy_source_backfill_proof AS p USING (source_id)
                WHERE s.source_id IS NULL OR p.source_id IS NULL
            )
        $proof$ INTO invalid_proof;
        IF invalid_proof THEN
            RAISE EXCEPTION 'legacy Source proof does not cover exactly the existing Sources'
                USING ERRCODE = '23514';
        END IF;
        EXECUTE $proof$
            WITH referenced AS (
                SELECT source_id, current_generation_id AS generation_id
                FROM public.search_source_coordination
                WHERE current_generation_id IS NOT NULL
                UNION
                SELECT source_id, generation_id FROM public.search_index_receipts
            )
            SELECT EXISTS (
                SELECT 1 FROM referenced AS r
                LEFT JOIN public.search_legacy_generation_backfill_proof AS g
                    USING (source_id, generation_id)
                WHERE g.source_id IS NULL
            ) OR EXISTS (
                SELECT 1
                FROM public.search_legacy_generation_backfill_proof AS g
                LEFT JOIN public.search_legacy_source_backfill_proof AS p USING (source_id)
                WHERE p.source_id IS NULL
                    OR g.tenant_owner_key IS DISTINCT FROM p.tenant_owner_key
                    OR g.source_kind IS DISTINCT FROM p.source_kind
                    OR g.registration_revision IS NULL
                    OR g.registration_revision <= 0
                    OR g.registration_revision > p.registration_revision
                    OR g.activation_epoch IS NULL
                    OR g.activation_epoch <= 0
                    OR g.activation_epoch > p.activation_epoch
            )
        $proof$ INTO invalid_proof;
        IF invalid_proof THEN
            RAISE EXCEPTION 'legacy generation ownership proof is missing or inconsistent'
                USING ERRCODE = '23514';
        END IF;
    END IF;
END;
$legacy_preflight$;

-- Matches TenantId's UTF-8 byte bound, trim and Unicode Cc rejection without
-- locale-dependent POSIX character classes. Rust trim uses White_Space at the
-- first and last scalar; interior whitespace is permitted except Cc controls.
CREATE FUNCTION search_valid_tenant_owner_key(value TEXT)
RETURNS BOOLEAN LANGUAGE plpgsql IMMUTABLE AS $key_validation$
DECLARE
    i INTEGER;
    scalar_count INTEGER;
    codepoint INTEGER;
BEGIN
    IF value IS NULL OR octet_length(value) = 0 OR octet_length(value) > 256 THEN
        RETURN FALSE;
    END IF;
    scalar_count := char_length(value);
    FOR i IN 1..scalar_count LOOP
        codepoint := ascii(substr(value, i, 1));
        IF codepoint BETWEEN 0 AND 31 OR codepoint BETWEEN 127 AND 159 THEN
            RETURN FALSE;
        END IF;
        IF (i = 1 OR i = scalar_count) AND (
            codepoint = 32 OR codepoint = 160 OR codepoint = 5760
            OR codepoint BETWEEN 8192 AND 8202
            OR codepoint IN (8232, 8233, 8239, 8287, 12288)
        ) THEN
            RETURN FALSE;
        END IF;
    END LOOP;
    RETURN TRUE;
END;
$key_validation$;

CREATE TABLE search_source_ownership (
    source_id UUID PRIMARY KEY,
    tenant_owner_key TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    registration_revision BIGINT NOT NULL,
    visibility_revision BIGINT NOT NULL,
    activation_epoch BIGINT NOT NULL,
    state TEXT NOT NULL,
    registration_dto JSONB NOT NULL,
    registration_digest TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_search_source_owner UNIQUE (source_id, tenant_owner_key),
    CONSTRAINT fk_search_ownership_source FOREIGN KEY (source_id)
        REFERENCES search_source_coordination(source_id)
        ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT ck_search_owner_key CHECK (search_valid_tenant_owner_key(tenant_owner_key)),
    CONSTRAINT ck_search_source_kind CHECK (source_kind IN ('DOCUMENT', 'REMOTE')),
    CONSTRAINT ck_search_owner_registration_revision CHECK (registration_revision > 0),
    CONSTRAINT ck_search_owner_visibility_revision CHECK (visibility_revision > 0),
    CONSTRAINT ck_search_owner_activation CHECK (activation_epoch > 0),
    CONSTRAINT ck_search_owner_state CHECK (state IN ('ACTIVE', 'TOMBSTONED')),
    CONSTRAINT ck_search_owner_dto CHECK (
        jsonb_typeof(registration_dto) = 'object'
        AND registration_dto ? 'dto_version'
        AND registration_dto ->> 'dto_version' = 'v1'
        AND octet_length(registration_dto::TEXT) <= 65536
    ),
    CONSTRAINT ck_search_owner_digest CHECK (
        octet_length(registration_digest) = 71
        AND registration_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    CONSTRAINT ck_search_owner_timestamps CHECK (updated_at >= created_at)
);

CREATE TABLE search_registration_serial (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    document_deployment_revision BIGINT NULL,
    document_desired_set_digest TEXT NULL,
    remote_deployment_revision BIGINT NULL,
    remote_desired_set_digest TEXT NULL,
    CONSTRAINT ck_search_registration_singleton CHECK (singleton),
    CONSTRAINT ck_search_document_set_complete CHECK (
        num_nonnulls(document_deployment_revision, document_desired_set_digest) = 0
        OR (
            num_nonnulls(document_deployment_revision, document_desired_set_digest) = 2
            AND document_deployment_revision > 0
            AND octet_length(document_desired_set_digest) = 71
            AND document_desired_set_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    ),
    CONSTRAINT ck_search_remote_set_complete CHECK (
        num_nonnulls(remote_deployment_revision, remote_desired_set_digest) = 0
        OR (
            num_nonnulls(remote_deployment_revision, remote_desired_set_digest) = 2
            AND remote_deployment_revision > 0
            AND octet_length(remote_desired_set_digest) = 71
            AND remote_desired_set_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    )
);
INSERT INTO search_registration_serial (singleton) VALUES (TRUE);

ALTER TABLE search_source_coordination
    ADD COLUMN tenant_owner_key TEXT NULL,
    ADD COLUMN registration_revision BIGINT NULL,
    ADD COLUMN visibility_revision BIGINT NULL,
    ADD COLUMN activation_epoch BIGINT NULL,
    ADD COLUMN registration_active BOOLEAN NOT NULL DEFAULT FALSE,
    ADD CONSTRAINT ck_search_source_registration_complete CHECK (
        num_nonnulls(tenant_owner_key, registration_revision,
                     visibility_revision, activation_epoch) IN (0, 4)
        AND (tenant_owner_key IS NOT NULL OR NOT registration_active)
    ),
    ADD CONSTRAINT ck_search_source_owner_key CHECK (
        tenant_owner_key IS NULL OR search_valid_tenant_owner_key(tenant_owner_key)
    ),
    ADD CONSTRAINT ck_search_source_registration_revision CHECK (
        registration_revision IS NULL OR registration_revision > 0
    ),
    ADD CONSTRAINT ck_search_source_visibility_revision CHECK (
        visibility_revision IS NULL OR visibility_revision > 0
    ),
    ADD CONSTRAINT ck_search_source_activation CHECK (
        activation_epoch IS NULL OR activation_epoch > 0
    );

ALTER TABLE search_index_receipts
    ADD COLUMN bundle_version TEXT NULL,
    ADD CONSTRAINT ck_search_receipt_bundle_version CHECK (
        bundle_version IS NULL OR bundle_version = 'v1'
    );

-- Backfill is an all-or-nothing copy of host assertions after the exact Source
-- and referenced-generation scans above. The previous current pointer and all
-- receipts are left byte-for-byte unchanged; old receipts keep NULL version.
DO $legacy_backfill$
BEGIN
    IF EXISTS (SELECT 1 FROM search_source_coordination) THEN
        EXECUTE $proof$
            INSERT INTO public.search_source_ownership (
                source_id, tenant_owner_key, source_kind,
                registration_revision, visibility_revision, activation_epoch,
                state, registration_dto, registration_digest, created_at, updated_at
            )
            SELECT source_id, tenant_owner_key, source_kind,
                   registration_revision, visibility_revision, activation_epoch,
                   state, registration_dto, registration_digest,
                   clock_timestamp(), clock_timestamp()
            FROM public.search_legacy_source_backfill_proof
        $proof$;
        EXECUTE $proof$
            UPDATE public.search_source_coordination AS s
            SET tenant_owner_key = p.tenant_owner_key,
                registration_revision = p.registration_revision,
                visibility_revision = p.visibility_revision,
                activation_epoch = p.activation_epoch,
                registration_active = (p.state = 'ACTIVE')
            FROM public.search_legacy_source_backfill_proof AS p
            WHERE s.source_id = p.source_id
        $proof$;
    END IF;
END;
$legacy_backfill$;

ALTER TABLE search_source_coordination
    ADD CONSTRAINT fk_search_source_owner FOREIGN KEY (source_id, tenant_owner_key)
        REFERENCES search_source_ownership(source_id, tenant_owner_key)
        ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION search_guard_owner_identity()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $owner_guard$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Source ownership is permanent' USING ERRCODE = '23514';
    END IF;
    IF NEW.source_id IS DISTINCT FROM OLD.source_id
       OR NEW.tenant_owner_key IS DISTINCT FROM OLD.tenant_owner_key
       OR NEW.source_kind IS DISTINCT FROM OLD.source_kind THEN
        RAISE EXCEPTION 'SourceId, tenant owner and kind are immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.registration_revision < OLD.registration_revision
       OR NEW.visibility_revision < OLD.visibility_revision
       OR NEW.activation_epoch < OLD.activation_epoch THEN
        RAISE EXCEPTION 'Source registration revision or activation rollback'
            USING ERRCODE = '23514';
    END IF;
    IF (NEW.state IS DISTINCT FROM OLD.state
        OR NEW.registration_dto IS DISTINCT FROM OLD.registration_dto
        OR NEW.registration_digest IS DISTINCT FROM OLD.registration_digest
        OR NEW.registration_revision IS DISTINCT FROM OLD.registration_revision
        OR NEW.visibility_revision IS DISTINCT FROM OLD.visibility_revision)
       AND NEW.activation_epoch <= OLD.activation_epoch THEN
        RAISE EXCEPTION 'Source definition change requires a new activation'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$owner_guard$;
CREATE TRIGGER search_guard_owner_identity
    BEFORE UPDATE OR DELETE ON search_source_ownership
    FOR EACH ROW EXECUTE FUNCTION search_guard_owner_identity();

CREATE FUNCTION search_guard_source_id()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $source_guard$
BEGIN
    IF NEW.source_id IS DISTINCT FROM OLD.source_id THEN
        RAISE EXCEPTION 'SourceId is immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$source_guard$;
CREATE TRIGGER search_guard_source_id
    BEFORE UPDATE ON search_source_coordination
    FOR EACH ROW EXECUTE FUNCTION search_guard_source_id();

-- Both rows may be changed in one registration transaction. A standalone
-- ownership swap or Source active/revision/activation change fails at commit.
CREATE FUNCTION search_assert_source_ownership_pair()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $pair_guard$
DECLARE
    changed_source UUID;
BEGIN
    IF TG_OP = 'DELETE' THEN
        changed_source := OLD.source_id;
    ELSE
        changed_source := NEW.source_id;
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM public.search_source_coordination AS s
        JOIN public.search_source_ownership AS o USING (source_id)
        WHERE s.source_id = changed_source
          AND s.tenant_owner_key = o.tenant_owner_key
          AND s.registration_revision = o.registration_revision
          AND s.visibility_revision = o.visibility_revision
          AND s.activation_epoch = o.activation_epoch
          AND s.registration_active = (o.state = 'ACTIVE')
    ) THEN
        RAISE EXCEPTION 'Source coordination and ownership must match at commit'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$pair_guard$;
CREATE CONSTRAINT TRIGGER search_source_pair_from_coordination
    AFTER INSERT OR UPDATE OR DELETE ON search_source_coordination
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION search_assert_source_ownership_pair();
CREATE CONSTRAINT TRIGGER search_source_pair_from_ownership
    AFTER INSERT OR UPDATE OR DELETE ON search_source_ownership
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION search_assert_source_ownership_pair();

CREATE FUNCTION search_guard_registration_serial()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $serial_guard$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'registration serial row is permanent'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.singleton IS DISTINCT FROM OLD.singleton THEN
        RAISE EXCEPTION 'registration serial identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.document_deployment_revision IS NOT NULL AND (
        NEW.document_deployment_revision IS NULL
        OR NEW.document_deployment_revision < OLD.document_deployment_revision
        OR (NEW.document_deployment_revision = OLD.document_deployment_revision
            AND NEW.document_desired_set_digest IS DISTINCT FROM OLD.document_desired_set_digest)
    ) THEN
        RAISE EXCEPTION 'Document desired-set revision rollback or conflict'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.remote_deployment_revision IS NOT NULL AND (
        NEW.remote_deployment_revision IS NULL
        OR NEW.remote_deployment_revision < OLD.remote_deployment_revision
        OR (NEW.remote_deployment_revision = OLD.remote_deployment_revision
            AND NEW.remote_desired_set_digest IS DISTINCT FROM OLD.remote_desired_set_digest)
    ) THEN
        RAISE EXCEPTION 'Remote desired-set revision rollback or conflict'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$serial_guard$;
CREATE TRIGGER search_guard_registration_serial
    BEFORE UPDATE OR DELETE ON search_registration_serial
    FOR EACH ROW EXECUTE FUNCTION search_guard_registration_serial();

-- Existing receipts may be historical metadata with unknown bundle version.
-- Every new receipt must name its version, and no later update may guess an
-- old NULL version from either digest.
CREATE FUNCTION search_guard_receipt_bundle_version()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $receipt_guard$
BEGIN
    IF TG_OP = 'INSERT' AND NEW.bundle_version IS DISTINCT FROM 'v1' THEN
        RAISE EXCEPTION 'new Search receipt requires bundle version v1'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'UPDATE' AND NEW.bundle_version IS DISTINCT FROM OLD.bundle_version THEN
        RAISE EXCEPTION 'historical Search receipt bundle version is immutable'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$receipt_guard$;
CREATE TRIGGER search_guard_receipt_bundle_version
    BEFORE INSERT OR UPDATE ON search_index_receipts
    FOR EACH ROW EXECUTE FUNCTION search_guard_receipt_bundle_version();

REVOKE ALL ON search_source_ownership, search_registration_serial FROM PUBLIC;
REVOKE ALL ON FUNCTION search_guard_owner_identity(), search_guard_source_id(),
    search_assert_source_ownership_pair(), search_guard_registration_serial(),
    search_guard_receipt_bundle_version() FROM PUBLIC;
