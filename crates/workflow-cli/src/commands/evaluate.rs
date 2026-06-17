use workflow_i18n::tf as i18n_tf;
use workflow_actions::builtin_handlers;
use workflow_domain::{GlobalSettings, RuleEngineConfig, WorkflowResult};
use workflow_engine::RuleEngine;
use workflow_serialize::TriggerLoader;

pub async fn run(
    path: &str,
    event: &str,
    data: Option<&str>,
    vars: Option<&str>,
) -> WorkflowResult<()> {
    let rules = if std::path::Path::new(path).is_dir() {
        TriggerLoader::load_rules_from_dir(path)?
    } else {
        TriggerLoader::load_rule(path)?
    };

    let event_data: serde_json::Value = match data {
        Some(d) => {
            serde_json::from_str(d).map_err(workflow_domain::WorkflowError::Serialization)?
        }
        None => serde_json::json!({}),
    };

    let event_vars: Option<serde_json::Value> = match vars {
        Some(v) => {
            Some(serde_json::from_str(v).map_err(workflow_domain::WorkflowError::Serialization)?)
        }
        None => None,
    };

    let config = RuleEngineConfig {
        rules,
        global_settings: GlobalSettings {
            debug_mode: true,
            evaluate_all: true,
            strict_actions: false,
        },
    };

    let mut engine = RuleEngine::new(config);

    for handler in builtin_handlers() {
        engine.register_handler(handler);
    }

    println!("{}", i18n_tf("cli.evaluate_processing", &[("event", event)]));
    println!("{}", i18n_tf("cli.evaluate_data", &[("data", &event_data.to_string())]));

    let results = engine
        .process_event_simple(event, event_data, event_vars)
        .await?;

    println!("
{}", i18n_tf("cli.evaluate_matched", &[("count", &results.len().to_string())]));
    for result in &results {
        let status = if result.success { "✓" } else { "✗" };
        println!(
            "\n{} Rule: {} (success: {})",
            status, result.rule_id, result.success
        );

        for action in &result.executed_actions {
            let action_status = if action.error.is_some() {
                "✗"
            } else if action.skipped.is_some() {
                "○"
            } else {
                "✓"
            };
            println!("{}", i18n_tf("cli.evaluate_action", &[("status", action_status), ("action_type", &action.action_type)]));
            if let Some(result) = &action.result {
                println!("{}", i18n_tf("cli.evaluate_action_result", &[("result", &result.to_string())]));
            }
            if let Some(error) = &action.error {
                eprintln!("{}", i18n_tf("cli.evaluate_action_error", &[("error", error)]));
            }
            if let Some(skipped) = &action.skipped {
                println!("{}", i18n_tf("cli.evaluate_action_skipped", &[("skipped", skipped)]));
            }
        }
    }

    Ok(())
}
