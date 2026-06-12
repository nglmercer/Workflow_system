use workflow_domain::{TriggerRule, WorkflowError, WorkflowResult};

pub fn from_yaml_str(input: &str) -> WorkflowResult<Vec<TriggerRule>> {
    let rules: Vec<TriggerRule> =
        serde_yaml::from_str(input).map_err(|e| WorkflowError::Yaml(e.to_string()))?;
    Ok(rules)
}

pub fn to_yaml_string(rules: &[TriggerRule]) -> WorkflowResult<String> {
    serde_yaml::to_string(rules).map_err(|e| WorkflowError::Yaml(e.to_string()))
}
