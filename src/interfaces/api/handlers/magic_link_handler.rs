//! Magic-link redemption endpoint.
//!
//! Single public route: `GET /magic/v1/{token}`. Validating the token is
//! the entire authentication — the URL is the credential.
//!
//! Successful redemption:
//!   1. Atomically marks the token used (single-use, race-free).
//!   2. Issues access + refresh JWT for the token's owning user.
//!   3. Sets the standard `oxicloud_access` / `oxicloud_refresh` /
//!      `oxicloud_csrf` cookies (same as `POST /api/auth/login`).
//!   4. 302-redirects to a frontend hash-route based on the token's
//!      resource target:
//!        - Folder       → `/#/files/folder/{id}`
//!        - File or NULL → `/#/sharedwithme`
//!
//! Files don't have a deep-link route today; v1 lands file invitations
//! on Shared With Me where the file shows up.
//!
//! Failure cases (all return 4xx without setting cookies):
//!   - Token not found / expired / already used → 410 Gone.
//!   - Magic-link feature disabled (no SMTP / repo) → 503.
//!   - Owning user deactivated → 410 Gone.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header::CONTENT_TYPE, header::LOCATION},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::application::services::auth_application_service::MagicLinkRedemption;
use crate::common::di::AppState;
use crate::common::errors::ErrorKind;
use crate::domain::entities::magic_link_token::MagicLinkResourceKind;
use crate::interfaces::api::cookie_auth;

/// Build the `/magic/v1/{token}` router. Mounted at the top of the
/// application tree in `main.rs` — no auth middleware, no CSRF (the
/// token is the credential, the route is GET-only).
pub fn magic_link_routes() -> Router<Arc<AppState>> {
    Router::new().route("/magic/v1/{token}", get(redeem_magic_link))
}

#[utoipa::path(
    get,
    path = "/magic/v1/{token}",
    params(("token" = String, Path, description = "Opaque magic-link token")),
    responses(
        (status = 302, description = "Redemption succeeded — redirects to the resource or to /#/sharedwithme"),
        (status = 410, description = "Token is unknown, expired, or already used"),
        (status = 503, description = "Magic-link feature is not configured on this server"),
    ),
    tag = "magic-link",
)]
async fn redeem_magic_link(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Response {
    let Some(auth_svc) = state.auth_service.as_ref() else {
        return error_page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Authentication subsystem is not configured.",
        );
    };

    match auth_svc
        .auth_application_service
        .redeem_magic_link(&token)
        .await
    {
        Ok(redemption) => build_success_response(&state, redemption),
        Err(e) => {
            // Log the cause for ops; the user gets a generic page so the
            // outcome can't be used as an enumeration oracle.
            tracing::info!(
                target: "audit",
                event = "magic_link.redemption_failed",
                error_kind = ?e.kind,
                error = %e.message,
            );
            match e.kind {
                ErrorKind::NotImplemented => error_page(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Magic-link sign-in is not enabled on this server.",
                ),
                ErrorKind::NotFound | ErrorKind::AccessDenied => error_page(
                    StatusCode::GONE,
                    "This sign-in link is no longer valid. It may have already been \
                     used or expired. Request a fresh link from the login page.",
                ),
                _ => error_page(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong while signing you in. Please try again.",
                ),
            }
        }
    }
}

fn build_success_response(state: &Arc<AppState>, redemption: MagicLinkRedemption) -> Response {
    let target = redirect_target(redemption.resource_kind, redemption.resource_id);

    let mut response = (StatusCode::FOUND, [(LOCATION, target.as_str())]).into_response();

    cookie_auth::append_auth_cookies(
        response.headers_mut(),
        &redemption.auth.access_token,
        &redemption.auth.refresh_token,
        redemption.auth.expires_in,
        state.core.config.auth.refresh_token_expiry_secs,
    );
    cookie_auth::append_csrf_cookie(response.headers_mut(), redemption.auth.expires_in);

    response
}

/// Build the SPA hash-route the redemption should land on. Mirrors the
/// front-end's `deserializeHash()` parser at `static/js/app/main.js`.
fn redirect_target(kind: Option<MagicLinkResourceKind>, id: Option<uuid::Uuid>) -> String {
    match (kind, id) {
        (Some(MagicLinkResourceKind::Folder), Some(folder_id)) => {
            format!("/#/files/folder/{}", folder_id)
        }
        _ => "/#/sharedwithme".to_string(),
    }
}

fn error_page(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>OxiCloud</title>\
         <style>body{{font-family:system-ui,sans-serif;max-width:640px;margin:6em auto;\
         padding:0 1em;color:#333}}h1{{font-size:1.4em}}p{{line-height:1.5}}</style>\
         </head><body><h1>Sign-in link</h1><p>{}</p>\
         <p><a href=\"/\">Return to OxiCloud</a></p></body></html>",
        html_escape(message)
    );

    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

/// Tiny HTML escape — only used in the error fallback page. Anything more
/// elaborate belongs in a templating layer (not in scope here).
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
