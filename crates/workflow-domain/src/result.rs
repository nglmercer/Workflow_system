use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerResult {
    pub rule_id: String,
    pub success: bool,
    pub executed_actions: Vec<ExecutedAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedAction {
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

impl TriggerResult {
    pub fn success(rule_id: impl Into<String>, actions: Vec<ExecutedAction>) -> Self {
        Self {
            rule_id: rule_id.into(),
            success: true,
            executed_actions: actions,
            error: None,
        }
    }

    pub fn error(rule_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            rule_id: rule_id.into(),
            success: false,
            executed_actions: vec![],
            error: Some(error.into()),
        }
    }
}
