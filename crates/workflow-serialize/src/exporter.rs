use std::path::Path;

use workflow_domain::{TriggerRule, WorkflowResult};

use crate::{json, toml_format, yaml};

pub struct RuleExporter;

impl RuleExporter {
    pub fn to_json(rules: &[TriggerRule]) -> WorkflowResult<String> {
        json::to_json_string(rules)
    }

    pub fn to_yaml(rules: &[TriggerRule]) -> WorkflowResult<String> {
        yaml::to_yaml_string(rules)
    }

    pub fn to_toml(rules: &[TriggerRule]) -> WorkflowResult<String> {
        toml_format::to_toml_string(rules)
    }

    pub fn to_flow(rules: &[TriggerRule]) -> WorkflowResult<String> {
        yaml::to_yaml_string(rules)
    }

    pub fn save_to_file(rules: &[TriggerRule], path: &str) -> WorkflowResult<()> {
        let content = match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some("yaml") | Some("yml") | Some("flow") => Self::to_yaml(rules)?,
            Some("json") => Self::to_json(rules)?,
            Some("toml") => Self::to_toml(rules)?,
            _ => {
                return Err(workflow_domain::WorkflowError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    workflow_i18n::tf("serialize.export_unsupported", &[("path", &path.to_string())]),
                )))
            }
        };

        std::fs::write(path, content)?;
        Ok(())
    }
}
