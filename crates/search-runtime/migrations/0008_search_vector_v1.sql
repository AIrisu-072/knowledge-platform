-- E (P2-07): durable Vector generations beside the P7 bundle they index.
--
-- A stage is an isolated, unpublished index of one P7 bundle key and model.
-- Its entries carry the hit reference, the cache key and the vector bytes
-- with a digest recomputed by the adapter on every read. A published
-- manifest names exactly one stage; publication is a CAS against the
-- Source's current P7 key and the authority scope's epoch, so a build that
-- began before a purge or a newer bundle never publishes after it.

CREATE TABLE search_vector_scope_epoch (
    authority_scope_key TEXT NOT NULL,
    epoch BIGINT NOT NULL,
    CONSTRAINT search_vector_scope_epoch_pkey PRIMARY KEY (authority_scope_key),
    CONSTRAINT ck_search_vector_scope_key CHECK
        (octet_length(authority_scope_key) BETWEEN 1 AND 256),
    CONSTRAINT ck_search_vector_scope_epoch CHECK (epoch > 0)
);

CREATE TABLE search_vector_stage (
    index_digest TEXT NOT NULL,
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    model_id TEXT NOT NULL,
    descriptor JSONB NOT NULL,
    entry_count INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_vector_stage_pkey PRIMARY KEY (index_digest),
    CONSTRAINT ck_search_vector_stage_digests CHECK (
        search_is_sha256_digest(index_digest) AND search_is_sha256_digest(model_id)
    ),
    CONSTRAINT ck_search_vector_stage_count CHECK (entry_count >= 0)
);

CREATE TABLE search_vector_entry (
    index_digest TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    authority_scope_key TEXT NOT NULL,
    entry_ref JSONB NOT NULL,
    vector BYTEA NOT NULL,
    vector_sha256 TEXT NOT NULL,
    CONSTRAINT search_vector_entry_pkey PRIMARY KEY (index_digest, ordinal),
    CONSTRAINT fk_search_vector_entry_stage FOREIGN KEY (index_digest)
        REFERENCES search_vector_stage(index_digest) ON DELETE CASCADE,
    CONSTRAINT ck_search_vector_entry_ordinal CHECK (ordinal >= 0),
    CONSTRAINT ck_search_vector_entry_bytes CHECK
        (octet_length(vector) BETWEEN 4 AND 65536 AND octet_length(vector) % 4 = 0),
    CONSTRAINT ck_search_vector_entry_digest CHECK (search_is_sha256_digest(vector_sha256))
);

CREATE INDEX search_vector_entry_by_scope ON search_vector_entry (authority_scope_key);

CREATE TABLE search_vector_generation (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    model_id TEXT NOT NULL,
    authority_scope_key TEXT NOT NULL,
    index_digest TEXT NOT NULL,
    manifest JSONB NOT NULL,
    published_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT search_vector_generation_pkey PRIMARY KEY (source_id, generation_id, model_id),
    CONSTRAINT fk_search_vector_generation_stage FOREIGN KEY (index_digest)
        REFERENCES search_vector_stage(index_digest) ON DELETE RESTRICT,
    CONSTRAINT ck_search_vector_generation_manifest CHECK (jsonb_typeof(manifest) = 'object')
);
