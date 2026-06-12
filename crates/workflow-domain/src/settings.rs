use serde::{Deserialize, Serialize};

use crate::TriggerRule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEngineConfig {
    pub rules: Vec<TriggerRule>,
    #[serde(default)]
    pub global_settings: GlobalSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSettings {
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub evaluate_all: bool,
    #[serde(default)]
    pub strict_actions: bool,
}
