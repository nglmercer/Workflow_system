use serde::{Deserialize, Serialize};

use crate::{TriggerRule, WorkflowError, WorkflowResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub severity: IssueSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IssueSeverity {
    Error,
    Warning,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            valid: true,
            issues: vec![],
        }
    }

    pub fn error(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            valid: false,
            issues: vec![ValidationIssue {
                field: field.into(),
                message: message.into(),
                severity: IssueSeverity::Error,
            }],
        }
    }

    pub fn add_issue(
        &mut self,
        field: impl Into<String>,
        message: impl Into<String>,
        severity: IssueSeverity,
    ) {
        self.issues.push(ValidationIssue {
            field: field.into(),
            message: message.into(),
            severity,
        });
        if severity == IssueSeverity::Error {
            self.valid = false;
        }
    }
}

pub struct TriggerValidator;

impl TriggerValidator {
    pub fn validate(rule: &TriggerRule) -> ValidationResult {
        let mut result = ValidationResult::ok();

        if rule.metadata.id.is_empty() {
            result.add_issue("id", "Rule ID cannot be empty", IssueSeverity::Error);
        }

        if rule.on.is_empty() {
            result.add_issue("on", "Event name cannot be empty", IssueSeverity::Error);
        }

        if let Some(cooldown) = rule.metadata.cooldown {
            if cooldown == 0 {
                result.add_issue(
                    "cooldown",
                    "Cooldown must be greater than 0",
                    IssueSeverity::Warning,
                );
            }
        }

        if let Some(priority) = rule.metadata.priority {
            if priority < 0 {
                result.add_issue(
                    "priority",
                    "Priority should be non-negative",
                    IssueSeverity::Warning,
                );
            }
        }

        result
    }

    pub fn validate_all(rules: &[TriggerRule]) -> ValidationResult {
        let mut result = ValidationResult::ok();
        let mut seen_ids = std::collections::HashSet::new();

        for rule in rules {
            let rule_result = Self::validate(rule);
            for issue in rule_result.issues {
                result.add_issue(
                    format!("rule[{}].{}", rule.metadata.id, issue.field),
                    issue.message,
                    issue.severity,
                );
            }

            if !seen_ids.insert(&rule.metadata.id) {
                result.add_issue(
                    format!("rule[{}].id", rule.metadata.id),
                    format!("Duplicate rule ID: {}", rule.metadata.id),
                    IssueSeverity::Error,
                );
            }
        }

        result
    }
}

impl From<ValidationResult> for WorkflowResult<()> {
    fn from(result: ValidationResult) -> Self {
        if result.valid {
            Ok(())
        } else {
            let errors: Vec<String> = result
                .issues
                .iter()
                .filter(|i| i.severity == IssueSeverity::Error)
                .map(|i| format!("{}: {}", i.field, i.message))
                .collect();
            Err(WorkflowError::Validation(errors.join("; ")))
        }
    }
}
