use std::path::{Path, PathBuf};

use workflow_domain::{TriggerRule, WorkflowError, WorkflowResult};

use crate::{json, toml_format, yaml};

pub struct TriggerLoader;

impl TriggerLoader {
    pub fn load_rules_from_dir(dir_path: &str) -> WorkflowResult<Vec<TriggerRule>> {
        let dir = Path::new(dir_path);
        if !dir.exists() {
            return Err(WorkflowError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory not found: {}", dir_path),
            )));
        }

        let mut all_rules = Vec::new();
        let mut stack = vec![dir.to_path_buf()];

        while let Some(current_dir) = stack.pop() {
            for entry in std::fs::read_dir(&current_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let content = std::fs::read_to_string(&path)?;
                    match ext {
                        "yaml" | "yml" | "flow" => {
                            let mut rules = yaml::from_yaml_str(&content)?;
                            all_rules.append(&mut rules);
                        }
                        "json" => {
                            let mut rules = json::from_json_str(&content)?;
                            all_rules.append(&mut rules);
                        }
                        "toml" => {
                            let mut rules = toml_format::from_toml_str(&content)?;
                            all_rules.append(&mut rules);
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(all_rules)
    }

    pub fn load_rule(file_path: &str) -> WorkflowResult<Vec<TriggerRule>> {
        let content = std::fs::read_to_string(file_path)?;
        let path = Path::new(file_path);

        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml") | Some("yml") | Some("flow") => yaml::from_yaml_str(&content),
            Some("json") => json::from_json_str(&content),
            Some("toml") => toml_format::from_toml_str(&content),
            _ => Err(WorkflowError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unsupported file extension: {:?}", path.extension()),
            ))),
        }
    }

    pub fn collect_rule_files(dir_path: &str) -> WorkflowResult<Vec<PathBuf>> {
        let dir = Path::new(dir_path);
        if !dir.exists() {
            return Err(WorkflowError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory not found: {}", dir_path),
            )));
        }

        let mut files = Vec::new();
        let mut stack = vec![dir.to_path_buf()];

        while let Some(current_dir) = stack.pop() {
            for entry in std::fs::read_dir(&current_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "yaml" | "yml" | "json" | "toml" | "flow") {
                        files.push(path);
                    }
                }
            }
        }

        Ok(files)
    }
}
