-- Stage 4 (SD-T11 5): Graph rows as content-addressed per-document segments
-- and an ordered segment list per generation. The logical Graph digest and
-- schema are unchanged; a generation whose list is empty reads the row tables.

-- One document's Resource rows and the relations among them (or, for the
-- remainder segment, everything owned by no single document). Rows are only
-- added; the digest is the SHA-256 of the payload and every reader checks it.
CREATE TABLE search_graph.segment (
    segment_digest TEXT PRIMARY KEY,
    source_id UUID NOT NULL,
    resource_count INTEGER NOT NULL CHECK (resource_count >= 0),
    relation_count INTEGER NOT NULL CHECK (relation_count >= 0),
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ck_graph_segment_digest CHECK (segment_digest ~ '^sha256:[0-9a-f]{64}$')
);
CREATE INDEX graph_segment_by_age ON search_graph.segment(created_at);

CREATE TABLE search_graph.generation_segment (
    source_id UUID NOT NULL,
    generation_id UUID NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    segment_digest TEXT NOT NULL,
    CONSTRAINT graph_generation_segment_pkey PRIMARY KEY (source_id,generation_id,ordinal),
    CONSTRAINT fk_graph_generation_segment_parent FOREIGN KEY (source_id,generation_id)
        REFERENCES search_graph.generation(source_id,generation_id) ON DELETE RESTRICT,
    CONSTRAINT fk_graph_generation_segment_segment FOREIGN KEY (segment_digest)
        REFERENCES search_graph.segment(segment_digest) ON DELETE RESTRICT
);
CREATE INDEX graph_generation_segment_by_segment
    ON search_graph.generation_segment(segment_digest);

-- List rows are generation children: written only under a BUILDING parent
-- with a live guard, removed only by GC after DELETING.
CREATE TRIGGER graph_guard_generation_segment
    BEFORE INSERT OR UPDATE OR DELETE ON search_graph.generation_segment
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_child();

CREATE FUNCTION search_graph.guard_segment()
RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $segment$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'Graph segments are immutable' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF NOT search_graph.has_capability('search_gc') THEN
            RAISE EXCEPTION 'Graph segment removal requires GC' USING ERRCODE = '42501';
        END IF;
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$segment$;
CREATE TRIGGER graph_guard_segment
    BEFORE UPDATE OR DELETE ON search_graph.segment
    FOR EACH ROW EXECUTE FUNCTION search_graph.guard_segment();

CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.segment
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();
CREATE TRIGGER graph_refuse_truncate BEFORE TRUNCATE ON search_graph.generation_segment
    FOR EACH STATEMENT EXECUTE FUNCTION search_graph.refuse_truncate();

REVOKE ALL ON search_graph.segment, search_graph.generation_segment FROM PUBLIC;
REVOKE ALL ON FUNCTION search_graph.guard_segment() FROM PUBLIC;
