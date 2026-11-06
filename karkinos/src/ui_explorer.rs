//! S9 (Roadmap v0.2.72): pestaña Explorador de la barra lateral.

use eframe::egui;

use crate::app::EditorApp;
use crate::title_bar::{icon_close_button, icon_collapse_button};

impl EditorApp {
    pub(crate) fn render_explorer_tab(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::theme::Theme,
        close_panel: &mut bool,
    ) {
        let tree = crate::app::FileTreeNode::build_from_paths_and_dirs(
            &self.doc.browse_dir,
            &self.explorer.files,
            &self.explorer.directories,
        );
        let root_name = self
            .doc
            .browse_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Workspace")
            .to_string();
        let active_path = self.doc.current_file.clone();

        let mut open_file = None;
        let mut toggle_dir = None;
        let mut delete_file = None;
        let mut rename_file = None;
        let mut refresh = false;
        let mut collapse_all = false;
        let mut start_new_file = false;
        let mut start_new_folder = false;
        let mut start_open_folder = false;

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("EXPLORER")
                    .color(theme.text_muted)
                    .size(10.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_close_button(ui, "Close", theme).clicked() {
                    *close_panel = true;
                }
                if icon_collapse_button(ui, "Collapse All", theme).clicked() {
                    collapse_all = true;
                }
                if ui.button("↻").on_hover_text("Refresh").clicked() {
                    refresh = true;
                }
                // v0.2.41: creación inline de archivos y carpetas.
                if ui.button("+📄").on_hover_text("New File").clicked() {
                    start_new_file = true;
                }
                if ui.button("+📁").on_hover_text("New Folder").clicked() {
                    start_new_folder = true;
                }
                // v0.2.45 (Tarea C): cambiar de carpeta de trabajo.
                if ui.button("📂").on_hover_text("Open Folder (Change Workspace)").clicked() {
                    start_open_folder = true;
                }
            });
        });
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(format!("📂 {root_name}"))
                .color(theme.text)
                .size(13.5)
                .strong(),
        );
        ui.separator();

        if start_new_file {
            self.explorer.begin_new_item(
                crate::app::ExplorerNewItemKind::File,
                Some(self.doc.browse_dir.clone()),
            );
        }
        if start_new_folder {
            self.explorer.begin_new_item(
                crate::app::ExplorerNewItemKind::Folder,
                Some(self.doc.browse_dir.clone()),
            );
        }
        if start_open_folder {
            self.prompt_open_folder();
        }

        if let Some(kind) = self.explorer.creating_item {
            let mut submit = false;
            let mut cancel = false;
            let (icon, hint) = match kind {
                crate::app::ExplorerNewItemKind::File => ("📄", "new_file.rs"),
                crate::app::ExplorerNewItemKind::Folder => ("📁", "new_folder"),
            };

            egui::Frame::none()
                .fill(theme.elevated_bg)
                .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(icon).size(12.0));
                        let input = ui.add_sized(
                            [(ui.available_width() - 64.0).max(60.0), 22.0],
                            egui::TextEdit::singleline(&mut self.explorer.new_item_name)
                                .id_source("explorer_new_item_input")
                                .hint_text(hint),
                        );
                        if self.explorer.new_item_focus {
                            input.request_focus();
                            self.explorer.new_item_focus = false;
                        }
                        if input.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            submit = true;
                        }
                        if ui.button("OK").on_hover_text("Create (Enter)").clicked() {
                            submit = true;
                        }
                        if icon_close_button(ui, "Cancel (Esc)", theme).clicked() {
                            cancel = true;
                        }
                    });
                });

            if submit {
                // Si falla, el diálogo queda abierto para corregir el nombre.
                let _ = self.create_explorer_item();
            } else if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.explorer.cancel_new_item();
            }
        }

        if let Some(path) = self.explorer.item_to_delete.clone() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            let mut confirm = false;
            let mut cancel = false;

            egui::Frame::none()
                .fill(theme.elevated_bg)
                .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("🗑 Delete {name}?"))
                                .color(theme.text)
                                .size(11.5),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if icon_close_button(ui, "Cancel (Esc)", theme).clicked() {
                                    cancel = true;
                                }
                                if ui.button("Delete").clicked() {
                                    confirm = true;
                                }
                            },
                        );
                    });
                });

            if confirm {
                let _ = self.delete_explorer_file(&path);
            } else if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.explorer.item_to_delete = None;
            }
        }

        // v0.2.49: renombrado inline del explorador. El campo se muestra
        // mientras `renaming_item` tenga un path; Enter/✓ confirma, Esc/x cancela.
        if self.explorer.renaming_item.is_some() {
            let display = self
                .explorer
                .renaming_item
                .as_ref()
                .and_then(|(p, _)| p.file_name().and_then(|n| n.to_str()))
                .unwrap_or("item")
                .to_string();
            let focus = self.explorer.rename_focus;
            self.explorer.rename_focus = false;
            let mut submit = false;
            let mut cancel = false;

            egui::Frame::none()
                .fill(theme.elevated_bg)
                .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("✏").size(12.0));
                        ui.label(
                            egui::RichText::new(display)
                                .color(theme.text_muted)
                                .size(11.5),
                        );
                        let input = ui.add_sized(
                            [(ui.available_width() - 64.0).max(60.0), 22.0],
                            egui::TextEdit::singleline(
                                &mut self.explorer.renaming_item.as_mut().expect("renaming").1,
                            )
                            .id_source("explorer_rename_input")
                            .hint_text("new name"),
                        );
                        if focus {
                            input.request_focus();
                        }
                        if input.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            submit = true;
                        }
                        if ui.button("OK").on_hover_text("Rename (Enter)").clicked() {
                            submit = true;
                        }
                        if icon_close_button(ui, "Cancel (Esc)", theme).clicked() {
                            cancel = true;
                        }
                    });
                });

            if submit {
                // Si falla, el diálogo queda abierto para corregir el nombre.
                let _ = self.confirm_rename_explorer_item();
            } else if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.cancel_rename_explorer_item();
            }
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Self::draw_file_tree(
                    ui,
                    &tree,
                    0,
                    &self.explorer.collapsed_dirs,
                    active_path.as_deref(),
                    theme,
                    &mut toggle_dir,
                    &mut open_file,
                    &mut delete_file,
                    &mut rename_file,
                );
            });

        if refresh {
            self.explorer.loaded_for = None;
            self.explorer.collapsed_dirs.clear();
            self.refresh_explorer();
        }
        if collapse_all {
            let mut dirs = Vec::new();
            crate::app::FileTreeNode::collect_directories(&tree, &mut dirs);
            self.explorer.collapsed_dirs.extend(dirs);
        }
        if let Some(dir) = toggle_dir {
            if !self.explorer.collapsed_dirs.remove(&dir) {
                self.explorer.collapsed_dirs.insert(dir);
            }
        }
        if let Some(path) = delete_file {
            self.explorer.cancel_new_item();
            self.explorer.item_to_delete = Some(path);
        }
        if let Some(path) = rename_file {
            self.begin_rename_explorer_item(&path);
        }
        if let Some(path) = open_file {
            self.open_path(path);
        }
    }

    /// Filas recursivas del árbol del explorador (UI-6 / v0.2.39).
    /// Tanto carpetas como archivos son interactivos en todo el ancho del renglón para que
    /// hacer clic en cualquier punto de la fila amplíe o esconda la carpeta o abra el archivo.
    #[allow(clippy::too_many_arguments)]
    fn draw_file_tree(
        ui: &mut egui::Ui,
        nodes: &[crate::app::FileTreeNode],
        depth: usize,
        collapsed: &std::collections::HashSet<std::path::PathBuf>,
        active: Option<&std::path::Path>,
        theme: &crate::theme::Theme,
        toggle_dir: &mut Option<std::path::PathBuf>,
        open_file: &mut Option<std::path::PathBuf>,
        delete_file: &mut Option<std::path::PathBuf>,
        rename_file: &mut Option<std::path::PathBuf>,
    ) {
        for node in nodes {
            match node {
                crate::app::FileTreeNode::Directory { name, path, children } => {
                    let is_collapsed = collapsed.contains(path);
                    let indent = depth as f32 * 14.0 + 4.0;
                    let row_height = 24.0;

                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click(),
                    );

                    let row_id = ui.id().with(("explorer_dir_row", path));
                    let hover_t = ui
                        .ctx()
                        .animate_bool_with_time(row_id, response.hovered(), 0.12);
                    if hover_t > 0.001 {
                        ui.painter().rect_filled(
                            rect,
                            egui::Rounding::same(4.0),
                            theme.active_line.gamma_multiply(2.5 * hover_t),
                        );
                    }

                    // v0.2.49: botón ✏ de renombrado (solo al hover). Su rect
                    // se excluye del click que pliega/despliega la carpeta.
                    let rename_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.right() - 22.0, rect.top() + 4.0),
                        egui::vec2(18.0, 16.0),
                    );
                    let over_rename = ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| rename_rect.contains(p));

                    if response.clicked() && !over_rename {
                        *toggle_dir = Some(path.clone());
                    }

                    let icon_chevron = if is_collapsed { "▸" } else { "▾" };
                    // v0.2.42: una carpeta vacía mantiene el icono 📁 al expandirse.
                    let is_empty = children.is_empty();
                    let icon_folder = if is_collapsed || is_empty { "📁" } else { "📂" };
                    let label = format!("{icon_chevron} {icon_folder} {name}");

                    let text_color = crate::theme::lerp_color(
                        theme.text.gamma_multiply(0.9),
                        theme.text,
                        hover_t,
                    );
                    ui.painter().text(
                        egui::pos2(rect.left() + indent, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(13.0),
                        text_color,
                    );

                    response.on_hover_cursor(egui::CursorIcon::PointingHand);

                    // v0.2.49: botón ✏ de renombrado de carpeta al hover.
                    if hover_t > 0.02 {
                        let btn_color = theme.text_muted.gamma_multiply(hover_t);
                        ui.painter().text(
                            rename_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "✏",
                            egui::FontId::proportional(10.0),
                            btn_color,
                        );
                        let rename = ui.interact(
                            rename_rect,
                            ui.id().with(("explorer_rename", path)),
                            egui::Sense::click(),
                        );
                        if rename.clicked() {
                            *rename_file = Some(path.clone());
                        }
                        rename
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Rename (F2)");
                    }

                    // v0.2.42: una carpeta expandida sin hijos muestra un
                    // renglón tenue para dejar claro que está vacía.
                    if !is_collapsed && is_empty {
                        let empty_indent = (depth as f32 + 1.0) * 14.0 + 4.0;
                        let (empty_rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            egui::pos2(empty_rect.left() + empty_indent, empty_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "(empty)",
                            egui::FontId::proportional(11.5),
                            theme.text_muted.gamma_multiply(0.75),
                        );
                    }

                    if !is_collapsed {
                        Self::draw_file_tree(
                            ui,
                            children,
                            depth + 1,
                            collapsed,
                            active,
                            theme,
                            toggle_dir,
                            open_file,
                            delete_file,
                            rename_file,
                        );
                    }
                }
                crate::app::FileTreeNode::File {
                    name,
                    path,
                    extension,
                } => {
                    let indent = (depth as f32 + 1.0) * 14.0 + 4.0;
                    let is_active = active == Some(path.as_path());
                    let row_height = 24.0;

                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click(),
                    );

                    let row_id = ui.id().with(("explorer_file_row", path));
                    let hover_t = ui
                        .ctx()
                        .animate_bool_with_time(row_id, response.hovered(), 0.12);
                    if is_active {
                        ui.painter().rect_filled(
                            rect,
                            egui::Rounding::same(4.0),
                            theme.accent.gamma_multiply(0.2),
                        );
                    } else if hover_t > 0.001 {
                        ui.painter().rect_filled(
                            rect,
                            egui::Rounding::same(4.0),
                            theme.active_line.gamma_multiply(1.8 * hover_t),
                        );
                    }

                    // v0.2.41/0.2.49: botones sutiles al hacer hover (✏ renombrar,
                    // 🗑 borrar). Los rects se excluyen del click que abre el archivo.
                    let trash_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.right() - 22.0, rect.top() + 4.0),
                        egui::vec2(18.0, 16.0),
                    );
                    let rename_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.right() - 44.0, rect.top() + 4.0),
                        egui::vec2(18.0, 16.0),
                    );
                    let over_trash = ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| trash_rect.contains(p));
                    let over_rename = ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| rename_rect.contains(p));

                    if response.clicked() && !over_trash && !over_rename {
                        *open_file = Some(path.clone());
                    }

                    let icon = explorer_file_icon(extension);
                    let label = format!("{icon} {name}");

                    let text_color = if is_active {
                        theme.accent
                    } else {
                        crate::theme::lerp_color(theme.text_muted, theme.text, hover_t)
                    };
                    ui.painter().text(
                        egui::pos2(rect.left() + indent, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(13.0),
                        text_color,
                    );

                    response.on_hover_cursor(egui::CursorIcon::PointingHand);

                    if hover_t > 0.02 {
                        let btn_color = theme.text_muted.gamma_multiply(hover_t);
                        ui.painter().text(
                            rename_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "✏",
                            egui::FontId::proportional(10.0),
                            btn_color,
                        );
                        let rename = ui.interact(
                            rename_rect,
                            ui.id().with(("explorer_rename", path)),
                            egui::Sense::click(),
                        );
                        if rename.clicked() {
                            *rename_file = Some(path.clone());
                        }
                        rename
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Rename (F2)");

                        ui.painter().text(
                            trash_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "🗑",
                            egui::FontId::proportional(10.0),
                            btn_color,
                        );
                        let trash = ui.interact(
                            trash_rect,
                            ui.id().with(("explorer_delete", path)),
                            egui::Sense::click(),
                        );
                        if trash.clicked() {
                            *delete_file = Some(path.clone());
                        }
                        trash
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Delete file");
                    }
                }
            }
        }
    }

}

/// UI-6: icono contextual del explorador según la extensión.
fn explorer_file_icon(extension: &str) -> &'static str {
    match extension {
        "rs" => "⚡",
        "toml" => "⚙",
        "md" => "📄",
        "json" => "📦",
        "html" | "css" | "js" | "ts" => "🌐",
        _ => "📄",
    }
}
