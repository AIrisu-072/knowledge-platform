-- A generation payload larger than one JSONB value can hold (the validation
-- corpus: a 1,000-document Unit manifest exceeded the 256 MiB jsonb array
-- limit) is stored as ordered chunks of its JSON text. A payload that fits is
-- still one row with chunk 0, so existing rows and readers stay valid.
ALTER TABLE search_generation_payload
    ADD COLUMN chunk INTEGER NOT NULL DEFAULT 0;
ALTER TABLE search_generation_payload
    ADD CONSTRAINT ck_search_payload_chunk CHECK (chunk >= 0);
ALTER TABLE search_generation_payload
    DROP CONSTRAINT search_generation_payload_pkey;
ALTER TABLE search_generation_payload
    ADD CONSTRAINT search_generation_payload_pkey
    PRIMARY KEY (source_id, generation_id, kind, chunk);
