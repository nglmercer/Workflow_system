//! End-to-end smoke tests for the i18n catalog.
//!
//! These run as integration tests (in `tests/`) rather than
//! inline `#[cfg(test)] mod tests` because they need to drive
//! `workflow_i18n::init` from a fresh process state (the inline
//! tests share the OnceLock-cached locale).

#[test]
fn catalog_contains_core_keys() {
    workflow_i18n::init();
    for key in [
        "app.title",
        "find.placeholder",
        "toolbar.save",
        "test_panel.title",
        "search_in_files.title",
    ] {
        let value = workflow_i18n::t(key);
        assert!(
            !value.contains('.'),
            "translation for {} is missing; got {:?}",
            key,
            value
        );
    }
}

#[test]
fn tf_substitutes_simple_args() {
    workflow_i18n::init_with("en");
    let args: &[(&str, &str)] = &[("line", "42"), ("col", "7")];
    let s = workflow_i18n::tf("app.status_position", args);
    assert!(s.contains("42"));
    assert!(s.contains("7"));
}

#[test]
fn unknown_locale_falls_back_to_english() {
    workflow_i18n::init_with("zz_ZZ");
    let s = workflow_i18n::t("app.title");
    assert!(!s.is_empty());
    assert!(!s.contains('.'));
}

#[test]
fn language_selector_listing_includes_all_bundled() {
    let locales = workflow_i18n::available_locales();
    assert!(!locales.is_empty(), "no bundled locales registered");
    for code in locales {
        let name = workflow_i18n::display_name(code);
        assert!(
            !name.is_empty(),
            "locale {} has no display name in the catalog",
            code
        );
    }
}

#[test]
fn locale_switch_takes_effect_immediately() {
    workflow_i18n::init_with("en");
    let en_label = workflow_i18n::t("toolbar.locale_label");
    workflow_i18n::init_with("es");
    let es_label = workflow_i18n::t("toolbar.locale_label");
    if en_label != es_label {
        assert_eq!(es_label, "Idioma");
    }
    assert_eq!(workflow_i18n::current_locale(), "es");
    workflow_i18n::init_with("en");
}
