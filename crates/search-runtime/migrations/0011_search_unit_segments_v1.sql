-- Segmented generations (SD-T11 5). A Unit manifest is stored as one
-- content-addressed, immutable segment per authoritative item, shared by
-- every generation that contains the same item, and each generation keeps
-- its ordered list of segment digests. The writer verifies a segment once
-- before inserting it; a segment is never updated and is deleted only by GC
-- after no generation lists it.
CREATE TABLE search_unit_segment (
    segment_digest TEXT NOT NULL,
    dto_version TEXT NOT NULL,
    payload JSONB NOT NULL,
    unit_count BIGINT NOT NULL,
    verified_build TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT search_unit_segment_pkey PRIMARY KEY (segment_digest),
    CONSTRAINT ck_search_unit_segment_digest CHECK (search_is_sha256_digest(segment_digest)),
    CONSTRAINT ck_search_unit_segment_version CHECK (dto_version = 'v1'),
    CONSTRAINT ck_search_unit_segment_dto CHECK
        (jsonb_typeof(payload) = 'object' AND payload ? 'dto_version'
         AND COALESCE(payload ->> 'dto_version' = dto_version, FALSE)),
    CONSTRAINT ck_search_unit_segment_count CHECK (unit_count >= 0),
    CONSTRAINT ck_search_unit_segment_build CHECK
        (octet_length(verified_build) BETWEEN 1 AND 256)
);

CREATE OR REPLACE FUNCTION search_guard_unit_segment()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp AS $segment_guard$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'unit segment is immutable' USING ERRCODE = '23514';
    END IF;
    IF NOT (session_user = 'postgres'
            OR pg_catalog.pg_has_role(session_user, 'search_gc'::name, 'USAGE'::text)) THEN
        RAISE EXCEPTION 'unit segment delete requires GC role' USING ERRCODE = '23514';
    END IF;
    RETURN OLD;
END;
$segment_guard$;
CREATE TRIGGER search_guard_unit_segment
    BEFORE UPDATE OR DELETE ON search_unit_segment
    FOR EACH ROW EXECUTE FUNCTION search_guard_unit_segment();

-- The ordered segment list of one generation. Rows follow the same parent
-- guard as the other generation children: written only while the parent is
-- BUILDING under its live guard, deleted only by GC from a DELETING parent.
CREATE TABLE search_generation_segment (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    ordinal INTEGER NOT NULL,
    segment_digest TEXT NOT NULL,
    CONSTRAINT search_generation_segment_pkey PRIMARY KEY (source_id, generation_id, ordinal),
    CONSTRAINT fk_search_generation_segment_generation FOREIGN KEY (source_id, generation_id)
        REFERENCES search_generation(source_id, generation_id) ON DELETE RESTRICT,
    CONSTRAINT fk_search_generation_segment_segment FOREIGN KEY (segment_digest)
        REFERENCES search_unit_segment(segment_digest) ON DELETE RESTRICT,
    CONSTRAINT ck_search_generation_segment_ordinal CHECK (ordinal >= 0)
);
CREATE INDEX search_generation_segment_digest
    ON search_generation_segment (segment_digest);
CREATE TRIGGER search_guard_generation_segment
    BEFORE INSERT OR UPDATE OR DELETE ON search_generation_segment
    FOR EACH ROW EXECUTE FUNCTION search_guard_generation_child();

REVOKE ALL ON search_unit_segment, search_generation_segment FROM PUBLIC;
REVOKE ALL ON FUNCTION search_guard_unit_segment() FROM PUBLIC;
