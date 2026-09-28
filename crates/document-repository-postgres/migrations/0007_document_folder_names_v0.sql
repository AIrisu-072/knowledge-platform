-- Apply only after preflight_folder_names reports no findings and Folder writes
-- are stopped for the maintenance window. No existing name is rewritten.
DO $$
BEGIN
    IF (SELECT count(*) FROM folders WHERE parent_folder_id IS NULL) <> 1
       OR NOT EXISTS (
           SELECT 1 FROM folders
           WHERE folder_id = '00000000-0000-7000-8000-000000000001'
             AND parent_folder_id IS NULL
       ) THEN
        RAISE EXCEPTION 'folder root preflight failed';
    END IF;
    IF EXISTS (
        SELECT 1 FROM folders
        WHERE name <> normalize(name, NFC)
           OR name <> btrim(name)
           OR char_length(name) NOT BETWEEN 1 AND 255
           OR name IN ('.', '..')
           OR position('/' IN name) > 0
           OR position(chr(92) IN name) > 0
           OR name ~ '[[:cntrl:]]'
    ) THEN
        RAISE EXCEPTION 'folder name preflight failed';
    END IF;
    IF EXISTS (
        SELECT 1 FROM folders
        WHERE parent_folder_id IS NOT NULL
        GROUP BY parent_folder_id, (normalize(name, NFC) COLLATE "C")
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION 'folder name collision preflight failed';
    END IF;
    IF (
        WITH RECURSIVE rooted(folder_id) AS (
            SELECT folder_id FROM folders
            WHERE folder_id = '00000000-0000-7000-8000-000000000001'
            UNION
            SELECT child.folder_id FROM folders child
            JOIN rooted parent ON child.parent_folder_id = parent.folder_id
        )
        SELECT count(*) FROM rooted
    ) <> (SELECT count(*) FROM folders) THEN
        RAISE EXCEPTION 'folder hierarchy preflight failed';
    END IF;
END $$;

ALTER TABLE folders ADD CONSTRAINT ck_folders_canonical_name_v0 CHECK (
    name = normalize(name, NFC)
    AND name = btrim(name)
    AND char_length(name) BETWEEN 1 AND 255
    AND name NOT IN ('.', '..')
    AND position('/' IN name) = 0
    AND position(chr(92) IN name) = 0
    AND name !~ '[[:cntrl:]]'
);

CREATE UNIQUE INDEX uq_folders_parent_canonical_name_v0
    ON folders(parent_folder_id, (normalize(name, NFC) COLLATE "C"))
    WHERE parent_folder_id IS NOT NULL;

CREATE UNIQUE INDEX uq_folders_single_root_v0
    ON folders ((true)) WHERE parent_folder_id IS NULL;
