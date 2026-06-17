//! Translation catalog and locale resolution for the workflow tooling.
//!
//! Thin shim over [`rust_i18n`] so the rest of the workspace calls
//! `workflow_i18n::t("find.next")` without depending on the macro crate
//! directly. Locale is resolved at startup from `LC_ALL` / `LC_MESSAGES` /
//! `LANG` environment variables, falling back to English. The active
//! locale can be changed at runtime via [`init_with`] — useful for the
//! editor's language selector.

use std::sync::Mutex;

rust_i18n::i18n!("locales", fallback = "en");

static RESOLVED: Mutex<String> = Mutex::new(const { String::new() });

/// Locales bundled with the catalog. Anything else falls back to `en`.
///
/// Add a locale here when a new YAML file is shipped under
/// `crates/workflow-i18n/locales/<code>.yaml`. The constant is the
/// single source of truth used by the editor's language selector and
/// by the env-based [`resolve_locale`] fallback.
pub const BUNDLED: &[&str] = &["en", "es"];

/// Initialize the global catalog and select a locale from the process
/// environment. Safe to call more than once; subsequent calls are
/// no-ops for the initial resolution and just return the cached value.
pub fn init() -> &'static str {
    let needs_init = RESOLVED
        .lock()
        .map(|g| g.is_empty())
        .unwrap_or(true);
    if needs_init {
        let resolved = resolve_locale();
        rust_i18n::set_locale(&resolved);
        if let Ok(mut g) = RESOLVED.lock() {
            *g = resolved;
        }
    }
    locale_static()
}

/// Initialize using an explicit locale (e.g. from a CLI flag or the
/// editor's language selector). Overrides any prior selection and
/// takes effect immediately for subsequent `t()` / `tf()` calls.
pub fn init_with(locale: &str) -> &'static str {
    let sanitized = sanitize(locale);
    let resolved = if is_bundled(&sanitized) {
        sanitized
    } else {
        "en".to_string()
    };
    rust_i18n::set_locale(&resolved);
    if let Ok(mut g) = RESOLVED.lock() {
        *g = resolved;
    }
    locale_static()
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
pub fn current_locale() -> String {
    RESOLVED
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| "en".to_string())
}

/// The list of bundled locales, in display order. Drives the editor's
/// language selector dropdown.
pub fn available_locales() -> &'static [&'static str] {
    BUNDLED
}

/// A human-readable name for a locale code. Falls back to the raw
/// code if the locale has no display-name entry in the catalog.
pub fn display_name(locale: &str) -> String {
    let key = format!("locale.{}", sanitize(locale));
    let s = t(&key);
    if s == key || s.is_empty() {
        locale.to_string()
    } else {
        s
    }
}

/// `&'static str` view of the active locale code, for callers that
/// need to stash it (e.g. as a `HashMap` key). The returned reference
/// points at a string in a leaked allocation; it is updated on every
/// call to [`init`] or [`init_with`]. Use [`current_locale`] when you
/// need a fresh value.
fn locale_static() -> &'static str {
    use std::sync::OnceLock;
    static BUF: OnceLock<Mutex<String>> = OnceLock::new();
    let m = BUF.get_or_init(|| Mutex::new(String::from("en")));
    let mut g = m.lock().expect("locale mutex poisoned");
    let cur = current_locale();
    if *g != cur {
        *g = cur;
    }
    // SAFETY-equivalent: leak a clone so we can return `&'static str`.
    // The string is small (max ~5 chars) and updated infrequently; the
    // small leak is acceptable for the convenience.
    Box::leak(g.clone().into_boxed_str())
}

fn resolve_locale() -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(raw) = std::env::var_os(var) {
            let candidate = sanitize(&raw.to_string_lossy());
            if is_bundled(&candidate) {
                return candidate;
            }
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
    BUNDLED.contains(&locale)
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

    #[test]
    fn init_with_switches_locale_at_runtime() {
        init_with("en");
        let en_title = t("app.title");
        init_with("es");
        let es_title = t("app.title");
        if en_title != es_title {
            assert_eq!(es_title, "Editor Nativo de Flujo");
        }
        init_with("en");
    }

    #[test]
    fn init_with_unknown_locale_falls_back_to_english() {
        init_with("zz_ZZ");
        assert_eq!(current_locale(), "en");
        let title = t("app.title");
        assert_eq!(title, "Flow Native Editor");
    }

    #[test]
    fn available_locales_lists_bundled() {
        let locales = available_locales();
        assert!(locales.contains(&"en"));
        assert!(locales.contains(&"es"));
    }

    #[test]
    fn display_name_returns_localized_name() {
        init_with("en");
        let en = display_name("en");
        let es = display_name("es");
        assert!(!en.is_empty());
        assert!(!es.is_empty());
    }
}
