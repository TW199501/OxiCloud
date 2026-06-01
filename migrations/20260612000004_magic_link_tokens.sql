-- ════════════════════════════════════════════════════════════════════════════
-- Magic-link authentication tokens
-- ════════════════════════════════════════════════════════════════════════════
-- One-shot opaque tokens issued in two situations:
--
--   1. Invitation flow — an internal user shares a resource with someone by
--      email; if the recipient has no account yet, OxiCloud creates an
--      external user (`is_external = TRUE`, no password) and mints a token
--      pointed at the target resource. Mail with `/magic/v1/{token}` is
--      delivered; clicking lands on the resource directly.
--
--   2. Login-via-email flow — a user with no other credential (typically a
--      previously-invited external user) requests a fresh login link from
--      `/login`. Token has NO resource target; redemption lands on
--      `/shared-with-me`.
--
-- Tokens are 32 random bytes encoded as URL-safe base64 (43 chars). They
-- are stored in plaintext (single-use; revealed in the URL anyway) and the
-- table is indexed on `token` for O(1) redemption lookup.
--
-- Lifecycle states:
--   pending → used     (successful redemption; `used_at` stamped)
--   pending → expired  (background sweep when `expires_at < NOW()`)
--
-- The schema is intentionally close to `auth.device_codes` (initial_schema)
-- so future maintenance lessons learnt on one transfer to the other.

DO $BODY$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_type t
        JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
        WHERE t.typname = 'magic_link_status' AND n.nspname = 'auth'
    ) THEN
        CREATE TYPE auth.magic_link_status AS ENUM (
            'pending',  -- Issued, not yet redeemed
            'used',     -- Redeemed exactly once; cannot be reused
            'expired'   -- TTL exceeded without redemption
        );
    END IF;
END $BODY$;

CREATE TABLE IF NOT EXISTS auth.magic_link_tokens (
    id             UUID                     PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Plaintext base64url-encoded random bytes. The URL-as-credential model
    -- means this column is the secret; access is restricted by table-level
    -- permissions, not column-level hashing (matches device_codes).
    token          TEXT                     NOT NULL UNIQUE,
    user_id        UUID                     NOT NULL
                                            REFERENCES auth.users(id) ON DELETE CASCADE,
    status         auth.magic_link_status   NOT NULL DEFAULT 'pending',
    issued_at      TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at     TIMESTAMP WITH TIME ZONE NOT NULL,
    used_at        TIMESTAMP WITH TIME ZONE,
    -- Optional deep-link target. Both columns NULL → generic "login via
    -- email" flow, lands on /shared-with-me. Both NOT NULL → invitation
    -- flow, lands directly on /folders/{id} or /files/{id}. The XOR-on-
    -- NULL CHECK keeps the row consistent.
    resource_type  TEXT
                   CHECK (resource_type IS NULL OR resource_type IN ('file', 'folder')),
    resource_id    UUID,
    CONSTRAINT magic_link_tokens_resource_pair
        CHECK ((resource_type IS NULL) = (resource_id IS NULL))
);

-- Single-row lookup on every magic-link redemption.
CREATE INDEX IF NOT EXISTS idx_magic_link_tokens_token
    ON auth.magic_link_tokens (token);

-- Sweep of expired pending tokens (cleanup job).
CREATE INDEX IF NOT EXISTS idx_magic_link_tokens_expires_at
    ON auth.magic_link_tokens (expires_at)
    WHERE status = 'pending';

-- "List a user's outstanding tokens" (admin UI, or future
-- on_external_user_credential_set invalidation flow).
CREATE INDEX IF NOT EXISTS idx_magic_link_tokens_user_status
    ON auth.magic_link_tokens (user_id, status);

COMMENT ON TABLE auth.magic_link_tokens IS
    'One-shot opaque tokens for magic-link authentication (invitation + login-via-email flows). See migration file for the lifecycle and security model.';

COMMENT ON COLUMN auth.magic_link_tokens.token IS
    'URL-safe base64 of 32 random bytes (≈43 chars). Stored plaintext — the URL it sits in is the credential; column-level hashing would not change the threat model.';
