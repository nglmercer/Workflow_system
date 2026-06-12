use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub type ActionParams = HashMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActionOrGroup {
    Single(Action),
    Multiple(Vec<Action>),
    Group(ActionGroup),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<ActionParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionGroup {
    pub mode: ActionGroupMode,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActionGroupMode {
    All,
    Either,
    Sequence,
}
