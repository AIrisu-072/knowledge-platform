-- B5 (P7-R01P/R02): the trusted host registration inventory and the Search
-- Audit source it writes in the same commit.
--
-- One immutable revision row holds every tenant of one authority revision
-- (tenants without registrations included) and both namespaces' operator
-- input. The single head names the current revision and moves only forward.
-- The head is a reference to an inventory revision, never a second Source
-- ledger. search_audit_outbox_events is an append-only typed Audit source
-- with its own delivery state, separate from the Document audit_outbox_events.

CREATE TABLE search_host_inventory_revision (
    deployment_epoch BIGINT NOT NULL,
    authority_revision BIGINT NOT NULL,
    schema_version TEXT NOT NULL,
    tenant_roster_digest TEXT NOT NULL,
    document_set_digest TEXT NOT NULL,
    remote_set_digest TEXT NOT NULL,
    inventory_digest TEXT NOT NULL,
    tenant_count INTEGER NOT NULL,
    document_count INTEGER NOT NULL,
    remote_count INTEGER NOT NULL,
    inventory_dto JSONB NOT NULL,
    writer_ref TEXT NOT NULL,
    published_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_host_inventory_revision_pkey
        PRIMARY KEY (deployment_epoch, authority_revision),
    CONSTRAINT ck_host_inventory_revision_positive
        CHECK (deployment_epoch > 0 AND authority_revision > 0),
    CONSTRAINT ck_host_inventory_schema CHECK (schema_version = 'v1'),
    CONSTRAINT ck_host_inventory_digests CHECK (
        search_is_sha256_digest(tenant_roster_digest)
        AND search_is_sha256_digest(document_set_digest)
        AND search_is_sha256_digest(remote_set_digest)
        AND search_is_sha256_digest(inventory_digest)
    ),
    CONSTRAINT ck_host_inventory_counts CHECK
        (tenant_count > 0 AND document_count >= 0 AND remote_count >= 0),
    CONSTRAINT ck_host_inventory_dto CHECK (
        jsonb_typeof(inventory_dto) = 'object'
        AND COALESCE(inventory_dto ->> 'dto_version' = 'v1', FALSE)
        AND octet_length(inventory_dto::text) <= 4194304
    ),
    CONSTRAINT ck_host_inventory_writer CHECK
        (writer_ref ~ '^[A-Za-z0-9._:@/-]{1,128}$')
);

CREATE TABLE search_host_inventory_head (
    singleton BOOLEAN NOT NULL DEFAULT TRUE,
    deployment_epoch BIGINT NOT NULL,
    authority_revision BIGINT NOT NULL,
    inventory_digest TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_host_inventory_head_pkey PRIMARY KEY (singleton),
    CONSTRAINT ck_host_inventory_head_singleton CHECK (singleton),
    CONSTRAINT fk_host_inventory_head_revision
        FOREIGN KEY (deployment_epoch, authority_revision)
        REFERENCES search_host_inventory_revision(deployment_epoch, authority_revision)
        ON DELETE RESTRICT
);

