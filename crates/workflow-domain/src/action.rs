use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub type ActionParams = HashMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum ActionOrGroup {
    Single(Action),
    Multiple(Vec<Action>),
    Group(ActionGroup),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Action {
    #[serde(rename = "type")]
    pub action_type: String,
    pub params: Option<ActionParams>,
    pub delay: Option<u64>,
    pub probability: Option<f64>,
    pub retry: Option<RetryPolicy>,
    pub foreach: Option<ForeachConfig>,
    pub r#while: Option<WhileConfig>,
    pub repeat: Option<RepeatConfig>,
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

/// Retry policy for failed actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay in milliseconds before first retry
    pub initial_delay_ms: u64,
    /// Backoff multiplier (e.g., 2.0 for exponential)
    pub backoff_multiplier: f64,
    /// Maximum delay in milliseconds between retries
    pub max_delay_ms: u64,
    /// Types of errors to retry on (empty = retry all)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_on: Option<Vec<String>>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 1000,
            backoff_multiplier: 2.0,
            max_delay_ms: 30000,
            retry_on: None,
        }
    }
}

/// Configuration for foreach loop over an array
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeachConfig {
    /// Field path to iterate over (e.g., "data.items")
    pub field: String,
    /// Variable name for current item (e.g., "item")
    pub item_var: String,
    /// Optional variable name for current index (e.g., "index")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_var: Option<String>,
    /// Actions to execute for each item
    pub actions: Vec<Action>,
    /// Maximum parallel iterations (default: 1 = sequential)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel: Option<u32>,
}

/// Configuration for while loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhileConfig {
    /// Condition to evaluate before each iteration
    pub condition: crate::condition::RuleCondition,
    /// Maximum number of iterations (safety limit)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    /// Delay in milliseconds between iterations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
    /// Actions to execute in each iteration
    pub actions: Vec<Action>,
}

/// Configuration for repeat loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatConfig {
    /// Number of times to repeat (can be expression like "${data.count}")
    pub count: RepeatCount,
    /// Variable name for current iteration (e.g., "i")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_var: Option<String>,
    /// Actions to execute in each iteration
    pub actions: Vec<Action>,
    /// Delay in milliseconds between iterations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
}

/// Repeat count - either a fixed number or expression
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepeatCount {
    Fixed(u32),
    Expression(String),
}
