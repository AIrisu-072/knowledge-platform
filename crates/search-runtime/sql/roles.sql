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

GRANT EXECUTE ON FUNCTION search_is_sha256_digest(TEXT),
    search_valid_actor_scope_ref(TEXT)
    TO search_registration, search_builder, search_coordinator,
       search_reader, search_gc;
