CREATE TABLE document_semantic_inspections (
    file_id UUID NOT NULL REFERENCES file_objects(file_id),
    inspection_profile_version TEXT NOT NULL
        CHECK (inspection_profile_version = 'dsi-v0'),
    worker_protocol_version TEXT NOT NULL
        CHECK (worker_protocol_version = 'dsi-worker-v0'),
    observed_raw_content_hash BYTEA NOT NULL
        CHECK (octet_length(observed_raw_content_hash) = 32),
    observed_size_bytes BIGINT NOT NULL
        CHECK (observed_size_bytes >= 0),
    detected_format TEXT NOT NULL
        CHECK (detected_format IN (
            'docx', 'xlsx', 'xlsm', 'pptx', 'pdf', 'txt', 'csv', 'html'
        )),
    fingerprint_algorithm TEXT NOT NULL
        CHECK (fingerprint_algorithm = 'sha256'),
    fingerprint_digest BYTEA NOT NULL
        CHECK (octet_length(fingerprint_digest) = 32),
    semantic_capabilities JSONB NOT NULL
        CHECK (jsonb_typeof(semantic_capabilities) = 'array'),
    editorial_provenance JSONB NOT NULL
        CHECK (jsonb_typeof(editorial_provenance) = 'object'),
    external_dependencies JSONB NOT NULL
        CHECK (jsonb_typeof(external_dependencies) = 'array'),
    digital_signature_evidence JSONB NOT NULL
        CHECK (jsonb_typeof(digital_signature_evidence) = 'array'),
    worker_build_id TEXT NOT NULL CHECK (btrim(worker_build_id) <> ''),
    adapter_id TEXT NOT NULL CHECK (btrim(adapter_id) <> ''),
    adapter_version TEXT NOT NULL CHECK (btrim(adapter_version) <> ''),
    parser_libraries JSONB NOT NULL
        CHECK (jsonb_typeof(parser_libraries) = 'array'),
    native_dependency_identity JSONB NOT NULL
        CHECK (jsonb_typeof(native_dependency_identity) = 'array'),
    diagnostics JSONB NOT NULL
        CHECK (jsonb_typeof(diagnostics) = 'array'),
    inspected_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (file_id, inspection_profile_version)
);
