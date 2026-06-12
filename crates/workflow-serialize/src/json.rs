use workflow_domain::{TriggerRule, WorkflowError, WorkflowResult};

pub fn from_json_str(input: &str) -> WorkflowResult<Vec<TriggerRule>> {
    let rules: Vec<TriggerRule> =
        serde_json::from_str(input).map_err(WorkflowError::Serialization)?;
    Ok(rules)
}

pub fn to_json_string(rules: &[TriggerRule]) -> WorkflowResult<String> {
    serde_json::to_string_pretty(rules).map_err(WorkflowError::Serialization)
}
