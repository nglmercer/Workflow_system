use serde::{Deserialize, Serialize};

use crate::{ActionOrGroup, RuleCondition};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRule {
    #[serde(flatten)]
    pub metadata: RuleMetadata,
    pub on: String,
    #[serde(rename = "if")]
    pub condition: Option<RuleCondition>,
    pub r#do: ActionOrGroup,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuleMetadata {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl TriggerRule {
    pub fn is_enabled(&self) -> bool {
        self.metadata.enabled.unwrap_or(true)
    }

    pub fn priority_value(&self) -> i32 {
        self.metadata.priority.unwrap_or(0)
    }
}
