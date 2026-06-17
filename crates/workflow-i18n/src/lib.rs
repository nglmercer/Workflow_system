//! Translation catalog and locale resolution for the workflow tooling.
//!
//! Thin shim over [`rust_i18n`] so the rest of the workspace calls
//! `workflow_i18n::t("find.next")` without depending on the macro crate
//! directly. Locale is resolved at startup from `LC_ALL` / `LC_MESSAGES` /
//! `LANG` environment variables, falling back to English.

use std::sync::OnceLock;

rust_i18n::i18n!("locales", fallback = "en");

static RESOLVED: OnceLock<String> = OnceLock::new();

/// Locales bundled with the catalog. Anything else falls back to `en`.
const BUNDLED: &[&str] = &["en", "es"];

/// Initialize the global catalog and select a locale from the process
/// environment. Safe to call more than once; subsequent calls are no-ops
/// and return the locale resolved on the first invocation.
pub fn init() -> &'static str {
    if let Some(loc) = RESOLVED.get() {
        return loc;
    }
    let resolved = resolve_locale();
    rust_i18n::set_locale(&resolved);
    let _ = RESOLVED.set(resolved);
    RESOLVED.get().map(String::as_str).unwrap_or("en")
}

/// Initialize using an explicit locale (e.g. from a CLI flag). Overrides
/// the environment-based default for the lifetime of the process.
pub fn init_with(locale: &str) -> &'static str {
    let sanitized = sanitize(locale);
    rust_i18n::set_locale(&sanitized);
    let _ = RESOLVED.set(sanitized);
    RESOLVED.get().map(String::as_str).unwrap_or("en")
}

/// Translate a key using the current global locale. Returns the English
/// fallback if the active locale has no entry for `key`. Formatting
/// follows `rust_i18n` argument syntax (`{name}` placeholders).
pub fn t(key: &str) -> String {
    rust_i18n::t!(key).into_owned()
}

/// Translate a key with named arguments. Keys are catalog keys; values
/// are inserted into `{name}` placeholders in the resolved template.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut out = rust_i18n::t!(key).into_owned();
    for (k, v) in args {
        let needle = format!("{{{}}}", k);
        out = out.replace(&needle, v);
    }
    out
}

/// Return the active locale code (e.g. `"en"`, `"es"`).
pub fn current_locale() -> &'static str {
    RESOLVED.get().map(String::as_str).unwrap_or("en")
}

fn resolve_locale() -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(raw) = std::env::var_os(var) {
            let candidate = sanitize(&raw.to_string_lossy());
            if is_bundled(&candidate) {
                return candidate;
            }
            // Region-tolerant fallback: try the language part
            // (e.g. `es_es` → `es`).
            if let Some(underscore) = candidate.find('_') {
                let lang = &candidate[..underscore];
                if is_bundled(lang) {
                    return lang.to_string();
                }
            }
        }
    }
    "en".to_string()
}

fn is_bundled(locale: &str) -> bool {
    BUNDLED.iter().any(|b| *b == locale)
}

fn sanitize(raw: &str) -> String {
    let mut s = raw.trim().to_ascii_lowercase();
    if let Some(dot) = s.find('.') {
        s.truncate(dot);
    }
    if let Some(at) = s.find('@') {
        s.truncate(at);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        let first = init();
        let second = init();
        assert_eq!(first, second);
    }

    #[test]
    fn t_returns_english_for_known_key() {
        init_with("en");
        assert_eq!(t("app.title"), "Flow Native Editor");
    }

    #[test]
    fn tf_substitutes_args() {
        init_with("en");
        let s = tf("app.status_opened", &[("path", "example.flow")]);
        assert!(s.contains("example.flow"));
    }

    #[test]
    fn sanitize_strips_encoding_and_modifier() {
        assert_eq!(sanitize("es_ES.UTF-8"), "es_es");
        assert_eq!(sanitize("en_US@euro"), "en_us");
        assert_eq!(sanitize("  C.POSIX  "), "c");
    }

    #[test]
    fn resolve_locale_falls_back_to_language_part() {
        // Temporarily clear and re-set LANG.
        let prev = std::env::var_os("LANG");
        // SAFETY: setting a process env var is safe in a single-threaded
        // test context. Other test threads would be a problem.
        unsafe { std::env::set_var("LANG", "es_ES.UTF-8"); }
        let resolved = resolve_locale();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("LANG", v),
                None => std::env::remove_var("LANG"),
            }
        }
        assert_eq!(resolved, "es");
    }
}
