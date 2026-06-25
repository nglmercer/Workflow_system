use eframe::egui::{self, RichText};
use workflow_i18n::{t as i18n_t, tf as i18n_tf};

use super::plugin_manager::EditorPluginManager;

/// Show the plugin panel. Returns an action if the user clicked a button.
pub enum PluginAction {
    /// Reload all plugins.
    ReloadAll,
    /// Toggle the panel visibility.
    TogglePanel,
}

/// Render the plugin panel as a side panel on the right.
pub fn show(ctx: &egui::Context, plugin_manager: &EditorPluginManager) -> Option<PluginAction> {
    if !plugin_manager.panel_open {
        return None;
    }

    let mut action = None;

    egui::SidePanel::right("plugin_panel")
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(i18n_t("plugins.title"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if ui.small_button("×").clicked() {
                        action = Some(PluginAction::TogglePanel);
                    }
                });
            });

            ui.separator();

            // Plugin directory info
            ui.label(
                RichText::new(i18n_tf(
                    "plugins.directory",
                    &[("path", &plugin_manager.plugin_dir().display().to_string())],
                ))
                .small(),
            );

            ui.add_space(4.0);

            // Reload button
            if ui
                .button(RichText::new(i18n_t("plugins.reload_all")).strong())
                .clicked()
            {
                action = Some(PluginAction::ReloadAll);
            }

            ui.add_space(8.0);

            // Status message
            if !plugin_manager.status.is_empty() {
                ui.label(RichText::new(&plugin_manager.status).small().italics());
                ui.add_space(4.0);
            }

            ui.separator();

            // Loaded plugins list
            let names = plugin_manager.plugin_names();
            if names.is_empty() {
                ui.label(i18n_t("plugins.no_plugins"));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(i18n_t("plugins.place_hint"))
                        .small()
                        .italics(),
                );
            } else {
                ui.label(
                    RichText::new(i18n_tf(
                        "plugins.loaded_count",
                        &[("count", &names.len().to_string())],
                    ))
                    .strong(),
                );

                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for name in &names {
                            render_plugin_card(ui, plugin_manager, name);
                        }
                    });
            }

            ui.separator();

            // Available functions
            let func_registry = plugin_manager.function_registry();
            let func_names = func_registry.function_names();
            let obj_names = func_registry.object_names();

            if !func_names.is_empty() || !obj_names.is_empty() {
                ui.label(RichText::new(i18n_t("plugins.available_api")).strong());
                ui.add_space(4.0);

                if !func_names.is_empty() {
                    ui.label(RichText::new(i18n_t("plugins.functions")).small().strong());
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            for name in &func_names {
                                if let Some(sig) = func_registry.get_function_signature(name) {
                                    let params = sig.params.join(", ");
                                    ui.label(
                                        RichText::new(format!("{}({})", name, params))
                                            .small()
                                            .monospace(),
                                    );
                                    ui.label(
                                        RichText::new(&sig.description)
                                            .small()
                                            .italics(),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new(name)
                                            .small()
                                            .monospace(),
                                    );
                                }
                                ui.add_space(2.0);
                            }
                        });
                }

                if !obj_names.is_empty() {
                    ui.label(RichText::new(i18n_t("plugins.objects")).small().strong());
                    egui::ScrollArea::vertical()
                        .max_height(100.0)
                        .show(ui, |ui| {
                            for name in &obj_names {
                                ui.label(
                                    RichText::new(format!("${{{}}}", name))
                                        .small()
                                        .monospace(),
                                );
                                let sigs = func_registry.object_signatures();
                                if let Some(sig) = sigs.iter().find(|s| s.plugin_name == *name) {
                                    ui.label(
                                        RichText::new(&sig.description)
                                            .small()
                                            .italics(),
                                    );
                                }
                                ui.add_space(2.0);
                            }
                        });
                }
            }
        });

    action
}

/// Render a single plugin card.
fn render_plugin_card(
    ui: &mut egui::Ui,
    plugin_manager: &EditorPluginManager,
    name: &str,
) {
    egui::Frame::none()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .rounding(4.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong());

                if let Some(meta) = plugin_manager.plugin_metadata(name) {
                    ui.label(
                        RichText::new(format!("v{}", meta.version))
                            .small()
                            .italics(),
                    );
                }
            });

            if let Some(meta) = plugin_manager.plugin_metadata(name) {
                if !meta.authors.is_empty() {
                    ui.label(
                        RichText::new(format!("by {}", meta.authors.join(", ")))
                            .small(),
                    );
                }
                if !meta.dependencies.is_empty() {
                    let deps: Vec<String> = meta
                        .dependencies
                        .iter()
                        .map(|d| format!("{} ({})", d.name, d.version_req))
                        .collect();
                    ui.label(
                        RichText::new(format!("deps: {}", deps.join(", ")))
                            .small(),
                    );
                }
            }
        });

    ui.add_space(4.0);
}
