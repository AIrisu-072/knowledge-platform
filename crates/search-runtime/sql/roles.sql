-- Apply after Search migration 0003 with the database owner. These NOLOGIN
-- capability roles are granted to deployment-specific LOGIN principals.
DO $roles$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'search_registration') THEN
        CREATE ROLE search_registration NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'search_builder') THEN
        CREATE ROLE search_builder NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'search_coordinator') THEN
        CREATE ROLE search_coordinator NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'search_reader') THEN
        CREATE ROLE search_reader NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'search_gc') THEN
        CREATE ROLE search_gc NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'search_host_publisher') THEN
        CREATE ROLE search_host_publisher NOLOGIN;
    END IF;
END;
$roles$;

REVOKE ALL ON search_source_coordination, search_index_receipts,
    search_source_ownership, search_registration_serial,
    search_generation_identity, search_generation,
    search_full_guard_issuance, search_generation_full_guard,
    search_generation_payload, search_generation_receipt,
    search_lexical_artifact, search_evaluation_lease FROM PUBLIC;

GRANT USAGE ON SCHEMA public TO search_registration, search_builder,
    search_coordinator, search_reader, search_gc;

GRANT SELECT ON search_source_coordination, search_source_ownership,
    search_registration_serial TO search_registration;
GRANT INSERT ON search_source_coordination, search_source_ownership
    TO search_registration;
GRANT UPDATE (tenant_owner_key,registration_revision,visibility_revision,
              activation_epoch,registration_active,fence_epoch,
              owner_token,lease_expires_at)
    ON search_source_coordination TO search_registration;
GRANT UPDATE (registration_revision,visibility_revision,activation_epoch,
              state,registration_dto,registration_digest,updated_at)
    ON search_source_ownership TO search_registration;
GRANT UPDATE (document_deployment_revision,document_desired_set_digest,
              remote_deployment_revision,remote_desired_set_digest)
    ON search_registration_serial TO search_registration;

GRANT SELECT ON search_source_coordination, search_source_ownership,
    search_generation_identity, search_generation, search_generation_full_guard,
    search_generation_payload, search_generation_receipt, search_lexical_artifact
    TO search_builder;
GRANT INSERT, UPDATE ON search_generation_payload,
    search_generation_receipt, search_lexical_artifact TO search_builder;

GRANT SELECT ON search_source_coordination, search_source_ownership,
    search_registration_serial, search_generation_identity,
    search_generation, search_full_guard_issuance, search_generation_full_guard,
    search_generation_payload, search_generation_receipt,
    search_lexical_artifact, search_evaluation_lease, search_index_receipts
    TO search_coordinator;
GRANT UPDATE (fence_epoch,owner_token,lease_expires_at,
              current_generation_id,current_manifest_digest,current_bundle_digest,
              pointer_revision,last_published_epoch,build_fence_seq)
    ON search_source_coordination TO search_coordinator;
GRANT INSERT ON search_generation_identity, search_generation,
    search_generation_full_guard, search_evaluation_lease,
    search_index_receipts TO search_coordinator;
GRANT UPDATE (state,ready_at) ON search_generation TO search_coordinator;
GRANT UPDATE (expires_at) ON search_generation_full_guard,
    search_evaluation_lease TO search_coordinator;
GRANT DELETE ON search_generation_full_guard,
    search_evaluation_lease TO search_coordinator;

GRANT SELECT ON search_source_coordination, search_source_ownership,
    search_registration_serial, search_generation_identity,
    search_generation, search_generation_payload, search_generation_receipt,
    search_lexical_artifact, search_evaluation_lease, search_index_receipts
    TO search_reader;

GRANT SELECT ON search_source_coordination, search_source_ownership,
    search_generation_identity, search_generation,
    search_generation_full_guard, search_generation_payload,
    search_generation_receipt, search_lexical_artifact,
    search_evaluation_lease TO search_gc;
GRANT UPDATE (state) ON search_generation TO search_gc;
GRANT DELETE ON search_generation_full_guard, search_generation_payload,
    search_generation_receipt, search_lexical_artifact,
    search_evaluation_lease, search_generation TO search_gc;
-- Migration 0011: segmented Unit manifests.
GRANT SELECT, INSERT ON search_unit_segment, search_generation_segment TO search_builder;
GRANT SELECT ON search_unit_segment, search_generation_segment
    TO search_coordinator, search_reader;
GRANT SELECT, DELETE ON search_unit_segment, search_generation_segment TO search_gc;
-- Migration 0004: lock-only access to the Source row for GC.
REVOKE ALL ON FUNCTION search_gc_lock_source(UUID) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION search_gc_lock_source(UUID) TO search_gc;

GRANT EXECUTE ON FUNCTION search_is_sha256_digest(TEXT),
    search_valid_actor_scope_ref(TEXT)
    TO search_registration, search_builder, search_coordinator,
       search_reader, search_gc;

-- P6の固定方針: Searchはoutboxをロックして読むだけ。配送完了列を更新しない。
-- Searchだけのスキーマ検査DBにはDomain表がないため、この付与を保留する。
-- EVENTを構成するDBではDomain移行後に本スクリプトを適用する。
DO $search_outbox_lock_role$
BEGIN
    IF to_regclass('public.outbox_events') IS NOT NULL THEN
        GRANT SELECT ON public.outbox_events TO search_coordinator;
        GRANT UPDATE (lease_token) ON public.outbox_events TO search_coordinator;
    END IF;
END;
$search_outbox_lock_role$;

-- B5: only the host inventory publisher writes the inventory and its Audit
-- event; registration and readers only read the inventory.
REVOKE ALL ON search_host_inventory_revision, search_host_inventory_head,
    search_audit_outbox_events FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO search_host_publisher;
GRANT SELECT, INSERT ON search_host_inventory_revision TO search_host_publisher;
GRANT SELECT, INSERT, UPDATE (deployment_epoch, authority_revision, inventory_digest, updated_at)
    ON search_host_inventory_head TO search_host_publisher;
GRANT INSERT ON search_audit_outbox_events TO search_host_publisher;
GRANT SELECT ON search_host_inventory_revision, search_host_inventory_head
    TO search_registration, search_reader;
