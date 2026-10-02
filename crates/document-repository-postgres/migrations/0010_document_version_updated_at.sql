ALTER TABLE document_versions
    ADD COLUMN updated_at TIMESTAMPTZ;

-- Historical edit times were not recorded. Preserve the only proven timestamp
-- instead of deriving a later value from current lifecycle state.
UPDATE document_versions
SET updated_at = created_at;

ALTER TABLE document_versions
    ALTER COLUMN updated_at SET DEFAULT now(),
    ALTER COLUMN updated_at SET NOT NULL;

CREATE OR REPLACE FUNCTION maintain_document_version_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.updated_at := NEW.created_at;
        RETURN NEW;
    END IF;

    IF (to_jsonb(NEW) - 'updated_at') IS DISTINCT FROM
       (to_jsonb(OLD) - 'updated_at') THEN
        NEW.updated_at := GREATEST(clock_timestamp(), OLD.updated_at);
    ELSE
        NEW.updated_at := OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER document_versions_updated_at
    BEFORE INSERT OR UPDATE ON document_versions
    FOR EACH ROW EXECUTE FUNCTION maintain_document_version_updated_at();
