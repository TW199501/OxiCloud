//! Filesystem-backed translation lookup.
//!
//! Reads JSON files under `static/locales/` (the same source the
//! frontend's `i18n.js` uses). Supports nested keys (`magic_link.invite.subject`
//! walks `{"magic_link":{"invite":{"subject":"…"}}}`) and falls back to
//! English when the resolved locale doesn't have the requested key.
//!
//! Locale validity is delegated to the [`LocaleRegistry`]
//! (`crate::common::locale`). This service does not maintain its own
//! list of supported codes — `available_locales` and `is_supported`
//! both consult the registry, so adding a 17th locale is a JSON-drop
//! operation with no Rust patch.

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

use crate::common::locale::{Locale, LocaleRegistry};
use crate::domain::services::i18n_service::{I18nError, I18nResult, I18nService};

/// File system implementation of the I18nService
pub struct FileSystemI18nService {
    /// Base directory containing translation files
    translations_dir: PathBuf,
    /// Validated registry of supported locale codes — built once at
    /// startup. `None` only in the [`dummy`](Self::dummy) test path.
    registry: Option<Arc<LocaleRegistry>>,
    /// Cached translations (locale code → JSON tree).
    cache: RwLock<HashMap<Locale, Value>>,
}

impl FileSystemI18nService {
    /// Create a dummy service for testing — no registry, no files on
    /// disk. Translation lookups will fail; this exists for stubs in
    /// non-i18n test code that just needs the type to compile.
    pub fn dummy() -> Self {
        Self {
            translations_dir: PathBuf::from("/tmp/dummy_translations"),
            registry: None,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Construct a service rooted at `translations_dir`. The
    /// [`LocaleRegistry`] should be the one built at boot (see
    /// `common/di.rs`) — it gates which locale codes are accepted by
    /// `is_supported` / `available_locales`.
    pub fn new(translations_dir: PathBuf, registry: Arc<LocaleRegistry>) -> Self {
        Self {
            translations_dir,
            registry: Some(registry),
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get translation file path for a locale.
    fn locale_file_path(&self, locale: &Locale) -> PathBuf {
        self.translations_dir
            .join(format!("{}.json", locale.as_str()))
    }

    /// Walk a dotted key (`"server.magic_link.subject"`) against a
    /// JSON tree, returning the matched string if every segment
    /// resolves and the terminal value is a string.
    fn lookup_nested<'a>(data: &'a Value, key: &str) -> Option<&'a str> {
        let mut current = data;
        for part in key.split('.') {
            current = current.get(part)?;
        }
        current.as_str()
    }

    /// Apply `{{name}}` substitutions to `template` using the
    /// (name, value) pairs in `args`. Mirrors the frontend regex
    /// `/\{\{\s*([^}]+)\s*\}\}/g` (see `static/js/core/i18n.js:117`):
    /// unmatched placeholders are left intact, whitespace inside
    /// `{{ … }}` is ignored, and the substitution is single-pass so
    /// values containing `{{x}}` won't be re-expanded.
    fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
        if args.is_empty() || !template.contains("{{") {
            return template.to_string();
        }
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open) = rest.find("{{") {
            out.push_str(&rest[..open]);
            let after_open = &rest[open + 2..];
            let Some(close) = after_open.find("}}") else {
                // No closing braces — copy the remainder verbatim.
                out.push_str("{{");
                out.push_str(after_open);
                return out;
            };
            let name = after_open[..close].trim();
            let after_close = &after_open[close + 2..];
            if let Some((_, value)) = args.iter().find(|(n, _)| *n == name) {
                out.push_str(value);
            } else {
                // Unknown placeholder — preserve the literal so it's
                // obvious in QA that a key wasn't passed.
                out.push_str("{{");
                out.push_str(&after_open[..close]);
                out.push_str("}}");
            }
            rest = after_close;
        }
        out.push_str(rest);
        out
    }
}

