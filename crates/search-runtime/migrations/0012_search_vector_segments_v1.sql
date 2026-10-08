-- Segmented Vector generations (SD-T11 5, stage 3). A vector value is stored
-- once per model and embedding cache key; a Vector segment holds the entry
-- references of one Unit segment, shared by every stage whose Unit segment
-- and authority binding are the same; a stage keeps its ordered segment list.
-- Segments carry no generation: the reader stamps the stage's bundle key.
--
-- v1 stages wrote every entry and vector per generation. They are withdrawn
-- here and the maintainer rebuilds the current generation from stored values.
DELETE FROM search_vector_generation;
DELETE FROM search_vector_stage;
DROP TABLE search_vector_entry;

CREATE TABLE search_vector_value (
    model_id TEXT NOT NULL,
    cache_digest TEXT NOT NULL,
    authority_scope_key TEXT NOT NULL,
    vector BYTEA NOT NULL,
    vector_sha256 TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT search_vector_value_pkey PRIMARY KEY (model_id, cache_digest),
    CONSTRAINT ck_search_vector_value_digests CHECK (
        search_is_sha256_digest(model_id) AND search_is_sha256_digest(cache_digest)
        AND search_is_sha256_digest(vector_sha256)
    ),
    CONSTRAINT ck_search_vector_value_scope CHECK
        (octet_length(authority_scope_key) BETWEEN 1 AND 256),
    CONSTRAINT ck_search_vector_value_bytes CHECK
        (octet_length(vector) BETWEEN 4 AND 65536 AND octet_length(vector) % 4 = 0)
);

CREATE INDEX search_vector_value_by_scope ON search_vector_value (authority_scope_key);

CREATE TABLE search_vector_segment (
    segment_digest TEXT NOT NULL,
    model_id TEXT NOT NULL,
    authority_scope_key TEXT NOT NULL,
    entry_count INTEGER NOT NULL,
    entries JSONB NOT NULL,
    entries_sha256 TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT search_vector_segment_pkey PRIMARY KEY (segment_digest),
    CONSTRAINT ck_search_vector_segment_digests CHECK (
        search_is_sha256_digest(segment_digest) AND search_is_sha256_digest(model_id)
        AND search_is_sha256_digest(entries_sha256)
    ),
    CONSTRAINT ck_search_vector_segment_scope CHECK
        (octet_length(authority_scope_key) BETWEEN 1 AND 256),
    CONSTRAINT ck_search_vector_segment_entries CHECK
        (jsonb_typeof(entries) = 'array' AND entry_count = jsonb_array_length(entries))
);

CREATE INDEX search_vector_segment_by_scope ON search_vector_segment (authority_scope_key);

CREATE TABLE search_vector_stage_segment (
    index_digest TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    segment_digest TEXT NOT NULL,
    CONSTRAINT search_vector_stage_segment_pkey PRIMARY KEY (index_digest, ordinal),
    CONSTRAINT fk_search_vector_stage_segment_stage FOREIGN KEY (index_digest)
        REFERENCES search_vector_stage(index_digest) ON DELETE CASCADE,
    CONSTRAINT fk_search_vector_stage_segment_segment FOREIGN KEY (segment_digest)
        REFERENCES search_vector_segment(segment_digest) ON DELETE RESTRICT,
    CONSTRAINT ck_search_vector_stage_segment_ordinal CHECK (ordinal >= 0)
);

CREATE INDEX search_vector_stage_segment_by_segment
    ON search_vector_stage_segment (segment_digest);
