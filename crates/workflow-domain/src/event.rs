use serde::{Deserialize, Serialize};

use crate::{TriggerContext, TriggerRule};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineEvent {
    EngineStart,
    EngineDone,
    RuleMatch,
    RuleSkip,
    ActionSuccess,
    ActionError,
    ActionSkip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineEventPayload {
    pub event: EngineEvent,
    pub rule: Option<TriggerRule>,
    pub context: Option<TriggerContext>,
    pub action_type: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}
