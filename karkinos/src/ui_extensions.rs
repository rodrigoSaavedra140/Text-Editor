//! S9 (Roadmap v0.2.72): pestaña Extensions de la barra lateral.
//! T5 (S4): panel moderno de tarjetas — control segmentado, buscador propio,
//! estados vacíos/carga y hover animado con los mismos tokens del tema.

use eframe::egui;

use crate::app::EditorApp;
use crate::extensions_service::ExtensionsService;
use crate::title_bar::icon_close_button;
use crate::ui_components::{
    compact_card, empty_state, extension_card, loading_skeleton, pill_button, search_field,
    section_label, segmented_control, CardAction, CardState, ExtensionCard, PillKind,
};

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
        let mut retry = false;
        let mut open_marketplace = false;
        let mut download_target: Option<karkinos_core::open_vsx::OpenVsxExtension> = None;
        let mut vsix_to_uninstall: Option<String> = None;
        let mut plugin_to_uninstall: Option<String> = None;
        let mut cmd_to_execute: Option<String> = None;

        // Encabezado: título fuerte + contador gris a la derecha y cierre.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Extensions")
                    .size(15.0)
                    .strong()
                    .color(theme.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_close_button(ui, "Close", theme).clicked() {
                    *close_panel = true;
                }
                let counter = match self.extensions_tab {
                    0 => format!(
                        "{} installed",
                        self.plugin_manager.len()
                            + ExtensionsService::list_installed_vsix().len()
                    ),
                    2 => format!("{} results", self.extensions.open_vsx_results.len()),
                    _ => "4 curated".to_string(),
                };
                ui.label(
                    egui::RichText::new(counter)
                        .size(10.5)
                        .color(theme.text_muted),
                );
            });
        });
        ui.add_space(8.0);

        // Control segmentado. Los índices de `extensions_tab` se mantienen
        // (0 = Installed, 2 = Marketplace, 1 = Showcase) y el mapeo queda acá.
        let mut segment = match self.extensions_tab {
            1 => 2,
            2 => 1,
            _ => 0,
        };
        if segmented_control(
            ui,
            theme,
            &mut segment,
            &["Installed", "Marketplace", "Showcase"],
        ) {
            self.extensions_tab = match segment {
                1 => 2,
                2 => 1,
                _ => 0,
            };
            if self.extensions_tab == 2
                && self.extensions.open_vsx_results.is_empty()
                && !self.extensions.open_vsx_searching
            {
                self.extensions.search_open_vsx("rust");
            }
        }
        ui.add_space(10.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                match self.extensions_tab {
                    0 => {
                        let plugins = self.plugin_manager.installed_plugins();
                        let vsix_list = ExtensionsService::list_installed_vsix();

                        if plugins.is_empty() && vsix_list.is_empty() {
                            empty_state(
                                ui,
                                theme,
                                "🧩",
                                "No extensions yet",
                                "Browse the Marketplace to download packages.",
                            );
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                if pill_button(
                                    ui,
                                    theme,
                                    "Open Marketplace",
                                    PillKind::Primary,
                                )
                                .clicked()
                                {
                                    open_marketplace = true;
                                }
                            });
                        } else {
                            if !plugins.is_empty() {
                                section_label(ui, theme, "PLUGINS");
                                for plugin in &plugins {
                                    let card = ExtensionCard {
                                        title: plugin.name.clone(),
                                        publisher: "Builtin".to_string(),
                                        version: plugin.version.clone(),
                                        description: plugin.description.clone(),
                                        downloads: None,
                                        state: CardState::Active,
                                    };
                                    if extension_card(ui, theme, &card) == CardAction::Delete {
                                        plugin_to_uninstall = Some(plugin.name.clone());
                                    }
                                    if !plugin.commands.is_empty() {
                                        ui.horizontal_wrapped(|ui| {
                                            for cmd in &plugin.commands {
                                                if pill_button(
                                                    ui,
                                                    theme,
                                                    &format!("⚡ {}", cmd.title),
                                                    PillKind::Secondary,
                                                )
                                                .clicked()
                                                {
                                                    cmd_to_execute = Some(cmd.id.clone());
                                                }
                                            }
                                        });
                                    }
                                }
                            }

                            if !vsix_list.is_empty() {
                                section_label(ui, theme, "DOWNLOADED PACKAGES");
                                for (filename, size) in &vsix_list {
                                    if compact_card(
                                        ui,
                                        theme,
                                        filename,
                                        &crate::util::format_size(*size as usize),
                                    ) {
                                        vsix_to_uninstall = Some(filename.clone());
                                    }
                                }
                            }
                        }
                    }
                    1 => {
                        section_label(ui, theme, "COMMUNITY SHOWCASE");
                        let showcase = [
                            (
                                "Git Lens & Blame",
                                "0.3.0",
                                "Inline git blame annotations, commit diff inspection, and file history.",
                            ),
                            (
                                "Markdown Live Preview",
                                "0.4.1",
                                "Real-time side-by-side formatted preview for Markdown and CommonMark files.",
                            ),
                            (
                                "Code Formatter & Linter",
                                "0.2.5",
                                "Automated code formatting for Rust, C/C++, Python, JS/TS, and Go on save.",
                            ),
                            (
                                "Snippet Expansion Pack",
                                "1.1.0",
                                "Fast boilerplate expansion and productivity snippets for popular languages.",
                            ),
                        ];
                        for (name, version, description) in showcase {
                            let card = ExtensionCard {
                                title: name.to_string(),
                                publisher: "Community".to_string(),
                                version: version.to_string(),
                                description: description.to_string(),
                                downloads: None,
                                state: CardState::ComingSoon,
                            };
                            extension_card(ui, theme, &card);
                        }
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "💡 Implement `karkinos_core::plugin::Plugin` and submit a PR to contribute!",
                            )
                            .italics()
                            .size(10.0)
                            .color(theme.text_muted),
                        );
                    }
                    _ => {
                        let response = search_field(
                            ui,
                            theme,
                            &mut self.extensions.open_vsx_query_input,
                            "Search extensions…",
                        );
                        if response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            do_search = true;
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if pill_button(ui, theme, "Search", PillKind::Primary).clicked() {
                                do_search = true;
                            }
                            if !self.extensions.open_vsx_searching {
                                if let Some(ref status) = self.extensions.open_vsx_status {
                                    ui.label(
                                        egui::RichText::new(status)
                                            .size(10.0)
                                            .color(theme.text_muted),
                                    );
                                }
                            }
                        });
                        ui.add_space(8.0);

                        if self.extensions.open_vsx_searching {
                            loading_skeleton(ui, theme, 4);
                        } else if let Some(ref err) = self.extensions.open_vsx_error {
                            empty_state(ui, theme, "⚠", "Couldn't reach Open VSX", err);
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                if pill_button(ui, theme, "Retry", PillKind::Secondary).clicked()
                                {
                                    retry = true;
                                }
                            });
                        } else if self.extensions.open_vsx_results.is_empty() {
                            empty_state(ui, theme, "🔍", "No results", "Try another search term.");
                        } else {
                            let downloading = self.extensions.downloading.clone();
                            for ext in &self.extensions.open_vsx_results {
                                let ext_id = ext.id();
                                let state = if downloading.as_deref() == Some(ext_id.as_str()) {
                                    CardState::Downloading
                                } else if self.extensions.is_vsix_installed(ext) {
                                    CardState::Downloaded
                                } else if ext.download_url.is_empty() {
                                    // Sin URL de descarga: tratamos la entrada como
                                    // no instalable, sin ofrecer un botón que falle.
                                    CardState::ComingSoon
                                } else {
                                    CardState::Available
                                };
                                let card = ExtensionCard {
                                    title: if ext.display_name.is_empty() {
                                        ext.name.clone()
                                    } else {
                                        ext.display_name.clone()
                                    },
                                    publisher: ext.namespace.clone(),
                                    version: ext.version.clone(),
                                    description: ext.description.clone(),
                                    downloads: if ext.download_count > 0 {
                                        Some(ext.download_count)
                                    } else {
                                        None
                                    },
                                    state,
                                };
                                match extension_card(ui, theme, &card) {
                                    CardAction::Download => {
                                        download_target = Some(ext.clone());
                                    }
                                    CardAction::Delete => {
                                        if let Some(name) = ext.vsix_file_name() {
                                            vsix_to_uninstall = Some(name);
                                        }
                                    }
                                    CardAction::None => {}
                                }
                            }
                        }
                    }
                }
            });

        if open_marketplace {
            self.extensions_tab = 2;
            if self.extensions.open_vsx_results.is_empty()
                && !self.extensions.open_vsx_searching
            {
                self.extensions.search_open_vsx("rust");
            }
        }
        if do_search || retry {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;
    use karkinos_core::open_vsx::OpenVsxExtension;

    fn sample_results() -> Vec<OpenVsxExtension> {
        vec![
            OpenVsxExtension {
                namespace: "rust-lang".to_string(),
                name: "rust-analyzer".to_string(),
                version: "0.4.3101".to_string(),
                display_name: "rust-analyzer".to_string(),
                description: "Rust language support: completion, diagnostics and more.".to_string(),
                download_url: "https://example.com/rust-analyzer.vsix".to_string(),
                download_count: 1_200_000,
            },
            OpenVsxExtension {
                namespace: "esbenp".to_string(),
                name: "prettier".to_string(),
                version: "10.1.0".to_string(),
                display_name: "Prettier".to_string(),
                description: "Code formatter using prettier.".to_string(),
                download_url: "https://example.com/prettier.vsix".to_string(),
                download_count: 34_500,
            },
            OpenVsxExtension {
                namespace: "vadimcn".to_string(),
                name: "vscode-lldb".to_string(),
                version: "1.9.2".to_string(),
                display_name: "CodeLLDB".to_string(),
                description: "Native debugger based on LLDB.".to_string(),
                download_url: "https://example.com/lldb.vsix".to_string(),
                download_count: 812,
            },
        ]
    }

    /// T5 (S4): el panel se renderiza sin pánico en las 3 pestañas y en todos
    /// los temas, con listas vacías y con resultados de ejemplo.
    #[test]
    fn render_extensions_tab_sin_panico_en_los_3_tabs_y_temas() {
        for mode in ThemeMode::ALL {
            let theme = mode.theme();
            for tab in [0usize, 1, 2] {
                for with_results in [false, true] {
                    let ctx = egui::Context::default();
                    let mut app = EditorApp::default();
                    app.extensions_tab = tab;
                    app.plugin_manager = karkinos_core::plugin::PluginManager::new();
                    if with_results {
                        app.extensions.open_vsx_results = sample_results();
                    }

                    let raw = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(320.0, 800.0),
                        )),
                        ..Default::default()
                    };
                    let mut closed = false;
                    let _ = ctx.run(raw, |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            app.render_extensions_tab(ui, &theme, &mut closed);
                        });
                    });
                }
            }
        }
    }
}
