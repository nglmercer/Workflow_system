use workflow_actions::builtin_handlers;
use workflow_domain::*;
use workflow_engine::RuleEngine;
use workflow_serialize::{RuleExporter, TriggerLoader};

fn rules_dir() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules");
    path.to_string_lossy().to_string()
}

#[test]
fn test_load_yaml_rules() {
    let rules = TriggerLoader::load_rules_from_dir(&rules_dir()).unwrap();
    assert!(rules.len() > 0);
    println!("Loaded {} rules", rules.len());
}

#[test]
fn test_validate_rules() {
    let rules = TriggerLoader::load_rules_from_dir(&rules_dir()).unwrap();
    let result = TriggerValidator::validate_all(&rules);
    assert!(result.valid, "Validation failed: {:?}", result.issues);
}

#[test]
fn test_export_json() {
    let rules = TriggerLoader::load_rules_from_dir(&rules_dir()).unwrap();
    let json = RuleExporter::to_json(&rules).unwrap();
    let parsed: Vec<TriggerRule> = serde_json::from_str(&json).unwrap();
    assert_eq!(rules.len(), parsed.len());
}

#[test]
fn test_export_yaml() {
    let rules = TriggerLoader::load_rules_from_dir(&rules_dir()).unwrap();
    let yaml = RuleExporter::to_yaml(&rules).unwrap();
    let parsed: Vec<TriggerRule> = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(rules.len(), parsed.len());
}

#[tokio::test]
async fn test_engine_process_event() {
    let rules = TriggerLoader::load_rules_from_dir(&rules_dir()).unwrap();

    let config = RuleEngineConfig {
        rules,
        global_settings: GlobalSettings {
            debug_mode: false,
            evaluate_all: true,
            strict_actions: false,
        },
    };

    let mut engine = RuleEngine::new(config);
    for handler in builtin_handlers() {
        engine.register_handler(handler);
    }

    let results = engine
        .process_event_simple(
            "USER_REGISTERED",
            serde_json::json!({
                "userId": "test123",
                "plan": "premium",
                "email": "test@example.com"
            }),
            None,
        )
        .await
        .unwrap();

    assert!(!results.is_empty());
    for result in &results {
        assert!(result.success);
    }
}

#[tokio::test]
async fn test_engine_condition_group() {
    let rules = vec![TriggerRule {
        metadata: RuleMetadata {
            id: "test-and-group".to_string(),
            ..Default::default()
        },
        on: "TEST_EVENT".to_string(),
        condition: Some(RuleCondition::Group(ConditionGroup {
            operator: LogicOperator::And,
            conditions: vec![
                RuleCondition::Single(Condition {
                    field: "data.value".to_string(),
                    operator: ComparisonOperator::Gt,
                    value: serde_json::json!(100),
                }),
                RuleCondition::Single(Condition {
                    field: "data.type".to_string(),
                    operator: ComparisonOperator::Eq,
                    value: serde_json::json!("important"),
                }),
            ],
        })),
        r#do: ActionOrGroup::Single(Action {
            action_type: "log_message".to_string(),
            params: Some({
                let mut m = std::collections::HashMap::new();
                m.insert("message".to_string(), serde_json::json!("Matched!"));
                m
            }),
            delay: None,
            probability: None,
        }),
    }];

    let config = RuleEngineConfig {
        rules,
        global_settings: GlobalSettings {
            debug_mode: false,
            evaluate_all: false,
            strict_actions: false,
        },
    };

    let mut engine = RuleEngine::new(config);
    for handler in builtin_handlers() {
        engine.register_handler(handler);
    }

    // Should match
    let results = engine
        .process_event_simple(
            "TEST_EVENT",
            serde_json::json!({"value": 150, "type": "important"}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success);

    // Should not match (value too low)
    let results = engine
        .process_event_simple(
            "TEST_EVENT",
            serde_json::json!({"value": 50, "type": "important"}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 0);
}

#[test]
fn test_rule_builder_pattern() {
    let rule = TriggerRule {
        metadata: RuleMetadata {
            id: "builder-test".to_string(),
            name: Some("Builder Test".to_string()),
            description: Some("Test rule".to_string()),
            priority: Some(10),
            enabled: Some(true),
            cooldown: Some(1000),
            tags: Some(vec!["test".to_string()]),
        },
        on: "TEST_EVENT".to_string(),
        condition: Some(RuleCondition::Single(Condition {
            field: "data.value".to_string(),
            operator: ComparisonOperator::Gt,
            value: serde_json::json!(50),
        })),
        r#do: ActionOrGroup::Multiple(vec![
            Action {
                action_type: "log_message".to_string(),
                params: None,
                delay: None,
                probability: None,
            },
            Action {
                action_type: "noop".to_string(),
                params: None,
                delay: None,
                probability: None,
            },
        ]),
    };

    let result = TriggerValidator::validate(&rule);
    assert!(result.valid);

    let json = serde_json::to_string(&rule).unwrap();
    let parsed: TriggerRule = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.metadata.id, "builder-test");
}
