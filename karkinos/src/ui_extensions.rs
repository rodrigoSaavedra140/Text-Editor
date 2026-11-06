//! S9 (Roadmap v0.2.72): pestaña Extensions de la barra lateral.

use eframe::egui;

use crate::app::EditorApp;
use crate::extensions_service::ExtensionsService;
use crate::title_bar::icon_close_button;

impl EditorApp {
    /// Pestaña de Extensiones en la barra lateral unificada (v0.2.56):
    /// gestiona plugins instalados, catálogo Open VSX y sugerencias comunitarias
    /// en el mismo panel lateral que el explorador de archivos.
    pub(crate) fn render_extensions_tab(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::theme::Theme,
        close_panel: &mut bool,
    ) {
        let mut do_search = false;
        let mut download_target: Option<karkinos_core::open_vsx::OpenVsxExtension> = None;
        let mut vsix_to_uninstall: Option<String> = None;
        let mut plugin_to_uninstall: Option<String> = None;
        let mut cmd_to_execute: Option<String> = None;

        // Cabecera unificada: EXTENSIONS + botón de cerrar
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("EXTENSIONS")
                    .color(theme.text_muted)
                    .size(10.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_close_button(ui, "Close", theme).clicked() {
                    *close_panel = true;
                }
            });
        });
        ui.add_space(4.0);

        // Sub-pestañas compactas
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.extensions_tab == 0, "Installed")
                .on_hover_text("Installed Extensions & Plugins")
                .clicked()
            {
                self.extensions_tab = 0;
            }
            if ui
                .selectable_label(self.extensions_tab == 2, "Open VSX")
                .on_hover_text("Open VSX Registry")
                .clicked()
            {
                self.extensions_tab = 2;
                if self.extensions.open_vsx_results.is_empty() && !self.extensions.open_vsx_searching {
                    self.extensions.search_open_vsx("rust");
                }
            }
            if ui
                .selectable_label(self.extensions_tab == 1, "Showcase")
                .on_hover_text("Community Showcase")
                .clicked()
            {
                self.extensions_tab = 1;
            }
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.extensions_tab == 0 {
                    ui.label(egui::RichText::new("Active Plugins").strong());
                    ui.add_space(2.0);

                    let plugins = self.plugin_manager.installed_plugins();
                    if plugins.is_empty() {
                        ui.label(
                            egui::RichText::new("No active plugins registered.")
                                .italics()
                                .color(theme.text_muted)
                                .small(),
                        );
                    } else {
                        for plugin in &plugins {
                            ui.group(|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new(&plugin.name).strong());
                                    ui.label(
                                        egui::RichText::new(format!("v{}", plugin.version))
                                            .color(theme.text_muted)
                                            .small(),
                                    );
                                });
                                if !plugin.description.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&plugin.description)
                                            .color(theme.text_muted)
                                            .small(),
                                    );
                                }
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("● Active")
                                            .color(egui::Color32::from_rgb(80, 200, 120))
                                            .small(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("🗑 Uninstall").clicked() {
                                                plugin_to_uninstall = Some(plugin.name.clone());
                                            }
                                        },
                                    );
                                });
                                if !plugin.commands.is_empty() {
                                    ui.add_space(2.0);
                                    ui.horizontal_wrapped(|ui| {
                                        for cmd in &plugin.commands {
                                            if ui
                                                .small_button(format!("⚡ {}", cmd.title))
                                                .clicked()
                                            {
                                                cmd_to_execute = Some(cmd.id.clone());
                                            }
                                        }
                                    });
                                }
                            });
                            ui.add_space(4.0);
                        }
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(egui::RichText::new("Downloaded .vsix Packages").strong());
                    ui.add_space(2.0);

                    let vsix_list = ExtensionsService::list_installed_vsix();
                    if vsix_list.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "No .vsix downloaded yet. Search Open VSX to download packages.",
                            )
                            .italics()
                            .color(theme.text_muted)
                            .small(),
                        );
                    } else {
                        for (filename, size) in &vsix_list {
                            ui.group(|ui| {
                                ui.label(egui::RichText::new(filename).strong().small());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(crate::util::format_size(*size as usize))
                                            .color(theme.text_muted)
                                            .small(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("🗑 Delete").clicked() {
                                                vsix_to_uninstall = Some(filename.clone());
                                            }
                                        },
                                    );
                                });
                            });
                            ui.add_space(3.0);
                        }
                    }
                } else if self.extensions_tab == 1 {
                    ui.label(egui::RichText::new("Community Showcase").strong());
                    ui.label(
                        egui::RichText::new("Curated extensions for Karkinos.")
                            .color(theme.text_muted)
                            .small(),
                    );
                    ui.add_space(4.0);

                    let showcase = [
                        (
                            "Git Lens & Blame",
                            "v0.3.0",
                            "Inline git blame annotations, commit diff inspection, and file history.",
                            "Community",
                        ),
                        (
                            "Markdown Live Preview",
                            "v0.4.1",
                            "Real-time side-by-side formatted preview for Markdown and CommonMark files.",
                            "Community",
                        ),
                        (
                            "Code Formatter & Linter",
                            "v0.2.5",
                            "Automated code formatting for Rust, C/C++, Python, JS/TS, and Go on save.",
                            "Community",
                        ),
                        (
                            "Snippet Expansion Pack",
                            "v1.1.0",
                            "Fast boilerplate expansion and productivity snippets for popular languages.",
                            "Community",
                        ),
                    ];

                    for (name, ver, desc, author) in &showcase {
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(*name).strong());
                                ui.label(
                                    egui::RichText::new(*ver)
                                        .color(theme.text_muted)
                                        .small(),
                                );
                            });
                            ui.label(
                                egui::RichText::new(format!("by {}", author))
                                    .color(egui::Color32::from_rgb(130, 170, 255))
                                    .small(),
                            );
                            ui.label(
                                egui::RichText::new(*desc)
                                    .color(theme.text_muted)
                                    .small(),
                            );
                            ui.label(
                                egui::RichText::new("Coming Soon / In Review")
                                    .italics()
                                    .color(theme.text_muted)
                                    .small(),
                            );
                        });
                        ui.add_space(4.0);
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            "💡 Implement `karkinos_core::plugin::Plugin` and submit a PR to contribute!",
                        )
                        .italics()
                        .color(egui::Color32::from_rgb(180, 200, 230))
                        .small(),
                    );
                } else {
                    ui.label(egui::RichText::new("Open VSX Registry").strong());
                    ui.add_space(2.0);

                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.extensions.open_vsx_query_input)
                                .hint_text("Search (e.g. rust)...")
                                .desired_width(120.0),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            do_search = true;
                        }
                        if ui.button("Search").clicked() {
                            do_search = true;
                        }
                    });

                    if self.extensions.open_vsx_searching {
                        ui.label(
                            egui::RichText::new("Searching…")
                                .italics()
                                .color(egui::Color32::from_rgb(255, 190, 60))
                                .small(),
                        );
                    }
                    if let Some(ref st) = self.extensions.open_vsx_status {
                        ui.label(
                            egui::RichText::new(st)
                                .color(egui::Color32::from_rgb(100, 200, 130))
                                .small(),
                        );
                    }
                    if let Some(ref err) = self.extensions.open_vsx_error {
                        ui.label(
                            egui::RichText::new(format!("Error: {}", err))
                                .color(egui::Color32::from_rgb(255, 90, 90))
                                .small(),
                        );
                    }

                    ui.add_space(4.0);

                    if self.extensions.open_vsx_results.is_empty() && !self.extensions.open_vsx_searching {
                        ui.label(
                            egui::RichText::new(
                                "No extensions found. Try searching for 'rust' or 'theme'.",
                            )
                            .italics()
                            .color(theme.text_muted)
                            .small(),
                        );
                    }

                    for ext in &self.extensions.open_vsx_results {
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(&ext.display_name).strong());
                                ui.label(
                                    egui::RichText::new(format!("v{}", ext.version))
                                        .color(theme.text_muted)
                                        .small(),
                                );
                            });
                            ui.label(
                                egui::RichText::new(format!("by {}", ext.namespace))
                                    .color(egui::Color32::from_rgb(130, 170, 255))
                                    .small(),
                            );
                            if !ext.description.is_empty() {
                                ui.label(
                                    egui::RichText::new(&ext.description)
                                        .color(theme.text_muted)
                                        .small(),
                                );
                            }
                            ui.horizontal(|ui| {
                                if ext.download_count > 0 {
                                    ui.label(
                                        egui::RichText::new(format!("📥 {}", ext.download_count))
                                            .small()
                                            .color(theme.text_muted),
                                    );
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let vsix_name = ext.vsix_file_name();
                                        if self.extensions.is_vsix_installed(ext) {
                                            if ui.small_button("🗑 Delete").clicked() {
                                                vsix_to_uninstall = vsix_name;
                                            }
                                            ui.label(
                                                egui::RichText::new("● Downloaded")
                                                    .color(egui::Color32::from_rgb(80, 200, 120))
                                                    .small()
                                                    .strong(),
                                            );
                                        } else if !ext.download_url.is_empty()
                                            && ui.small_button("📥 Download").clicked()
                                        {
                                            download_target = Some(ext.clone());
                                        }
                                    },
                                );
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
            });

        if do_search {
            let q = self.extensions.open_vsx_query_input.clone();
            self.extensions.search_open_vsx(&q);
        }
        if let Some(target) = download_target {
            self.extensions.download_open_vsx_extension(&target);
        }
        if let Some(filename) = vsix_to_uninstall {
            self.extensions.uninstall_vsix(&mut self.doc, &filename);
        }
        if let Some(name) = plugin_to_uninstall {
            self.uninstall_plugin(&name);
        }
        if let Some(cmd_id) = cmd_to_execute {
            self.execute_plugin_command(&cmd_id);
        }
    }
}
