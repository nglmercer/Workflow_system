use workflow_i18n::tf as i18n_tf;
use workflow_domain::WorkflowResult;
use workflow_serialize::{RuleExporter, TriggerLoader};

pub fn run(input: &str, output: &str) -> WorkflowResult<()> {
    let rules = if std::path::Path::new(input).is_dir() {
        TriggerLoader::load_rules_from_dir(input)?
    } else {
        TriggerLoader::load_rule(input)?
    };

    println!("{}", i18n_tf("cli.export_loaded", &[("count", &rules.len().to_string()), ("path", input)]));

    RuleExporter::save_to_file(&rules, output)?;

    println!("{}", i18n_tf("cli.export_wrote", &[("path", output)]));
    Ok(())
}
