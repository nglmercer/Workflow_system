use workflow_domain::WorkflowResult;

pub fn run(plugin_dir: Option<&str>) -> WorkflowResult<()> {
    let dir = plugin_dir.unwrap_or("plugins");

    if !std::path::Path::new(dir).exists() {
        println!("Plugin directory '{}' does not exist.", dir);
        println!("\nTo use plugins:");
        println!("  1. Create a plugin directory: mkdir {}", dir);
        println!("  2. Build plugin shared libraries into it");
        println!("  3. Run: workflow --plugins {} evaluate ...", dir);
        return Ok(());
    }

    let mut plugin_manager = workflow_plugins::WorkflowPluginManager::new(dir);
    let loaded = plugin_manager.load_all();

    if loaded.is_empty() {
        println!("No plugins found in '{}'.", dir);
        println!("\nExpected shared library files:");
        if cfg!(target_os = "linux") {
            println!("  - *.so files");
        } else if cfg!(target_os = "macos") {
            println!("  - *.dylib files");
        } else if cfg!(target_os = "windows") {
            println!("  - *.dll files");
        }
        return Ok(());
    }

    println!("Loaded {} plugin(s):\n", loaded.len());
    for name in &loaded {
        if let Some(meta) = plugin_manager.plugin_metadata(name) {
            println!("  {} v{}", meta.name, meta.version);
            if !meta.authors.is_empty() {
                println!("    Authors: {}", meta.authors.join(", "));
            }
            if !meta.dependencies.is_empty() {
                let deps: Vec<String> = meta
                    .dependencies
                    .iter()
                    .map(|d| format!("{} ({})", d.name, d.version_req))
                    .collect();
                println!("    Dependencies: {}", deps.join(", "));
            }
        } else {
            println!("  {}", name);
        }
    }

    Ok(())
}
