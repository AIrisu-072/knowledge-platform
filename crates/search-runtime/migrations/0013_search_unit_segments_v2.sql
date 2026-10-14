-- Unit segments v2. A segment is stored as compact JSON text: each distinct
-- Unit context (version, Part, provenance) once and no Unit text digests,
-- which are recomputed when a process first reads the segment. Reading text
-- needs no jsonb conversion in the database; lz4 compresses the stored rows.
-- Before release: existing segments are discarded, so every generation
-- listing them fails re-verification until the next full build writes them
-- again.
TRUNCATE search_generation_segment, search_unit_segment;
ALTER TABLE search_unit_segment DROP CONSTRAINT ck_search_unit_segment_dto;
ALTER TABLE search_unit_segment DROP CONSTRAINT ck_search_unit_segment_version;
ALTER TABLE search_unit_segment ALTER COLUMN payload TYPE TEXT USING payload::text;
ALTER TABLE search_unit_segment ALTER COLUMN payload SET COMPRESSION lz4;
ALTER TABLE search_unit_segment ADD CONSTRAINT ck_search_unit_segment_version
    CHECK (dto_version = 'v2');
ALTER TABLE search_unit_segment ADD CONSTRAINT ck_search_unit_segment_payload
    CHECK (octet_length(payload) > 0);
