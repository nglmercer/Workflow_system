use std::collections::HashSet;
use std::time::Duration;
use workflow_i18n::tf as i18n_tf;

use workflow_actions::builtin_handlers;
use workflow_domain::{GlobalSettings, RuleEngineConfig, WorkflowResult};
use workflow_engine::RuleEngine;
use workflow_serialize::TriggerLoader;

pub async fn run(
    path: &str,
    event: &str,
    data: Option<&str>,
    plugin_dir: Option<&str>,
) -> WorkflowResult<()> {
    let event_data: serde_json::Value = match data {
        Some(d) => {
            serde_json::from_str(d).map_err(workflow_domain::WorkflowError::Serialization)?
        }
        None => serde_json::json!({}),
    };

    // Pre-load plugins once if a directory was provided
    let mut plugin_manager = plugin_dir.map(workflow_plugins::WorkflowPluginManager::new);
    if let Some(ref mut pm) = plugin_manager {
        let loaded = pm.load_all();
        if !loaded.is_empty() {
            println!("Loaded {} plugin(s): {}", loaded.len(), loaded.join(", "));
        }
    }

    println!("{}", i18n_tf("cli.watching", &[("path", path)]));
    println!("Press Ctrl+C to stop.\n");

    let mut last_files: HashSet<std::path::PathBuf> = TriggerLoader::collect_rule_files(path)?
        .into_iter()
        .collect();

    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        let current_files: HashSet<std::path::PathBuf> = TriggerLoader::collect_rule_files(path)?
            .into_iter()
            .collect();

        let modified: Vec<_> = current_files
            .difference(&last_files)
            .chain(current_files.intersection(&last_files).filter(|f| {
                if let Ok(meta) = std::fs::metadata(f) {
                    if let Ok(modified) = meta.modified() {
                        if let Ok(elapsed) = modified.elapsed() {
                            return elapsed < Duration::from_secs(2);
                        }
                    }
                }
                false
            }))
            .cloned()
            .collect();

        if !modified.is_empty() {
            println!(
                "{}",
                i18n_tf(
                    "cli.watching_changes",
                    &[("count", &modified.len().to_string())]
                )
            );
            for file in &modified {
                println!(
                    "{}",
                    i18n_tf(
                        "cli.watching_file_changed",
                        &[("path", &file.display().to_string())]
                    )
                );
            }

            let rules = TriggerLoader::load_rules_from_dir(path)?;

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

            // Register plugin handlers if available
            if let Some(ref pm) = plugin_manager {
                pm.register_handlers(&mut engine);
            }

            match engine
                .process_event_simple(event, event_data.clone(), None)
                .await
            {
                Ok(results) => {
                    println!(
                        "{}",
                        i18n_tf(
                            "cli.watching_processed",
                            &[("count", &results.len().to_string())]
                        )
                    );
                    for result in &results {
                        println!(
                            "  ✓ {}: {} action(s)",
                            result.rule_id,
                            result.executed_actions.len()
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        i18n_tf("cli.error_prefix", &[("error", &e.to_string())])
                    );
                }
            }
            println!();
        }

        last_files = current_files;
    }
}
