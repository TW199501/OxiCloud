//! Domain port for translation lookup.
//!
//! The concrete locale type lives in [`crate::common::locale::Locale`] and
//! is a string-backed newtype validated at construction against a
//! [`LocaleRegistry`] populated at startup from `static/locales/*.json`.
//!
//! This module is a thin facade: the trait + error types stay where the
//! application + infrastructure layers expect them; the type itself is
//! re-exported from `common` so the same `Locale` value flows through
//! handlers, middleware, services, and DTOs without re-wrapping.
//!
//! [`LocaleRegistry`]: crate::common::locale::LocaleRegistry

use thiserror::Error;

pub use crate::common::locale::Locale;

/// Error types for i18n service operations
#[derive(Debug, Error)]
pub enum I18nError {
    #[error("Translation key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid locale: {0}")]
    InvalidLocale(String),

    #[error("Error loading translations: {0}")]
    LoadError(String),
}

/// Result type for i18n service operations
pub type I18nResult<T> = Result<T, I18nError>;

/// Interface for i18n service (primary port).
///
/// Implementations should fall back to English when the requested
/// locale has no entry for `key`. Unknown locales (codes not in the
/// configured [`crate::common::locale::LocaleRegistry`]) are an
/// `InvalidLocale` error — callers normally avoid this by going
/// through the registry's `parse_or_default` before calling
/// `translate`.
pub trait I18nService: Send + Sync + 'static {
    /// Get a translation for a key and locale.
    async fn translate(&self, key: &str, locale: Locale) -> I18nResult<String>;

    /// Get a translation with `{{name}}`-mustache substitution applied
    /// to the resolved string. Mirrors the frontend convention in
    /// `static/js/core/i18n.js:117` so JSON values are interchangeable
    /// between front- and back-end.
    async fn translate_args(
        &self,
        key: &str,
        locale: Locale,
        args: &[(&str, &str)],
    ) -> I18nResult<String>;

    /// Load translations for a locale into the in-memory cache.
    async fn load_translations(&self, locale: Locale) -> I18nResult<()>;

    /// Available locales — typically the contents of the underlying
    /// registry. Returned in arbitrary order; callers that need a
    /// stable order should sort.
    async fn available_locales(&self) -> Vec<Locale>;

    /// True iff the given locale is in the registry.
    async fn is_supported(&self, locale: Locale) -> bool;
}
