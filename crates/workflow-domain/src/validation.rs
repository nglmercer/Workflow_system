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
            result.add_issue(
                "id",
                workflow_i18n::t("domain.validation_id_empty"),
                IssueSeverity::Error,
            );
        }

        if rule.on.is_empty() {
            result.add_issue(
                "on",
                workflow_i18n::t("domain.validation_event_empty"),
                IssueSeverity::Error,
            );
        }

        if let Some(cooldown) = rule.metadata.cooldown {
            if cooldown == 0 {
                result.add_issue(
                    "cooldown",
                    workflow_i18n::t("domain.validation_cooldown_zero"),
                    IssueSeverity::Warning,
                );
            }
        }

        if let Some(priority) = rule.metadata.priority {
            if priority < 0 {
                result.add_issue(
                    "priority",
                    workflow_i18n::t("domain.validation_priority_negative"),
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
                    workflow_i18n::tf(
                        "domain.validation_duplicate_id",
                        &[("id", &rule.metadata.id)],
                    ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn make_rule(id: &str, event: &str) -> TriggerRule {
        TriggerRule {
            metadata: RuleMetadata {
                id: id.to_string(),
                name: Some(format!("Rule {}", id)),
                ..Default::default()
            },
            on: event.to_string(),
            condition: None,
            r#do: ActionOrGroup::Single(Action {
                action_type: "noop".to_string(),
                params: None,
                delay: None,
                probability: None,
                retry: None,
                foreach: None,
                r#while: None,
                repeat: None,
            }),
        }
    }

    #[test]
    fn test_validate_valid_rule() {
        workflow_i18n::init_with("en");
        let rule = make_rule("test-1", "TEST_EVENT");
        let result = TriggerValidator::validate(&rule);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_validate_empty_id() {
        workflow_i18n::init_with("en");
        let rule = make_rule("", "TEST_EVENT");
        let result = TriggerValidator::validate(&rule);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.field == "id"));
    }

    #[test]
    fn test_validate_empty_event() {
        workflow_i18n::init_with("en");
        let rule = make_rule("test-1", "");
        let result = TriggerValidator::validate(&rule);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.field == "on"));
    }

    #[test]
    fn test_validate_zero_cooldown() {
        workflow_i18n::init_with("en");
        let rule = TriggerRule {
            metadata: RuleMetadata {
                id: "test-1".to_string(),
                cooldown: Some(0),
                ..Default::default()
            },
            on: "TEST_EVENT".to_string(),
            condition: None,
            r#do: ActionOrGroup::Single(Action {
                action_type: "noop".to_string(),
                params: None,
                delay: None,
                probability: None,
                retry: None,
                foreach: None,
                r#while: None,
                repeat: None,
            }),
        };
        let result = TriggerValidator::validate(&rule);
        assert!(result.valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Warning));
    }

    #[test]
    fn test_validate_all_unique_ids() {
        workflow_i18n::init_with("en");
        let rules = vec![
            make_rule("rule-1", "EVENT_A"),
            make_rule("rule-2", "EVENT_B"),
        ];
        let result = TriggerValidator::validate_all(&rules);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_all_duplicate_ids() {
        workflow_i18n::init_with("en");
        let rules = vec![
            make_rule("rule-1", "EVENT_A"),
            make_rule("rule-1", "EVENT_B"),
        ];
        let result = TriggerValidator::validate_all(&rules);
        assert!(!result.valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.message.contains("Duplicate")));
    }

    #[test]
    fn test_validation_result_conversion_ok() {
        let result = ValidationResult::ok();
        let workflow_result: WorkflowResult<()> = result.into();
        assert!(workflow_result.is_ok());
    }

    #[test]
    fn test_validation_result_conversion_err() {
        let result = ValidationResult::error("test", "error message");
        let workflow_result: WorkflowResult<()> = result.into();
        assert!(workflow_result.is_err());
    }

    #[test]
    fn test_trigger_rule_default_metadata() {
        let meta = RuleMetadata::default();
        assert!(meta.id.is_empty());
        assert!(meta.name.is_none());
        assert_eq!(meta.priority, None);
        assert!(meta.enabled.is_none());
    }

    #[test]
    fn test_trigger_rule_enabled_default() {
        let rule = make_rule("test", "EVENT");
        assert!(rule.is_enabled());
    }

    #[test]
    fn test_trigger_rule_disabled() {
        let mut rule = make_rule("test", "EVENT");
        rule.metadata.enabled = Some(false);
        assert!(!rule.is_enabled());
    }

    #[test]
    fn test_trigger_rule_priority_value() {
        let mut rule = make_rule("test", "EVENT");
        assert_eq!(rule.priority_value(), 0);

        rule.metadata.priority = Some(10);
        assert_eq!(rule.priority_value(), 10);

        rule.metadata.priority = Some(-5);
        assert_eq!(rule.priority_value(), -5);
    }

    #[test]
    fn test_action_or_group_variants() {
        let action = Action {
            action_type: "test".to_string(),
            params: None,
            delay: None,
            probability: None,
            retry: None,
            foreach: None,
            r#while: None,
            repeat: None,
        };

        let single = ActionOrGroup::Single(action.clone());
        assert!(matches!(single, ActionOrGroup::Single(_)));

        let multiple = ActionOrGroup::Multiple(vec![action.clone()]);
        assert!(matches!(multiple, ActionOrGroup::Multiple(_)));

        let group = ActionOrGroup::Group(ActionGroup {
            mode: ActionGroupMode::All,
            actions: vec![action],
        });
        assert!(matches!(group, ActionOrGroup::Group(_)));
    }

    #[test]
    fn test_condition_single() {
        let cond = RuleCondition::Single(Condition {
            field: "data.value".to_string(),
            operator: ComparisonOperator::Gt,
            value: serde_json::json!(100),
        });
        assert!(matches!(cond, RuleCondition::Single(_)));
    }

    #[test]
    fn test_condition_group() {
        let cond = RuleCondition::Group(ConditionGroup {
            operator: LogicOperator::And,
            conditions: vec![
                RuleCondition::Single(Condition {
                    field: "data.a".to_string(),
                    operator: ComparisonOperator::Gt,
                    value: serde_json::json!(1),
                }),
                RuleCondition::Single(Condition {
                    field: "data.b".to_string(),
                    operator: ComparisonOperator::Lt,
                    value: serde_json::json!(10),
                }),
            ],
        });
        assert!(matches!(cond, RuleCondition::Group(_)));
    }

    #[test]
    fn test_foreach_config() {
        let config = ForeachConfig {
            field: "data.items".to_string(),
            item_var: "item".to_string(),
            index_var: Some("i".to_string()),
            actions: vec![],
            parallel: Some(4),
        };
        assert_eq!(config.field, "data.items");
        assert_eq!(config.item_var, "item");
        assert_eq!(config.index_var, Some("i".to_string()));
        assert_eq!(config.parallel, Some(4));
    }

    #[test]
    fn test_while_config() {
        let config = WhileConfig {
            condition: RuleCondition::Single(Condition {
                field: "data.count".to_string(),
                operator: ComparisonOperator::Lt,
                value: serde_json::json!(10),
            }),
            max_iterations: Some(100),
            delay_ms: Some(50),
            actions: vec![],
        };
        assert_eq!(config.max_iterations, Some(100));
        assert_eq!(config.delay_ms, Some(50));
    }

    #[test]
    fn test_repeat_config() {
        let config = RepeatConfig {
            count: RepeatCount::Fixed(5),
            index_var: Some("i".to_string()),
            actions: vec![],
            delay_ms: None,
        };
        match config.count {
            RepeatCount::Fixed(n) => assert_eq!(n, 5),
            _ => panic!("Expected Fixed"),
        }
    }

    #[test]
    fn test_repeat_config_expression() {
        let config = RepeatConfig {
            count: RepeatCount::Expression("${data.count}".to_string()),
            index_var: None,
            actions: vec![],
            delay_ms: None,
        };
        match config.count {
            RepeatCount::Expression(e) => assert_eq!(e, "${data.count}"),
            _ => panic!("Expected Expression"),
        }
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.initial_delay_ms, 1000);
        assert_eq!(policy.backoff_multiplier, 2.0);
        assert_eq!(policy.max_delay_ms, 30000);
        assert!(policy.retry_on.is_none());
    }

    #[test]
    fn test_trigger_context_new() {
        let ctx = TriggerContext::new("TEST_EVENT", serde_json::json!({"key": "value"}));
        assert_eq!(ctx.event, "TEST_EVENT");
        assert_eq!(ctx.data["key"], "value");
    }

    #[test]
    fn test_trigger_result_success() {
        let result = TriggerResult::success("rule-1", vec![]);
        assert!(result.success);
        assert_eq!(result.rule_id, "rule-1");
    }

    #[test]
    fn test_global_settings_default() {
        let settings = GlobalSettings::default();
        assert!(!settings.debug_mode);
        assert!(!settings.evaluate_all);
        assert!(!settings.strict_actions);
    }
}
