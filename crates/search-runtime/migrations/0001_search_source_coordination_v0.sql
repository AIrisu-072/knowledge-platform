-- One durable Source control row is shared by Search publication and P3/P7 coordination.
-- Source registration inserts this row before any lease acquisition.
CREATE TABLE search_source_coordination (
    source_id UUID PRIMARY KEY,
    fence_epoch BIGINT NOT NULL DEFAULT 0,
    owner_token UUID NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    current_generation_id UUID NULL,
    current_manifest_digest TEXT NULL,
    current_bundle_digest TEXT NULL,
    pointer_revision BIGINT NOT NULL DEFAULT 0,
    last_published_epoch BIGINT NOT NULL DEFAULT 0,
    build_fence_seq BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT ck_search_source_fence_epoch_nonnegative CHECK (fence_epoch >= 0),
    CONSTRAINT ck_search_source_pointer_revision_nonnegative CHECK (pointer_revision >= 0),
    CONSTRAINT ck_search_source_last_published_epoch_nonnegative CHECK (last_published_epoch >= 0),
    CONSTRAINT ck_search_source_build_fence_seq_nonnegative CHECK (build_fence_seq >= 0),
    CONSTRAINT ck_search_source_published_epoch_order CHECK (last_published_epoch <= fence_epoch),
    CONSTRAINT ck_search_source_lease_complete CHECK (
        num_nonnulls(owner_token, lease_expires_at) IN (0, 2)
    ),
    CONSTRAINT ck_search_source_current_complete CHECK (
        num_nonnulls(current_generation_id, current_manifest_digest, current_bundle_digest) IN (0, 3)
    ),
    CONSTRAINT ck_search_source_current_manifest_digest CHECK (
        current_manifest_digest IS NULL OR (
            octet_length(current_manifest_digest) = 71
            AND current_manifest_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    ),
    CONSTRAINT ck_search_source_current_bundle_digest CHECK (
        current_bundle_digest IS NULL OR (
            octet_length(current_bundle_digest) = 71
            AND current_bundle_digest ~ '^sha256:[0-9a-f]{64}$'
        )
    )
);

-- Historical event receipts are metadata. generation_id deliberately has no
-- generation FK: a retired generation must not be pinned by its old receipt.
CREATE TABLE search_index_receipts (
    source_id UUID NOT NULL,
    event_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    digest TEXT NOT NULL,
    bundle_digest TEXT NOT NULL,
    fence_epoch BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_index_receipts_pkey PRIMARY KEY (source_id, event_id),
    CONSTRAINT fk_search_receipt_source FOREIGN KEY (source_id)
        REFERENCES search_source_coordination(source_id) ON DELETE RESTRICT,
    CONSTRAINT ck_search_receipt_positive_epoch CHECK (fence_epoch > 0),
    CONSTRAINT ck_search_receipt_digest CHECK (
        octet_length(digest) = 71 AND digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    CONSTRAINT ck_search_receipt_bundle_digest CHECK (
        octet_length(bundle_digest) = 71 AND bundle_digest ~ '^sha256:[0-9a-f]{64}$'
    )
);
