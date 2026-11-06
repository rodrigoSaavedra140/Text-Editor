//! S9 (Roadmap v0.2.72): pestaña Source Control de la barra lateral.

use eframe::egui;

use crate::app::EditorApp;
use crate::title_bar::icon_close_button;

impl EditorApp {
    pub(crate) fn render_source_control_tab(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::theme::Theme,
        close_panel: &mut bool,
    ) {
        let mut refresh = false;
        let mut commit = false;
        let mut stage_all = false;

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SOURCE CONTROL")
                    .color(theme.text_muted)
                    .size(10.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_close_button(ui, "Close", theme).clicked() {
                    *close_panel = true;
                }
                if ui
                    .button("↻")
                    .on_hover_text("Refresh status")
                    .clicked()
                {
                    refresh = true;
                }
            });
        });
        ui.add_space(4.0);

        let branch = self
            .git_status
            .branch
            .clone()
            .unwrap_or_else(|| "no branch".to_string());
        ui.label(
            egui::RichText::new(format!("🌿 {branch}"))
                .color(theme.text)
                .size(11.5)
                .strong(),
        );
        if self.git_ui.is_loading {
            ui.label(
                egui::RichText::new("Loading…")
                    .color(theme.text_muted)
                    .size(10.0),
            );
        }
        ui.add_space(4.0);

        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(&mut self.git_ui.commit_message)
                .id_source("git_commit_message")
                .hint_text("Commit message"),
        );
        ui.horizontal(|ui| {
            if ui.button("Commit").clicked() {
                commit = true;
            }
            if ui.button("+ Stage All").clicked() {
                stage_all = true;
            }
        });

        if let Some(error) = &self.git_ui.last_error {
            ui.label(
                egui::RichText::new(format!("⚠ {error}"))
                    .color(egui::Color32::from_rgb(220, 120, 120))
                    .size(10.5),
            );
        }
        ui.separator();

        if self.git_ui.entries.is_empty() {
            ui.label(
                egui::RichText::new("No changes")
                    .color(theme.text_muted)
                    .size(11.0),
            );
        } else {
            let mut open_path: Option<std::path::PathBuf> = None;
            let mut stage_path: Option<std::path::PathBuf> = None;
            let mut diff_request: Option<(std::path::PathBuf, bool)> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for entry in &self.git_ui.entries {
                        let (badge, badge_color) = git_status_badge(&entry.kind);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(badge)
                                    .color(badge_color)
                                    .monospace()
                                    .size(10.0)
                                    .strong(),
                            );
                            let file = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&entry.rel_path)
                                        .color(theme.text)
                                        .size(11.5),
                                )
                                .wrap(false)
                                .sense(egui::Sense::click()),
                            );
                            if file.clicked() {
                                open_path = Some(entry.path.clone());
                            }
                            file.on_hover_cursor(egui::CursorIcon::PointingHand);

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("+")
                                        .on_hover_text("Stage file")
                                        .clicked()
                                    {
                                        stage_path = Some(entry.path.clone());
                                    }
                                    if ui
                                        .small_button("Diff")
                                        .on_hover_text("View Diff")
                                        .clicked()
                                    {
                                        diff_request = Some((entry.path.clone(), entry.staged));
                                    }
                                    if entry.staged {
                                        ui.label(
                                            egui::RichText::new("staged")
                                                .color(theme.text_muted)
                                                .size(9.0),
                                        );
                                    }
                                },
                            );
                        });
                    }
                });

            if let Some(path) = open_path {
                self.open_path(path);
            }
            if let Some(path) = stage_path {
                let _ = self.git_stage_path(&path);
            }
            if let Some((path, staged)) = diff_request {
                self.open_git_diff(&path, staged);
            }
        }

        if refresh {
            self.refresh_git_status();
        }
        if commit {
            let _ = self.git_commit();
        }
        if stage_all {
            let _ = self.git_stage_all();
        }
    }

}

/// v0.2.42: badge y color de la lista de Source Control.
fn git_status_badge(
    kind: &karkinos_core::git_status::GitStatusKind,
) -> (&'static str, egui::Color32) {
    use karkinos_core::git_status::GitStatusKind;

    match kind {
        GitStatusKind::Modified => ("M", egui::Color32::from_rgb(220, 180, 80)),
        GitStatusKind::Added => ("A", egui::Color32::from_rgb(120, 220, 160)),
        GitStatusKind::Deleted => ("D", egui::Color32::from_rgb(220, 120, 120)),
        GitStatusKind::Untracked => ("U", egui::Color32::from_rgb(200, 120, 220)),
        GitStatusKind::Renamed => ("R", egui::Color32::from_rgb(80, 180, 220)),
        GitStatusKind::Other(_) => ("?", egui::Color32::from_rgb(150, 150, 150)),
    }
}
