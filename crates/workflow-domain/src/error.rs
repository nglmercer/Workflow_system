use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkflowError {
    #[error("Rule validation error: {0}")]
    Validation(String),

    #[error("Condition evaluation error: {0}")]
    ConditionEvaluation(String),

    #[error("Action execution error: {0}")]
    ActionExecution(String),

    #[error("Unknown action type: {0}")]
    UnknownAction(String),

    #[error("Field not found: {0}")]
    FieldNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(String),

    #[error("TOML error: {0}")]
    Toml(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("Cooldown active for rule: {0}")]
    CooldownActive(String),

    #[error("Rule disabled: {0}")]
    RuleDisabled(String),

    #[error("Invalid state transition: {0}")]
    InvalidTransition(String),

    #[error("Plugin error: {0}")]
    Plugin(String),
}

impl WorkflowError {
    /// Localized display string. Uses the i18n catalog when a key
    /// is registered; falls back to the English `Display` impl for
    /// variants whose inner field needs no translation.
    pub fn display_localized(&self) -> String {
        use workflow_i18n::tf as i18n_tf;
        match self {
            WorkflowError::Validation(inner) => {
                i18n_tf("domain.error_validation", &[("error", inner)])
            }
            WorkflowError::ConditionEvaluation(inner) => {
                i18n_tf("domain.error_condition_evaluation", &[("error", inner)])
            }
            WorkflowError::ActionExecution(inner) => {
                i18n_tf("domain.error_action_execution", &[("error", inner)])
            }
            WorkflowError::UnknownAction(inner) => {
                i18n_tf("domain.error_unknown_action", &[("type", inner)])
            }
            WorkflowError::FieldNotFound(inner) => {
                i18n_tf("domain.error_field_not_found", &[("field", inner)])
            }
            WorkflowError::Serialization(err) => {
                i18n_tf("domain.error_serialization", &[("error", &err.to_string())])
            }
            WorkflowError::Yaml(inner) => i18n_tf("domain.error_yaml", &[("error", inner)]),
            WorkflowError::Toml(inner) => i18n_tf("domain.error_toml", &[("error", inner)]),
            WorkflowError::Io(err) => i18n_tf("domain.error_io", &[("error", &err.to_string())]),
            WorkflowError::Regex(err) => {
                i18n_tf("domain.error_regex", &[("error", &err.to_string())])
            }
            WorkflowError::CooldownActive(inner) => {
                i18n_tf("domain.error_cooldown_active", &[("rule", inner)])
            }
            WorkflowError::RuleDisabled(inner) => {
                i18n_tf("domain.error_rule_disabled", &[("rule", inner)])
            }
            WorkflowError::InvalidTransition(inner) => {
                i18n_tf("domain.error_invalid_transition", &[("state", inner)])
            }
            WorkflowError::Plugin(inner) => i18n_tf("domain.error_plugin", &[("error", inner)]),
        }
    }
}

pub type WorkflowResult<T> = Result<T, WorkflowError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_localized_falls_back_to_english_when_no_translation() {
        workflow_i18n::init_with("en");
        let err = WorkflowError::Validation("field is empty".to_string());
        let s = err.display_localized();
        assert!(s.contains("Rule validation error"));
        assert!(s.contains("field is empty"));
    }

    #[test]
    fn display_localized_includes_inner_value() {
        workflow_i18n::init_with("en");
        let err = WorkflowError::UnknownAction("frobnicate".to_string());
        let s = err.display_localized();
        assert!(s.contains("frobnicate"));
    }
}
