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
}

pub type WorkflowResult<T> = Result<T, WorkflowError>;
