use workflow_domain::WorkflowResult;
use workflow_serialize::{RuleExporter, TriggerLoader};

pub fn run(input: &str, output: &str) -> WorkflowResult<()> {
    let rules = if std::path::Path::new(input).is_dir() {
        TriggerLoader::load_rules_from_dir(input)?
    } else {
        TriggerLoader::load_rule(input)?
    };

    println!("Loaded {} rule(s) from {}", rules.len(), input);

    RuleExporter::save_to_file(&rules, output)?;

    println!("Exported to {}", output);
    Ok(())
}
