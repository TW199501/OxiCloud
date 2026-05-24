-- ════════════════════════════════════════════════════════════════════════════
-- ReBAC: access_grants table + lifecycle cleanup triggers + data migration
-- ════════════════════════════════════════════════════════════════════════════
-- PR 1 of the ReBAC rollout. Schema and data only — no code changes yet.
--
-- This migration:
--   1. Creates storage.access_grants (the single grant table for ReBAC)
--   2. Installs AFTER DELETE triggers so lifecycle cleanup is enforced at the
--      DB level even if a future code path bypasses the service layer
--   3. Migrates existing storage.shares permission flags into access_grants
--      rows with subject_type='token'
--
-- The storage.shares.permissions_* columns are NOT dropped here. They stay
-- until PR 5 (share_service is updated to read from access_grants instead).
-- See /Users/ed/.claude/plans/compiled-shimmying-bonbon.md → "Rollout sequencing".


-- ── 1. The grant table ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS storage.access_grants (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Subject (who has the permission)
    --   'user'     → auth.users.id
    --   'group'    → future: group membership
    --   'token'    → refers to storage.shares.id (anonymous link)
    --   'external' → future: refers to auth.external_subjects.id
    --                (Open Cloud Mesh / federated OIDC)
    subject_type    TEXT NOT NULL
        CHECK (subject_type IN ('user', 'group', 'token', 'external')),
    subject_id      UUID NOT NULL,

    -- Resource (what the permission is on)
    resource_type   TEXT NOT NULL
        CHECK (resource_type IN ('folder', 'file')),
    resource_id     UUID NOT NULL,

    -- Permission (what action is allowed)
    permission      TEXT NOT NULL
        CHECK (permission IN ('read', 'create', 'share', 'comment', 'delete', 'update')),

    -- Audit
    granted_by      UUID NOT NULL,
    granted_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (subject_type, subject_id, resource_type, resource_id, permission)
);

CREATE INDEX IF NOT EXISTS idx_grants_subject
    ON storage.access_grants (subject_type, subject_id);

CREATE INDEX IF NOT EXISTS idx_grants_resource
    ON storage.access_grants (resource_type, resource_id);

COMMENT ON TABLE storage.access_grants IS
    'ReBAC grant table — subject × resource × permission. Owner is implicit '
    'via storage.folders.user_id / storage.files.user_id (no rows here for owners).';


-- ── 2. Lifecycle cleanup triggers (defense-in-depth) ────────────────────────
-- These fire AFTER DELETE on the resource/subject tables so stale grants can
-- never outlive their target. The application layer also calls explicit
-- engine.revoke_all_for_* on the canonical paths.

CREATE OR REPLACE FUNCTION storage.cleanup_grants_on_resource_delete()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM storage.access_grants
     WHERE resource_type = TG_ARGV[0]
       AND resource_id   = OLD.id;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_cleanup_grants_folder ON storage.folders;
CREATE TRIGGER trg_cleanup_grants_folder
    AFTER DELETE ON storage.folders
    FOR EACH ROW
    EXECUTE FUNCTION storage.cleanup_grants_on_resource_delete('folder');

DROP TRIGGER IF EXISTS trg_cleanup_grants_file ON storage.files;
CREATE TRIGGER trg_cleanup_grants_file
    AFTER DELETE ON storage.files
    FOR EACH ROW
    EXECUTE FUNCTION storage.cleanup_grants_on_resource_delete('file');


CREATE OR REPLACE FUNCTION storage.cleanup_grants_on_subject_delete()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM storage.access_grants
     WHERE subject_type = TG_ARGV[0]
       AND subject_id   = OLD.id;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_cleanup_grants_user ON auth.users;
CREATE TRIGGER trg_cleanup_grants_user
    AFTER DELETE ON auth.users
    FOR EACH ROW
    EXECUTE FUNCTION storage.cleanup_grants_on_subject_delete('user');

DROP TRIGGER IF EXISTS trg_cleanup_grants_token ON storage.shares;
CREATE TRIGGER trg_cleanup_grants_token
    AFTER DELETE ON storage.shares
    FOR EACH ROW
    EXECUTE FUNCTION storage.cleanup_grants_on_subject_delete('token');


-- ── 3. Data migration from storage.shares ───────────────────────────────────
-- Each existing share row becomes one or more access_grants rows with
-- subject_type='token', subject_id=shares.id.
--
-- The old model's permission flags map to the new model as:
--   permissions_read    → ['read']
--   permissions_write   → ['read', 'create', 'update', 'delete']
--                          (write implies full mutation rights)
--   permissions_reshare → ['share']
--
-- WHERE NOT EXISTS guards make this idempotent — re-running the migration
-- won't create duplicates.

INSERT INTO storage.access_grants
    (subject_type, subject_id, resource_type, resource_id, permission, granted_by)
SELECT 'token', s.id, s.item_type, s.item_id::uuid, 'read', s.created_by
  FROM storage.shares s
 WHERE s.permissions_read
   AND NOT EXISTS (
       SELECT 1 FROM storage.access_grants g
        WHERE g.subject_type = 'token'
          AND g.subject_id   = s.id
          AND g.resource_id  = s.item_id::uuid
          AND g.permission   = 'read'
   );

INSERT INTO storage.access_grants
    (subject_type, subject_id, resource_type, resource_id, permission, granted_by)
SELECT 'token', s.id, s.item_type, s.item_id::uuid, p.perm, s.created_by
  FROM storage.shares s
  CROSS JOIN (VALUES ('read'), ('create'), ('update'), ('delete')) AS p(perm)
 WHERE s.permissions_write
   AND NOT EXISTS (
       SELECT 1 FROM storage.access_grants g
        WHERE g.subject_type = 'token'
          AND g.subject_id   = s.id
          AND g.resource_id  = s.item_id::uuid
          AND g.permission   = p.perm
   );

INSERT INTO storage.access_grants
    (subject_type, subject_id, resource_type, resource_id, permission, granted_by)
SELECT 'token', s.id, s.item_type, s.item_id::uuid, 'share', s.created_by
  FROM storage.shares s
 WHERE s.permissions_reshare
   AND NOT EXISTS (
       SELECT 1 FROM storage.access_grants g
        WHERE g.subject_type = 'token'
          AND g.subject_id   = s.id
          AND g.resource_id  = s.item_id::uuid
          AND g.permission   = 'share'
   );
