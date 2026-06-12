use workflow_domain::{TriggerRule, WorkflowError, WorkflowResult};

pub fn from_toml_str(input: &str) -> WorkflowResult<Vec<TriggerRule>> {
    let rules: Vec<TriggerRule> =
        toml::from_str(input).map_err(|e| WorkflowError::Toml(e.to_string()))?;
    Ok(rules)
}

pub fn to_toml_string(rules: &[TriggerRule]) -> WorkflowResult<String> {
    toml::to_string_pretty(rules).map_err(|e| WorkflowError::Toml(e.to_string()))
}
