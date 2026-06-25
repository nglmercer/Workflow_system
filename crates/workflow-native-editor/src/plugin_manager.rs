use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use workflow_plugins::{PluginFunctionRegistry, WorkflowPluginManager};

/// Events sent from the file watcher thread to the main UI thread.
#[allow(dead_code)]
pub enum PluginEvent {
    /// A plugin file was modified or created.
    PluginChanged(String),
    /// A plugin file was removed.
    PluginRemoved(String),
    /// An error occurred in the watcher.
    Error(String),
}

/// Manages plugin loading, hot-reload, and integration with the editor.
pub struct EditorPluginManager {
    /// The underlying workflow plugin manager.
    pub manager: WorkflowPluginManager,
    /// Path to the plugin directory.
    plugin_dir: PathBuf,
    /// Channel receiver for file watcher events.
    watcher_receiver: Option<mpsc::Receiver<PluginEvent>>,
    /// Channel sender for file watcher events (kept to prevent drop).
    _watcher_sender: Option<mpsc::Sender<PluginEvent>>,
    /// Handle to the file watcher thread (kept alive for the duration of the app).
    #[allow(dead_code)]
    watcher_handle: Option<std::thread::JoinHandle<()>>,
    /// Flag to signal the watcher to stop.
    watcher_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Whether the plugin panel is open.
    pub panel_open: bool,
    /// Status message for the plugin panel.
    pub status: String,
}

impl EditorPluginManager {
    /// Create a new plugin manager with hot-reload support.
    pub fn new(plugin_dir: impl AsRef<Path>) -> Self {
        let plugin_dir = plugin_dir.as_ref().to_path_buf();
        let manager = WorkflowPluginManager::new(&plugin_dir);

        // Set up file watcher channel
        let (sender, receiver) = mpsc::channel();
        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Start file watcher thread
        let watcher_sender = sender.clone();
        let watch_path = plugin_dir.clone();
        let stop = stop_flag.clone();

        let handle = std::thread::spawn(move || {
            watch_plugin_directory(&watch_path, watcher_sender, stop);
        });

        Self {
            manager,
            plugin_dir,
            watcher_receiver: Some(receiver),
            _watcher_sender: Some(sender),
            watcher_handle: Some(handle),
            watcher_stop: stop_flag,
            panel_open: false,
            status: String::new(),
        }
    }

    /// Load all plugins from the plugin directory.
    pub fn load_all(&mut self) -> Vec<String> {
        let loaded = self.manager.load_all();
        if loaded.is_empty() {
            self.status = "No plugins loaded".to_string();
        } else {
            self.status = format!("Loaded {} plugin(s): {}", loaded.len(), loaded.join(", "));
        }
        loaded
    }

    /// Reload all plugins (unload + load).
    pub fn reload_all(&mut self) -> Vec<String> {
        // Unload all current plugins
        let names = self.manager.plugin_names();
        for name in &names {
            if let Err(e) = self.manager.unload_plugin(name) {
                log::warn!("Failed to unload plugin '{}': {}", name, e);
            }
        }
        // Reload
        self.load_all()
    }

    /// Check for file watcher events and process them.
    /// Returns true if any events were processed (request repaint).
    pub fn poll_events(&mut self) -> bool {
        let mut had_events = false;

        // Collect events first to avoid borrow conflicts
        let events: Vec<PluginEvent> = if let Some(receiver) = &self.watcher_receiver {
            let mut events = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
            events
        } else {
            Vec::new()
        };

        for event in events {
            had_events = true;
            match event {
                PluginEvent::PluginChanged(name) => {
                    log::info!("Plugin file changed: {}", name);
                    // Reload the specific plugin
                    if self.manager.is_loaded(&name) {
                        if let Err(e) = self.manager.reload_plugin(&name) {
                            self.status = format!("Failed to reload '{}': {}", name, e);
                            log::error!("Failed to reload plugin '{}': {}", name, e);
                        } else {
                            self.status = format!("Reloaded plugin: {}", name);
                            log::info!("Reloaded plugin: {}", name);
                        }
                    } else {
                        // New plugin, reload all
                        self.reload_all();
                    }
                }
                PluginEvent::PluginRemoved(name) => {
                    log::info!("Plugin file removed: {}", name);
                    if self.manager.is_loaded(&name) {
                        if let Err(e) = self.manager.unload_plugin(&name) {
                            log::warn!("Failed to unload plugin '{}': {}", name, e);
                        }
                        self.status = format!("Unloaded plugin: {}", name);
                    }
                }
                PluginEvent::Error(e) => {
                    self.status = format!("Watcher error: {}", e);
                    log::error!("Plugin watcher error: {}", e);
                }
            }
        }

        had_events
    }

    /// Inject plugin functions into a FlowEvaluator.
    #[allow(dead_code)]
    pub fn inject_into_evaluator(&self, evaluator: &mut workflow_parser::evaluator::FlowEvaluator) {
        self.manager.inject_into_evaluator(evaluator);
    }

    /// Get a reference to the function registry.
    pub fn function_registry(&self) -> &PluginFunctionRegistry {
        self.manager.function_registry()
    }

    /// Get the list of loaded plugin names.
    pub fn plugin_names(&self) -> Vec<String> {
        self.manager.plugin_names()
    }

    /// Get metadata for a plugin.
    pub fn plugin_metadata(&self, name: &str) -> Option<workflow_plugins::PluginMetadata> {
        self.manager.plugin_metadata(name)
    }

    /// Get the plugin directory path.
    pub fn plugin_dir(&self) -> &Path {
        &self.plugin_dir
    }

    /// Toggle the plugin panel visibility.
    pub fn toggle_panel(&mut self) {
        self.panel_open = !self.panel_open;
    }
}

impl Drop for EditorPluginManager {
    fn drop(&mut self) {
        // Signal the watcher to stop
        self.watcher_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // The thread will exit on its own when the flag is set
    }
}

/// Watch the plugin directory for changes and send events.
fn watch_plugin_directory(
    dir: &Path,
    sender: mpsc::Sender<PluginEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        for path in &event.paths {
                            if let Some(name) = plugin_name_from_path(path) {
                                let _ = sender.send(PluginEvent::PluginChanged(name));
                            }
                        }
                    }
                    EventKind::Remove(_) => {
                        for path in &event.paths {
                            if let Some(name) = plugin_name_from_path(path) {
                                let _ = sender.send(PluginEvent::PluginRemoved(name));
                            }
                        }
                    }
                    _ => {}
                }
            }
        },
        Config::default().with_poll_interval(Duration::from_secs(1)),
    ) {
        Ok(w) => w,
        Err(e) => {
            log::error!("Failed to create file watcher: {}", e);
            return;
        }
    };

    if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
        log::error!("Failed to watch plugin directory {}: {}", dir.display(), e);
        return;
    }

    // Keep running until stopped
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Extract the plugin name from a file path.
/// e.g., "libplugin_hello.so" -> "hello"
fn plugin_name_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;

    let name = if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
        stem.strip_prefix("lib").unwrap_or(stem)
    } else {
        stem
    };

    let name = name.strip_prefix("plugin_").unwrap_or(name);
    let name = name.strip_prefix("plugin-").unwrap_or(name);

    if name.is_empty() {
        return None;
    }

    Some(name.replace('-', "_"))
}
