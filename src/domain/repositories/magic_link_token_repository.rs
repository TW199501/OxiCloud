//! Storage port for [`MagicLinkToken`].
//!
//! Minimal CRUD surface — magic-link tokens have only three lifecycle
//! states (`Pending`, `Used`, `Expired`) and three callers (mint at invite
//! time, redeem at click time, sweep at maintenance time). New methods
//! should be resisted until a concrete consumer needs them.

use async_trait::async_trait;
use uuid::Uuid;

use crate::common::errors::DomainError;
use crate::domain::entities::magic_link_token::MagicLinkToken;

#[async_trait]
pub trait MagicLinkTokenRepository: Send + Sync + 'static {
    /// Persist a freshly-minted pending token.
    async fn create(&self, token: &MagicLinkToken) -> Result<(), DomainError>;

    /// Look up a token by its opaque value. Returns `Ok(None)` when no row
    /// matches (use this for "unknown token" rather than treating it as an
    /// error). The caller is responsible for checking `is_redeemable()`
    /// before honouring the token.
    async fn find_by_token(&self, token: &str) -> Result<Option<MagicLinkToken>, DomainError>;

    /// Atomically transition a token from `Pending` → `Used`. Returns
    /// `Ok(true)` exactly when this call performed the transition; a
    /// concurrent redemption attempt receives `Ok(false)` and must reject
    /// the request. Implementations MUST do this in a single SQL
    /// statement (`UPDATE … WHERE status='pending' …`) — the row-level
    /// lock provided by Postgres' MVCC is what makes single-use
    /// enforcement race-free.
    async fn mark_used(&self, id: Uuid) -> Result<bool, DomainError>;

    /// Delete every token that has expired (status pending, expires_at
    /// in the past). Returns the number of rows removed; called from a
    /// background sweeper that runs on a slow cadence (≤ once per hour).
    async fn delete_expired(&self) -> Result<u64, DomainError>;

    /// Hard-delete every still-outstanding token for a user. Called by
    /// the user-lifecycle `on_user_deleted` hook so an admin's delete
    /// can't leave dangling tokens behind. Operates inside the caller's
    /// transaction so the cleanup commits atomically with the user
    /// DELETE.
    async fn delete_all_for_user_tx(
        &self,
        user_id: Uuid,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<u64, DomainError>;
}
