use workflow_domain::{TriggerValidator, WorkflowResult};
use workflow_serialize::TriggerLoader;

pub fn run(path: &str) -> WorkflowResult<()> {
    let rules = if std::path::Path::new(path).is_dir() {
        TriggerLoader::load_rules_from_dir(path)?
    } else {
        TriggerLoader::load_rule(path)?
    };

    println!("Loaded {} rule(s)", rules.len());

    let result = TriggerValidator::validate_all(&rules);

    if result.valid {
        println!("✓ All rules are valid");
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
