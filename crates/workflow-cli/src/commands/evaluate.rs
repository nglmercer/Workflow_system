use workflow_actions::builtin_handlers;
use workflow_domain::{GlobalSettings, RuleEngineConfig, WorkflowResult};
use workflow_engine::RuleEngine;
use workflow_i18n::tf as i18n_tf;
use workflow_parser::evaluator::{FlowEvaluator, WorkflowOutcome};
use workflow_parser::parser::FlowParser;
use workflow_serialize::TriggerLoader;

pub async fn run(
    path: &str,
    event: &str,
    data: Option<&str>,
    vars: Option<&str>,
    plugin_dir: Option<&str>,
) -> WorkflowResult<()> {
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

    // Build a plugin manager if a plugin directory was provided
    let plugin_manager = plugin_dir.map(workflow_plugins::WorkflowPluginManager::new);

    // Check if the input is a .flow file
    let is_flow_file = std::path::Path::new(path)
        .extension()
        .map(|e| e == "flow")
        .unwrap_or(false);

    if is_flow_file {
        // Evaluate .flow file with plugin support
        run_flow(path, event, &event_data, event_vars.as_ref(), plugin_manager.as_ref()).await
    } else {
        // Evaluate YAML/JSON rules with plugin support
        run_rules(path, event, &event_data, event_vars.as_ref(), plugin_manager.as_ref()).await
    }
}

/// Evaluate a `.flow` file using the FlowEvaluator with plugin support.
async fn run_flow(
    path: &str,
    event: &str,
    data: &serde_json::Value,
    vars: Option<&serde_json::Value>,
    plugin_manager: Option<&workflow_plugins::WorkflowPluginManager>,
) -> WorkflowResult<()> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| workflow_domain::WorkflowError::Io(e))?;

    // Parse the .flow file
    let program = FlowParser::parse_flow_program(&source)
        .map_err(|e| workflow_domain::WorkflowError::Plugin(format!("Parse error: {}", e)))?;

    // Create evaluator
    let mut evaluator = FlowEvaluator::new();

    // Inject plugin functions and objects if available
    if let Some(pm) = plugin_manager {
        pm.inject_into_evaluator(&mut evaluator);
        let func_names = pm.function_registry().function_names();
        let obj_names = pm.function_registry().object_names();
        if !func_names.is_empty() || !obj_names.is_empty() {
            println!("Plugin functions: {}", func_names.join(", "));
            println!("Plugin objects: {}", obj_names.join(", "));
        }
    }

    // Load the program into the evaluator
    evaluator.load_program(&program);

    // Build context from event data
    let mut context = workflow_domain::TriggerContext::new(event, data.clone());
    if let Some(v) = vars {
        context.vars = Some(v.clone());
    }

    // Find and execute the matching workflow
    let mut found = false;
    for workflow in &program.workflows {
        if workflow.event == event {
            found = true;
            println!("Executing workflow '{}' for event '{}'", workflow.name, event);

            let outcome = evaluator
                .execute_workflow_with_result(workflow, &context)
                .map_err(|e| workflow_domain::WorkflowError::Plugin(format!("Execution error: {}", e)))?;

            print_outcome(&outcome);
            break;
        }
    }

    if !found {
        println!("No workflow found for event '{}'", event);
        println!("Available workflows:");
        for workflow in &program.workflows {
            println!("  - {} (on: {})", workflow.name, workflow.event);
        }
    }

    Ok(())
}

/// Evaluate YAML/JSON rules using the RuleEngine with plugin support.
async fn run_rules(
    path: &str,
    event: &str,
    data: &serde_json::Value,
    vars: Option<&serde_json::Value>,
    plugin_manager: Option<&workflow_plugins::WorkflowPluginManager>,
) -> WorkflowResult<()> {
    let rules = if std::path::Path::new(path).is_dir() {
        TriggerLoader::load_rules_from_dir(path)?
    } else {
        TriggerLoader::load_rule(path)?
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

    // Register plugin handlers if available
    if let Some(pm) = plugin_manager {
        pm.register_handlers(&mut engine);
    }

    println!(
        "{}",
        i18n_tf("cli.evaluate_processing", &[("event", event)])
    );
    println!(
        "{}",
        i18n_tf("cli.evaluate_data", &[("data", &data.to_string())])
    );

    let results = engine
        .process_event_simple(event, data.clone(), vars.cloned())
        .await?;

    println!(
        "
{}",
        i18n_tf(
            "cli.evaluate_matched",
            &[("count", &results.len().to_string())]
        )
    );
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
            println!(
                "{}",
                i18n_tf(
                    "cli.evaluate_action",
                    &[
                        ("status", action_status),
                        ("action_type", &action.action_type)
                    ]
                )
            );
            if let Some(result) = &action.result {
                println!(
                    "{}",
                    i18n_tf(
                        "cli.evaluate_action_result",
                        &[("result", &result.to_string())]
                    )
                );
            }
            if let Some(error) = &action.error {
                eprintln!(
                    "{}",
                    i18n_tf("cli.evaluate_action_error", &[("error", error)])
                );
            }
            if let Some(skipped) = &action.skipped {
                println!(
                    "{}",
                    i18n_tf("cli.evaluate_action_skipped", &[("skipped", skipped)])
                );
            }
        }
    }

    Ok(())
}

/// Print the outcome of a .flow evaluation.
fn print_outcome(outcome: &WorkflowOutcome) {
    if !outcome.logs.is_empty() {
        println!("\nLogs:");
        for log in &outcome.logs {
            println!("  {}", log);
        }
    }

    if !outcome.emitted.is_empty() {
        println!("\nEmitted events:");
        for event in &outcome.emitted {
            println!("  → {}", event);
        }
    }

    if !matches!(outcome.return_value, workflow_parser::evaluator::Value::Null) {
        println!("\nReturn value: {}", outcome.return_value);
    }

    if !outcome.scope.is_empty() {
        println!("\nScope:");
        for (key, value) in &outcome.scope {
            println!("  {} = {}", key, value);
        }
    }
}