impl I18nService for FileSystemI18nService {
    async fn translate(&self, key: &str, locale: Locale) -> I18nResult<String> {
        // First attempt — the requested locale, cached or freshly loaded.
        {
            let cache = self.cache.read().await;
            if let Some(translations) = cache.get(&locale)
                && let Some(value) = Self::lookup_nested(translations, key)
            {
                return Ok(value.to_string());
            }
            // Fall back to English while we still hold the read lock.
            let english = Locale::english();
            if locale != english
                && let Some(translations) = cache.get(&english)
                && let Some(value) = Self::lookup_nested(translations, key)
            {
                return Ok(value.to_string());
            }
        }

        // Cold-load the requested locale and try once more.
        self.load_translations(locale.clone()).await?;
        {
            let cache = self.cache.read().await;
            if let Some(translations) = cache.get(&locale)
                && let Some(value) = Self::lookup_nested(translations, key)
            {
                return Ok(value.to_string());
            }
            let english = Locale::english();
            if locale != english
                && let Some(translations) = cache.get(&english)
                && let Some(value) = Self::lookup_nested(translations, key)
            {
                return Ok(value.to_string());
            }
        }
        Err(I18nError::KeyNotFound(key.to_string()))
    }

    async fn translate_args(
        &self,
        key: &str,
        locale: Locale,
        args: &[(&str, &str)],
    ) -> I18nResult<String> {
        let template = self.translate(key, locale).await?;
        Ok(Self::interpolate(&template, args))
    }

    async fn load_translations(&self, locale: Locale) -> I18nResult<()> {
        let file_path = self.locale_file_path(&locale);
        tracing::debug!(
            target: "oxicloud::i18n",
            "Loading translations for locale {} from {:?}",
            locale.as_str(),
            file_path
        );

        if !file_path.exists() {
            return Err(I18nError::InvalidLocale(locale.as_str().to_string()));
        }

        let content = fs::read_to_string(&file_path)
            .await
            .map_err(|e| I18nError::LoadError(format!("Failed to read translation file: {}", e)))?;

        let translations: Value = serde_json::from_str(&content).map_err(|e| {
            I18nError::LoadError(format!("Failed to parse translation file: {}", e))
        })?;

        {
            let mut cache = self.cache.write().await;
            cache.insert(locale, translations);
        }
        Ok(())
    }

    async fn available_locales(&self) -> Vec<Locale> {
        match &self.registry {
            Some(reg) => reg.iter().collect(),
            None => vec![Locale::english()],
        }
    }

    async fn is_supported(&self, locale: Locale) -> bool {
        match &self.registry {
            Some(reg) => reg.parse(locale.as_str()).is_some(),
            None => locale.is_english(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_replaces_named_placeholders() {
        let out = FileSystemI18nService::interpolate(
            "Hello {{name}}, you have {{count}} new messages.",
            &[("name", "Alice"), ("count", "3")],
        );
        assert_eq!(out, "Hello Alice, you have 3 new messages.");
    }

    #[test]
    fn interpolate_tolerates_whitespace_inside_braces() {
        let out = FileSystemI18nService::interpolate("hi {{  who  }}", &[("who", "there")]);
        assert_eq!(out, "hi there");
    }

    #[test]
    fn interpolate_preserves_unmatched_placeholders() {
        // Caller forgot to pass `name` — keep the literal in the output
        // so QA can see what was missed.
        let out = FileSystemI18nService::interpolate("Hello {{name}}", &[]);
        assert_eq!(out, "Hello {{name}}");
    }

    #[test]
    fn interpolate_is_single_pass() {
        // A value containing `{{x}}` should NOT be re-expanded —
        // otherwise an untrusted arg could trigger key lookup.
        let out =
            FileSystemI18nService::interpolate("{{greeting}}", &[("greeting", "Hello {{name}}")]);
        assert_eq!(out, "Hello {{name}}");
    }
}
