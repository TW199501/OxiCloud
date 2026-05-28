-- ── storage.files: pre-computed category_order ──────────────────────────────
-- Stores a numeric sort bucket derived from the file's mime_type so that
-- GROUP-BY-TYPE queries in the grants engine can ORDER BY an indexed integer
-- instead of evaluating a long CASE WHEN mime_type LIKE '…' chain at runtime.
--
-- Values are sparse multiples of 100 so future categories can be inserted
-- between existing ones without renumbering (e.g. "RichText" = 550).
-- Folder rows live in storage.folders and are not affected; the SQL query
-- hard-codes 0 for them.
--
-- Mapping (mirrors category_order_for() in display_helpers.rs):
--   0     → Folder  (SQL-only constant, not stored)
--   100   → Image
--   200   → Video
--   300   → Audio
--   400   → PDF
--   500   → Document
--   600   → Spreadsheet
--   700   → Presentation
--   800   → Archive
--   900   → Code
--   1000  → Markdown
--   1100  → Text
--   1200  → Installer
--   9999  → Other (default)

ALTER TABLE storage.files
    ADD COLUMN IF NOT EXISTS category_order SMALLINT NOT NULL DEFAULT 9999;

-- Backfill existing rows.  The CASE mirrors category_for() + category_order_for().
UPDATE storage.files
SET category_order = CASE
    -- Image
    WHEN mime_type LIKE 'image/%'                                                       THEN 100
    -- Video
    WHEN mime_type LIKE 'video/%'                                                       THEN 200
    -- Audio
    WHEN mime_type LIKE 'audio/%'                                                       THEN 300
    -- PDF
    WHEN mime_type = 'application/pdf'                                                  THEN 400
    -- Document
    WHEN mime_type IN (
        'application/msword',
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        'application/vnd.oasis.opendocument.text',
        'application/rtf')                                                              THEN 500
    -- Spreadsheet
    WHEN mime_type IN (
        'application/vnd.ms-excel',
        'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
        'application/vnd.oasis.opendocument.spreadsheet',
        'text/csv')                                                                     THEN 600
    -- Presentation
    WHEN mime_type IN (
        'application/vnd.ms-powerpoint',
        'application/vnd.openxmlformats-officedocument.presentationml.presentation',
        'application/vnd.oasis.opendocument.presentation')                             THEN 700
    -- Archive
    WHEN mime_type IN (
        'application/zip', 'application/x-rar-compressed', 'application/vnd.rar',
        'application/x-7z-compressed', 'application/gzip', 'application/x-tar')       THEN 800
    -- Code (application/* and text/x-* variants)
    WHEN mime_type IN (
        'application/json', 'application/javascript', 'application/typescript',
        'application/xml',  'application/sql',
        'application/x-sh', 'application/x-shellscript')
      OR mime_type LIKE 'text/x-%'
      OR mime_type LIKE 'text/html%'
      OR mime_type = 'text/css'                                                        THEN 900
    -- Markdown
    WHEN mime_type LIKE 'text/markdown%'                                               THEN 1000
    -- Text (generic)
    WHEN mime_type LIKE 'text/%'                                                       THEN 1100
    -- Installer / disk image
    WHEN mime_type IN (
        'application/x-apple-diskimage', 'application/x-ms-dos-executable',
        'application/x-msdownload',      'application/x-msi')                         THEN 1200
    -- Everything else → Other
    ELSE 9999
END;

-- Index so ORDER BY category_order is a fast index scan, not a table sort.
CREATE INDEX IF NOT EXISTS idx_files_category_order ON storage.files (category_order);