CREATE TABLE search_audit_outbox_events (
    event_id UUID NOT NULL,
    schema_version TEXT NOT NULL,
    event_class TEXT NOT NULL,
    event_type TEXT NOT NULL,
    origin_component TEXT NOT NULL,
    actor_kind TEXT NOT NULL,
    actor_ref TEXT NULL,
    subject_kind TEXT NOT NULL,
    subject_ref TEXT NULL,
    result TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    delivered_at TIMESTAMPTZ NULL,
    CONSTRAINT search_audit_outbox_events_pkey PRIMARY KEY (event_id),
    CONSTRAINT ck_search_audit_schema CHECK (schema_version = 'v1'),
    CONSTRAINT ck_search_audit_class CHECK (event_class IN
        ('CONFIGURATION','PRIVILEGED_OPERATION','SYSTEM_AUDIT','SECURITY')),
    CONSTRAINT ck_search_audit_type CHECK (event_type IN
        ('host.registration.changed')),
    CONSTRAINT ck_search_audit_origin CHECK (origin_component IN
        ('search.host_inventory')),
    CONSTRAINT ck_search_audit_actor CHECK (actor_kind IN
        ('VerifiedPrincipal','SystemComponent')),
    CONSTRAINT ck_search_audit_subject CHECK (subject_kind IN
        ('HostInventory','AuditPolicy','MaintenanceOperation','SystemComponent','UnknownTarget')),
    CONSTRAINT ck_search_audit_result CHECK (result IN ('SUCCEEDED','DENIED','FAILED')),
    CONSTRAINT ck_search_audit_reason CHECK (reason_code ~ '^[A-Z][A-Z0-9_]{0,63}$'),
    CONSTRAINT ck_search_audit_refs CHECK (
        (actor_ref IS NULL OR actor_ref ~ '^[A-Za-z0-9._:@/-]{1,128}$')
        AND (subject_ref IS NULL OR subject_ref ~ '^[A-Za-z0-9._:@/-]{1,128}$')
    ),
    CONSTRAINT ck_search_audit_delivery CHECK (attempt_count >= 0)
);

CREATE FUNCTION search_guard_host_inventory()
RETURNS TRIGGER LANGUAGE plpgsql
SET search_path = pg_catalog, public, pg_temp AS $host_inventory$
BEGIN
    IF TG_TABLE_NAME = 'search_host_inventory_revision' THEN
        RAISE EXCEPTION 'host inventory revisions are immutable' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'host inventory head is never removed' USING ERRCODE = '23514';
    END IF;
    IF (NEW.deployment_epoch, NEW.authority_revision)
       <= (OLD.deployment_epoch, OLD.authority_revision) THEN
        RAISE EXCEPTION 'host inventory head only moves forward' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$host_inventory$;

CREATE TRIGGER search_guard_host_inventory_revision
    BEFORE UPDATE OR DELETE ON search_host_inventory_revision
    FOR EACH ROW EXECUTE FUNCTION search_guard_host_inventory();
CREATE TRIGGER search_guard_host_inventory_head
    BEFORE UPDATE OR DELETE ON search_host_inventory_head
    FOR EACH ROW EXECUTE FUNCTION search_guard_host_inventory();

CREATE FUNCTION search_guard_audit_source()
RETURNS TRIGGER LANGUAGE plpgsql
SET search_path = pg_catalog, public, pg_temp AS $audit_source$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Search Audit events are append-only' USING ERRCODE = '23514';
    END IF;
    IF ROW(NEW.event_id, NEW.schema_version, NEW.event_class, NEW.event_type,
           NEW.origin_component, NEW.actor_kind, NEW.actor_ref, NEW.subject_kind,
           NEW.subject_ref, NEW.result, NEW.reason_code, NEW.occurred_at)
       IS DISTINCT FROM
       ROW(OLD.event_id, OLD.schema_version, OLD.event_class, OLD.event_type,
           OLD.origin_component, OLD.actor_kind, OLD.actor_ref, OLD.subject_kind,
           OLD.subject_ref, OLD.result, OLD.reason_code, OLD.occurred_at) THEN
        RAISE EXCEPTION 'Search Audit event content is immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.delivered_at IS NOT NULL AND NEW.delivered_at IS DISTINCT FROM OLD.delivered_at THEN
        RAISE EXCEPTION 'delivered Search Audit event is settled' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$audit_source$;

CREATE TRIGGER search_guard_audit_source
    BEFORE UPDATE OR DELETE ON search_audit_outbox_events
    FOR EACH ROW EXECUTE FUNCTION search_guard_audit_source();

CREATE INDEX search_audit_outbox_undelivered
    ON search_audit_outbox_events (occurred_at)
    WHERE delivered_at IS NULL;
