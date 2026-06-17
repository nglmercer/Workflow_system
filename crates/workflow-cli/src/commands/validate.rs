use workflow_i18n::{t as i18n_t, tf as i18n_tf};
use workflow_domain::{TriggerValidator, WorkflowResult};
use workflow_serialize::TriggerLoader;

pub fn run(path: &str) -> WorkflowResult<()> {
    let rules = if std::path::Path::new(path).is_dir() {
        TriggerLoader::load_rules_from_dir(path)?
    } else {
        TriggerLoader::load_rule(path)?
    };

    println!("{}", i18n_tf("cli.validate_loaded", &[("count", &rules.len().to_string()), ("path", path)]));

    let result = TriggerValidator::validate_all(&rules);

    if result.valid {
        println!("{}", i18n_t("cli.validate_ok"));
        Ok(())
    } else {
        for issue in &result.issues {
            let prefix = match issue.severity {
                workflow_domain::IssueSeverity::Error => "✗ ERROR",
                workflow_domain::IssueSeverity::Warning => "⚠ WARN",
            };
            eprintln!("{} [{}]: {}", prefix, issue.field, issue.message);
        }
        Err(workflow_domain::WorkflowError::Validation(
            "Validation failed".to_string(),
        ))
    }
}
