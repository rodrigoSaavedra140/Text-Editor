use std::path::PathBuf;

use eframe::egui;

use crate::app::{picker_should_stay_open, EditorApp};
use crate::title_bar::{draw_resize_handles, title_bar_icon_button, TitleBarIcon};
use crate::util::{format_size, request_close};
use crate::LOGO_BYTES;

// Métodos de EditorApp que dibujan cosas con egui. La lógica de
// negocio (abrir/guardar/deshacer) vive en app.rs — Rust permite
// repartir los métodos de un mismo struct en varios `impl` de
// distintos archivos, así que acá van los que necesitan `egui`.
impl EditorApp {
    /// Popup del explorador de archivos (carpetas + archivos del
    /// browse_dir actual), posicionado debajo del botón "Abrir…".
    /// A diferencia de egui::ComboBox, se mantiene abierto mientras
    /// se navega de carpeta en carpeta: cada frame re-dibuja el
    /// listado de la carpeta actual (cached_list_dir refresca solo
    /// si cambió la carpeta o su mtime).
    fn show_picker_popup(&mut self, ctx: &egui::Context) {
        if !self.picker_open {
            return;
        }
        // Esc lo cierra por convención (igual que la barra buscar).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.picker_open = false;
            return;
        }

        let button_rect = self.picker_button_rect;
        let pos = button_rect
            .map(|r| r.left_bottom())
            .unwrap_or_else(|| ctx.screen_rect().left_top() + egui::vec2(10.0, 34.0));

        let dir_label = format!("📂 {}", self.doc.browse_dir.display());
        let mut navigate_to: Option<PathBuf> = None;
        let mut open_file: Option<PathBuf> = None;

        let area_response = egui::Area::new(egui::Id::new("file_picker_popup"))
            .order(egui::Order::Foreground)
            .constrain(true)
            .fixed_pos(pos)
            .pivot(egui::Align2::LEFT_TOP)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(280.0);
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        ui.label(&dir_label);
                        ui.separator();

                        // Subir a la carpeta padre (si existe).
                        if let Some(parent) = self.doc.browse_dir.parent() {
                            if ui.selectable_label(false, "⬆ ..").clicked() {
                                navigate_to = Some(parent.to_path_buf());
                            }
                        }

                        // Caché del listado (ver cached_list_dir en
                        // app.rs): sin read_dir por frame.
                        let (dirs, files) = self.doc.cached_list_dir();

                        for name in &dirs {
                            if ui.selectable_label(false, format!("📁 {}", name)).clicked() {
                                navigate_to = Some(self.doc.browse_dir.join(name));
                            }
                        }
                        for name in &files {
                            if ui.selectable_label(false, name).clicked() {
                                open_file = Some(self.doc.browse_dir.join(name));
                            }
                        }
                        if dirs.is_empty() && files.is_empty() {
                            ui.label("(empty directory)");
                        }
                    });
                });
            });

        // Elegir un ARCHIVO cierra el popup (ya se abrió en el
        // editor): es el objetivo final del selector.
        if let Some(dir) = navigate_to {
            self.doc.browse_dir = dir;
        }
        if let Some(path) = open_file {
            self.picker_open = false;
            self.open_path(path);
        }

        // Click COMPLETO afuera del popup (y que no sea el propio
        // toggle del botón): cerrar. La decisión va por el helper
        // puro picker_should_stay_open (testeable).
        let (any_click, click_pos) =
            ctx.input(|i| (i.pointer.any_click(), i.pointer.interact_pos()));
        if any_click {
            if let Some(click_pos) = click_pos {
                if !picker_should_stay_open(
                    click_pos,
                    Some(area_response.response.rect),
                    button_rect,
                ) {
                    self.picker_open = false;
                }
            }
        }
    }

    /// Dibuja la ventana emergente de progreso mientras haya un
    /// guardado en curso, y la cierra sola cuando termina.
    fn show_save_progress(&mut self, ctx: &egui::Context) {
        let Some(progress) = &self.doc.save_progress else {
            return;
        };
        let (written, total, done, error) = {
            let p = progress.lock().unwrap();
            (p.written, p.total, p.done, p.error.clone())
        };

        let fraction = if total == 0 { 1.0 } else { written as f32 / total as f32 };

        egui::Window::new("Saving…")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(280.0);
                ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).show_percentage());
                ui.label(format!("{} / {}", format_size(written), format_size(total)));
            });

        // Mientras esté guardando, seguimos pidiendo repintar cada
        // frame — si no, la barra de progreso quedaría congelada
        // hasta que el usuario mueva el mouse o toque algo.
        ctx.request_repaint();

        if done {
            // S1: un guardado bien hecho refresca el mtime recordado
            // (¡el disco acaba de cambiar!) y un guardado fallido
            // cancela el "cerrar tras guardar" para no matar la
            // ventana encima de un error.
            self.doc.note_save_done();
            match error {
                Some(e) => self.doc.status = format!("Save error: {}", e),
                None => {
                    self.doc.dirty = false;
                    self.doc.status = "Saved".to_string();
                }
            }
            self.doc.save_progress = None;
        } else {
            ctx.request_repaint();
        }

        // S1: si el usuario pidió guardar-Y-cerrar desde el diálogo y
        // el guardado terminó bien, se revisa si quedan pestañas sucias antes de cerrar.
        if self.doc.close_after_save && self.doc.save_progress.is_none() {
            self.doc.close_after_save = false;
            if let Some(next_dirty) = self.doc.first_dirty_tab_index() {
                self.switch_tab(next_dirty);
                self.doc.close_pending = true;
            } else {
                request_close(ctx, &mut self.doc.closing_since);
            }
        }
        // S3: si se pidió guardar y cerrar una pestaña puntual
        if let Some(target_idx) = self.doc.tab_close_after_save {
            if self.doc.save_progress.is_none() {
                self.doc.tab_close_after_save = None;
                self.close_tab(target_idx);
            }
        }
        // S8: si el usuario pidió guardar-Y-nuevo desde el diálogo y
        // el guardado terminó bien, limpiamos el editor para el nuevo documento.
        if self.doc.new_after_save && self.doc.save_progress.is_none() {
            self.doc.new_after_save = false;
            self.new_document();
        }
    }

    /// S1 / S8: ventana de confirmación "¿cerrar con cambios sin
    /// guardar?" o "¿nuevo con cambios sin guardar?", visible mientras
    /// `close_pending` o `new_doc_pending`. Tres caminos:
    /// Guardar (guarda y procede al terminar), Descartar (procede sin
    /// guardar) y Cancelar (vuelve al editor sin tocar nada). NADA
    /// mata el proceso mientras el diálogo esté abierto.
    fn show_close_confirm(&mut self, ctx: &egui::Context) {
        if !self.doc.close_pending && !self.doc.new_doc_pending {
            return;
        }
        // Esc = Cancelar, convención universal de diálogos.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.doc.cancel_close();
            return;
        }
        let is_new = self.doc.new_doc_pending;
        let dirty_count = self.doc.dirty_tabs_count();
        let title = if is_new { "New Document" } else { "Unsaved Changes" };
        let doc_name = self
            .doc
            .tabs
            .get(self.doc.active_tab)
            .map(|t| t.display_title())
            .unwrap_or_else(|| "this document".to_string());

        let prompt = if is_new {
            "You have unsaved changes. Do you want to save before creating a new document?".to_string()
        } else if dirty_count > 1 {
            format!(
                "You have {} tabs with unsaved changes.\nDo you want to save \"{}\" before quitting?",
                dirty_count, doc_name
            )
        } else {
            format!(
                "You have unsaved changes in \"{}\".\nDo you want to save before quitting?",
                doc_name
            )
        };

        let mut open = true;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(prompt);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if is_new {
                            self.doc.confirm_new_doc_save();
                        } else {
                            self.doc.confirm_close_save();
                        }
                    }
                    if ui.button("Discard").clicked() {
                        if is_new {
                            self.doc.new_doc_pending = false;
                            self.new_document();
                        } else {
                            self.doc.confirm_close_discard();
                            self.doc.dirty = false;
                            if let Some(tab) = self.doc.tabs.get_mut(self.doc.active_tab) {
                                tab.dirty = false;
                            }
                            if let Some(next_dirty) = self.doc.first_dirty_tab_index() {
                                self.switch_tab(next_dirty);
                                self.doc.close_pending = true;
                            } else {
                                request_close(ctx, &mut self.doc.closing_since);
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.doc.cancel_close();
                    }
                });
            });
        if !open {
            // La X del propio diálogo cuenta como Cancelar.
            self.doc.cancel_close();
        }
    }

    /// S8 (Etapa 2) / S3: Diálogo de confirmación para cerrar pestaña con cambios sin guardar.
    fn show_tab_close_confirm(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.doc.tab_close_pending else {
            return;
        };
        let mut save_and_close = false;
        let mut discard = false;
        let mut cancel = false;

        let title = self
            .doc
            .tabs
            .get(idx)
            .map(|t| t.display_title())
            .unwrap_or_default();

        egui::Window::new("Close Tab")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!("Tab \"{}\" has unsaved changes.", title));
                ui.label("Do you want to save changes before closing it?");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_and_close = true;
                    }
                    if ui.button("Discard").clicked() {
                        discard = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if save_and_close {
            self.doc.tab_close_pending = None;
            if idx != self.doc.active_tab {
                self.switch_tab(idx);
            }
            self.doc.tab_close_after_save = Some(idx);
            self.doc.try_save();
        } else if discard {
            self.doc.tab_close_pending = None;
            if idx == self.doc.active_tab {
                self.doc.dirty = false;
            }
            if let Some(tab) = self.doc.tabs.get_mut(idx) {
                tab.dirty = false;
            }
            self.close_tab(idx);
        } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.doc.tab_close_pending = None;
        }
    }

    /// Diálogo modal para solicitar nombre al guardar un archivo sin nombre.
    fn show_save_name_dialog(&mut self, ctx: &egui::Context) {
        if !self.doc.save_name_pending {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.doc.cancel_save_name();
            return;
        }

        let mut apply = false;
        let mut cancel = false;
        let mut is_open = true;

        egui::Window::new("Save File")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Enter a name for this file:");
                ui.label(
                    egui::RichText::new("(If left blank, it will be saved as untitled.txt)")
                        .color(egui::Color32::from_gray(140))
                        .small(),
                );
                ui.add_space(4.0);

                let field = ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut self.doc.save_name_input)
                        .hint_text("untitled.txt"),
                );
                field.request_focus();

                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if !is_open || cancel {
            self.doc.cancel_save_name();
        } else if apply {
            self.doc.confirm_save_name();
        }
    }

    /// S4 (Rust IDE): Modal popup for Quick Fixes & Code Actions (Ctrl+. / Alt+Enter)
    fn show_code_actions_modal(&mut self, ctx: &egui::Context) {
        if !self.lsp.code_actions_menu_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.lsp.code_actions_menu_open = false;
            return;
        }

        let mut applied_action = None;
        let mut close_modal = false;

        egui::Window::new("💡 Quick Fix / Code Actions (Ctrl+.)")
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                if self.lsp.code_actions.is_empty() {
                    ui.label("No quick fixes or assists available at cursor.");
                    ui.add_space(8.0);
                    if ui.button("Close (Esc)").clicked() {
                        close_modal = true;
                    }
                } else {
                    ui.label(egui::RichText::new("Select an assist to apply:").small());
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for (idx, action) in self.lsp.code_actions.iter().enumerate() {
                            let is_pref = if action.is_preferred == Some(true) { "★ " } else { "" };
                            let kind_badge = action.kind.as_deref().unwrap_or("assist");
                            let text = format!("{}[{}] {}", is_pref, kind_badge, action.title);

                            if ui
                                .add_sized([ui.available_width(), 26.0], egui::Button::new(text))
                                .clicked()
                            {
                                applied_action = Some(idx);
                            }
                        }
                    });

                    ui.separator();
                    if ui.button("Cancel (Esc)").clicked() {
                        close_modal = true;
                    }
                }
            });

        if let Some(idx) = applied_action {
            self.apply_code_action(idx);
        } else if close_modal {
            self.lsp.code_actions_menu_open = false;
        }
    }

    /// S4 (Rust IDE): Modal dialog for symbol renaming (F2)
    fn show_rename_dialog(&mut self, ctx: &egui::Context) {
        if !self.lsp.rename_dialog_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.lsp.rename_dialog_open = false;
            return;
        }

        let mut apply = false;
        let mut cancel = false;

        egui::Window::new("✏ Rename Symbol (F2)")
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Enter new name for symbol:");
                ui.add_space(4.0);

                let field = ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut self.lsp.rename_input).hint_text("New symbol name"),
                );
                field.request_focus();

                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Rename").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel (Esc)").clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.lsp.rename_dialog_open = false;
        } else if apply {
            let name = self.lsp.rename_input.clone();
            self.lsp.rename_dialog_open = false;
            self.request_rename(&name);
        }
    }

    /// Cierre de ventana con puertas de datos: si hay cambios sin
    /// guardar (en la pestaña activa o en cualquier otra) o un guardado
    /// en pleno vuelo, NO cierra nada — planta el diálogo S1 y, si el cierre
    /// vino del sistema (X/Alt+F4 del WM), lo cancela para que la ventana
    /// siga viva mientras el usuario decide. Sin cambios: cierre de toda la vida.
    fn try_close(&mut self, ctx: &egui::Context) {
        self.sync_active_to_tab();

        // Si el archivo está vacío o todas las pestañas sucias están vacías, salimos de inmediato sin diálogo
        if self.doc.all_dirty_tabs_are_empty() && self.doc.save_progress.is_none() {
            request_close(ctx, &mut self.doc.closing_since);
            return;
        }

        if let Some(dirty_idx) = self.doc.first_dirty_tab_index() {
            if dirty_idx != self.doc.active_tab {
                self.switch_tab(dirty_idx);
            }
            self.doc.close_pending = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if self.doc.save_progress.is_some() {
            self.doc.close_pending = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else {
            request_close(ctx, &mut self.doc.closing_since);
        }
    }

    /// S2: barra de búsqueda y reemplazo debajo del editor (Ctrl+F).
    fn show_find_bar(&mut self, ctx: &egui::Context) {
        if !self.find_open {
            return;
        }
        egui::TopBottomPanel::bottom("find_bar")
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(30, 30, 30)).inner_margin(6.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Find:");
                    let find_field = ui.add(
                        egui::TextEdit::singleline(&mut self.find_query)
                            .desired_width(200.0)
                            .hint_text("text…"),
                    );
                    if find_field.changed() {
                        self.find_update_matches();
                        if !self.find_matches.is_empty() {
                            self.find_goto_current();
                        }
                    }
                    // Enter en el campo = siguiente match.
                    if find_field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.find_next();
                        find_field.request_focus();
                    }

                    let total = self.find_matches.len();
                    if total > 0 {
                        ui.label(format!("{}/{}", self.find_current + 1, total));
                    } else if !self.find_query.is_empty() {
                        ui.label("0/0");
                    }

                    if ui.button("◀").clicked() {
                        self.find_prev();
                    }
                    if ui.button("▶").clicked() {
                        self.find_next();
                    }

                    let mut case = self.find_case_sensitive;
                    if ui.checkbox(&mut case, "Aa").changed() {
                        self.find_case_sensitive = case;
                        self.find_update_matches();
                        if !self.find_matches.is_empty() {
                            self.find_goto_current();
                        }
                    }

                    ui.separator();
                    ui.label("Replace:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.find_replace)
                            .desired_width(150.0)
                            .hint_text("replacement…"),
                    );
                    if ui.button("Replace").clicked() {
                        self.find_replace_current();
                    }
                    if ui.button("All").clicked() {
                        self.find_replace_all();
                    }

                    if ui.button("✕").clicked() {
                        self.find_open = false;
                    }
                });
            });
    }

    /// S1 (Rust IDE): Panel inferior para ejecución y diagnósticos de Cargo (F7 / F5 / Ctrl+\).
    fn show_cargo_panel(&mut self, ctx: &egui::Context, theme: &crate::theme::Theme) {
        if !self.cargo.panel_open {
            return;
        }

        let mut close_panel = false;
        let mut jump_target: Option<karkinos_core::cargo::CargoDiagnostic> = None;

        let status = self.cargo.runner.status();
        let (errors_count, warnings_count) = self.cargo.runner.error_and_warning_counts();

        egui::TopBottomPanel::bottom("cargo_panel")
            .resizable(true)
            .default_height(200.0)
            .min_height(100.0)
            .max_height(450.0)
            .frame(egui::Frame::none().fill(theme.panel_bg))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🦀 Cargo Pipeline").strong());

                    let (status_text, status_color) = match status {
                        karkinos_core::cargo::CargoStatus::Idle => ("Idle", egui::Color32::from_gray(140)),
                        karkinos_core::cargo::CargoStatus::Running => ("Running…", egui::Color32::from_rgb(255, 190, 60)),
                        karkinos_core::cargo::CargoStatus::Success => ("Success", egui::Color32::from_rgb(80, 200, 120)),
                        karkinos_core::cargo::CargoStatus::Failed(_code) => {
                            ("Failed", egui::Color32::from_rgb(255, 90, 90))
                        }
                        karkinos_core::cargo::CargoStatus::Killed => ("Terminated", egui::Color32::from_gray(160)),
                    };

                    ui.label(egui::RichText::new(format!("[{}]", status_text)).color(status_color).strong());

                    if let Some(action) = self.cargo.runner.current_action {
                        ui.label(egui::RichText::new(action.display_name()).italics().color(egui::Color32::from_gray(170)));
                    }

                    ui.separator();

                    if ui.selectable_label(self.cargo.active_tab == 0, "Terminal Output").clicked() {
                        self.cargo.active_tab = 0;
                    }

                    let diag_tab_label = format!("Diagnostics ({} err, {} warn)", errors_count, warnings_count);
                    let diag_color = if errors_count > 0 {
                        egui::Color32::from_rgb(255, 90, 90)
                    } else if warnings_count > 0 {
                        egui::Color32::from_rgb(255, 190, 60)
                    } else {
                        egui::Color32::from_gray(180)
                    };

                    if ui.selectable_label(self.cargo.active_tab == 1, egui::RichText::new(diag_tab_label).color(diag_color)).clicked() {
                        self.cargo.active_tab = 1;
                    }

                    let dap_label = if self.dap.client.as_ref().map(|c| c.state().is_stopped).unwrap_or(false) {
                        "🐞 Debugger (STOPPED)"
                    } else if self.dap.client.is_some() {
                        "🐞 Debugger (Running)"
                    } else {
                        "🐞 Debugger"
                    };
                    if ui.selectable_label(self.cargo.active_tab == 2, dap_label).clicked() {
                        self.cargo.active_tab = 2;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            close_panel = true;
                        }
                        if ui.small_button("Clear").clicked() {
                            self.cargo.runner.clear();
                        }
                        if self.cargo.runner.is_running() {
                            if ui.small_button("⏹ Stop").clicked() {
                                self.cargo.runner.kill();
                            }
                        }
                    });
                });

                ui.separator();

                if self.cargo.active_tab == 0 {
                    let lines = self.cargo.runner.raw_lines();
                    egui::ScrollArea::both()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in lines {
                                let color = if line.is_error {
                                    egui::Color32::from_rgb(255, 120, 120)
                                } else {
                                    theme.text
                                };
                                ui.label(egui::RichText::new(line.text).monospace().color(color));
                            }
                        });
                } else if self.cargo.active_tab == 1 {
                    let diags = self.cargo.runner.diagnostics();
                    if diags.is_empty() {
                        ui.label(egui::RichText::new("No compiler errors or warnings detected.").italics().color(egui::Color32::from_gray(140)));
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for diag in &diags {
                                let (badge, color) = match diag.level.as_str() {
                                    "error" => ("● ERROR", egui::Color32::from_rgb(255, 80, 80)),
                                    "warning" => ("▲ WARN", egui::Color32::from_rgb(255, 190, 60)),
                                    _ => ("ℹ NOTE", egui::Color32::from_rgb(100, 180, 255)),
                                };

                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(badge).strong().color(color));
                                        if let Some(ref file) = diag.file {
                                            let loc_str = format!("{}:{}:{}", file.display(), diag.line + 1, diag.col + 1);
                                            if ui.link(egui::RichText::new(loc_str).color(egui::Color32::from_rgb(130, 190, 255))).clicked() {
                                                jump_target = Some(diag.clone());
                                            }
                                        }
                                    });
                                    ui.label(egui::RichText::new(&diag.message).color(theme.text));
                                    if !diag.rendered.is_empty() {
                                        ui.collapsing("Compiler Details", |ui| {
                                            ui.label(egui::RichText::new(&diag.rendered).monospace().small());
                                        });
                                    }
                                });
                                ui.add_space(2.0);
                            }
                        });
                    }
                } else if self.cargo.active_tab == 2 {
                    // S4 (Rust IDE): DAP Native Debugger & Inspector
                    let dap_state = self.dap.client.as_ref().map(|c| c.state()).unwrap_or_default();

                    ui.horizontal(|ui| {
                        let is_active = dap_state.is_active;
                        let is_stopped = dap_state.is_stopped;

                        if !is_active {
                            if ui.button("▶ Start Debugging (F5)").clicked() {
                                self.start_debugging();
                            }
                        } else if is_stopped {
                            if ui.button("▶ Continue (F5)").clicked() {
                                self.start_debugging();
                            }
                        } else {
                            if ui.button("⏸ Pause").clicked() {
                                self.pause_debugger();
                            }
                        }

                        if ui.add_enabled(is_stopped, egui::Button::new("↷ Step Over (F10)")).clicked() {
                            self.step_over();
                        }
                        if ui.add_enabled(is_stopped, egui::Button::new("⇲ Step In (F11)")).clicked() {
                            self.step_in();
                        }
                        if ui.add_enabled(is_stopped, egui::Button::new("⇱ Step Out (Shift+F11)")).clicked() {
                            self.step_out();
                        }
                        if ui.add_enabled(is_active, egui::Button::new("⏹ Stop (Shift+F5)")).clicked() {
                            self.stop_debugging();
                        }

                        if let Some(ref reason) = dap_state.stop_reason {
                            ui.label(egui::RichText::new(format!("Stopped: {:?}", reason)).strong().color(egui::Color32::from_rgb(255, 215, 0)));
                        }
                    });

                    ui.separator();

                    ui.columns(3, |cols| {
                        // Col 0: Call Stack
                        cols[0].label(egui::RichText::new("Call Stack:").strong());
                        if dap_state.stack_frames.is_empty() {
                            cols[0].label(egui::RichText::new("No active frames").italics().small());
                        } else {
                            egui::ScrollArea::vertical().id_source("dap_stack_scroll").max_height(140.0).show(&mut cols[0], |ui| {
                                for frame in &dap_state.stack_frames {
                                    let label = format!("{}:{}", frame.name, frame.line + 1);
                                    if ui.selectable_label(false, label).clicked() {
                                        if let Some(ref path) = frame.source_path {
                                            if path.exists() {
                                                self.open_path(path.clone());
                                                self.editor_state.cursor_line = frame.line;
                                                self.editor_state.cursor_col = frame.column;
                                                self.editor_state.needs_autoscroll = true;
                                            }
                                        }
                                    }
                                }
                            });
                        }

                        // Col 1: Variables
                        cols[1].label(egui::RichText::new("Variables:").strong());
                        if dap_state.variables.is_empty() {
                            cols[1].label(egui::RichText::new("No local variables").italics().small());
                        } else {
                            egui::ScrollArea::vertical().id_source("dap_vars_scroll").max_height(140.0).show(&mut cols[1], |ui| {
                                for var in &dap_state.variables {
                                    let type_part = var.type_name.as_deref().map(|t| format!(": {}", t)).unwrap_or_default();
                                    ui.label(egui::RichText::new(format!("{}{}", var.name, type_part)).monospace().color(theme.syntax_keyword));
                                    ui.label(egui::RichText::new(format!(" = {}", var.value)).monospace().color(theme.text));
                                }
                            });
                        }

                        // Col 2: Breakpoints
                        cols[2].label(egui::RichText::new("Breakpoints:").strong());
                        if self.dap.breakpoints.is_empty() {
                            cols[2].label(egui::RichText::new("No breakpoints set (F9 to toggle)").italics().small());
                        } else {
                            let mut to_remove = None;
                            egui::ScrollArea::vertical().id_source("dap_bps_scroll").max_height(140.0).show(&mut cols[2], |ui| {
                                for (path, line) in &self.dap.breakpoints {
                                    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("● {}:{}", file_name, line + 1)).color(egui::Color32::from_rgb(240, 70, 70)));
                                        if ui.small_button("×").clicked() {
                                            to_remove = Some((path.clone(), *line));
                                        }
                                    });
                                }
                            });
                            if let Some(target) = to_remove {
                                self.dap.breakpoints.remove(&target);
                                self.sync_editor_breakpoints();
                            }
                        }
                    });
                }
            });

        if close_panel {
            self.cargo.panel_open = false;
        }

        if let Some(target) = jump_target {
            self.jump_to_cargo_diagnostic(&target);
        }
    }

    /// S4: ventana pequeña "Ir a línea" de Ctrl+G. Enter aplica,
    /// Esc cierra; la X del propio diálogo cuenta como cerrar.
    /// Números inválidos no mueven el cursor: solo avisan.
    /// S4: diálogo "Ir a línea N" como ventana flotante (Ctrl+G).
    fn show_goto_line(&mut self, ctx: &egui::Context) {
        if !self.goto_pending {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.goto_pending = false;
            return;
        }
        let mut apply = false;
        let mut enter_pressed = false;
        egui::Window::new("Go to Line")
            // La X del diálogo escribe false directamente en el flag.
            .open(&mut self.goto_pending)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0))
            .show(ctx, |ui| {
                ui.set_min_width(240.0);
                ui.horizontal(|ui| {
                    ui.label("Line:");
                    let field =
                        ui.add(egui::TextEdit::singleline(&mut self.goto_input).char_limit(12));
                    // El campo pide el foco cada frame del diálogo:
                    // escribir y Enter salen solos.
                    field.request_focus();
                    if ui.button("Go").clicked() {
                        apply = true;
                    }
                    // Enter del campo (con foco) = Ir. Se lleva a una
                    // flag local: no hay valores que "escapen" del
                    // closure.
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        enter_pressed = true;
                    }
                });
            });
        if enter_pressed {
            apply = true;
        }
        if apply {
            self.goto_pending = false;
            match crate::util::parse_goto_line(&self.goto_input, self.doc.lines.len()) {
                Some(idx) => {
                    self.editor_state.cursor_line = idx;
                    self.editor_state.cursor_col = 0;
                    self.editor_state.sel_anchor = Some((idx, 0));
                    self.editor_state.request_autoscroll();
                    self.doc.status = format!("Line {}", idx + 1);
                }
                None => self.doc.status = "Please enter a valid line number".to_string(),
            }
        }
    }

    /// S7 (Etapa 2): Diálogo modal de búsqueda rápida de archivos (Ctrl+P / Quick Open).
    fn show_quick_open(&mut self, ctx: &egui::Context) {
        if !self.quick_open.open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.quick_open.close();
            return;
        }

        let mut is_open = true;
        let mut query_changed = false;
        let mut open_selected = false;

        egui::Window::new("🔍 Quick Open (Ctrl+P)")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 50.0))
            .show(ctx, |ui| {
                ui.set_min_width(480.0);
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    let input_resp = ui.add_sized(
                        [ui.available_width() - 8.0, 24.0],
                        egui::TextEdit::singleline(&mut self.quick_open.query)
                            .hint_text("Type to search files... (↑/↓ navigate, Enter open, Esc close)"),
                    );
                    if self.quick_open.request_focus {
                        input_resp.request_focus();
                        self.quick_open.request_focus = false;
                    }
                    if input_resp.changed() {
                        query_changed = true;
                    }
                });

                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.quick_open.select_prev();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    self.quick_open.select_next();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    open_selected = true;
                }

                ui.separator();
                // S4 (Roadmap v0.2.9): estados de fondo — nunca un "0 files
                // found" que mienta mientras el worker escanea o puntúa.
                if self.quick_open.indexing {
                    ui.label("Scanning… (indexing in background)");
                } else if self.quick_open.scoring {
                    ui.label(format!(
                        "Scoring… ({} files indexed)",
                        self.quick_open.scanned_files.len()
                    ));
                } else {
                    let total = self.quick_open.filtered.len();
                    ui.label(format!("{} files found", total));
                }

                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    let mut clicked_idx = None;
                    for (idx, (path, _score)) in self.quick_open.filtered.iter().enumerate().take(50) {
                        let is_selected = idx == self.quick_open.selected_idx;
                        let path_str = path.display().to_string();
                        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or(&path_str);
                        let parent = path.parent().and_then(|p| p.to_str()).unwrap_or("");

                        let text = if parent.is_empty() {
                            egui::RichText::new(file_name).strong()
                        } else {
                            egui::RichText::new(format!("{}  ({})", file_name, parent))
                        };

                        let row = ui.selectable_label(is_selected, text);
                        if row.clicked() {
                            clicked_idx = Some(idx);
                        }
                    }
                    if let Some(idx) = clicked_idx {
                        self.quick_open.selected_idx = idx;
                        open_selected = true;
                    }
                });
            });

        if !is_open {
            self.quick_open.close();
            return;
        }

        if query_changed {
            let q = self.quick_open.query.clone();
            self.quick_open.set_query(q, &mut self.quick_open_services);
        }

        if open_selected {
            if let Some(rel) = self.quick_open.selected_path() {
                let full = self.doc.browse_dir.join(rel);
                self.quick_open.close();
                self.open_path(full);
            }
        }
    }

    /// Carga el logo como textura de egui, una sola vez (la primera
    /// vez que hace falta dibujarlo), aplicando recorte alfa para
    /// que se fusione perfectamente con cualquier tema.
    fn ensure_logo_texture(&mut self, ctx: &egui::Context) {
        if self.logo_texture.is_some() {
            return;
        }
        if let Ok(img) = image::load_from_memory(LOGO_BYTES) {
            let img = img.to_rgba8();
            let color_image = cutout_dark_background(&img);
            self.logo_texture =
                Some(ctx.load_texture("karkinos_logo", color_image, egui::TextureOptions::default()));
        }
    }

    /// Fondo minimalista: SOLO el logo, bien tenue, centrado — se
    /// dibuja mientras el buffer está vacío, y queda "detrás" de
    /// donde vas a empezar a escribir. El nombre "Karkinos" ya se
    /// muestra en la barra de título propia, así que acá no hace
    /// falta repetirlo. El tinte se adapta según el tema visual.
    fn draw_watermark(&self, ui: &egui::Ui, rect: egui::Rect, _theme: &crate::theme::Theme) {
        let painter = ui.painter();
        let center = rect.center();

        if let Some(tex) = &self.logo_texture {
            let logo_side = (rect.height() * 0.32).min(260.0);
            let logo_rect = egui::Rect::from_center_size(center, egui::vec2(logo_side, logo_side));
            let tint = egui::Color32::from_white_alpha(45);
            painter.image(
                tex.id(),
                logo_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
    }

    /// Diálogo modal para explorar extensiones de la comunidad y plugins instalados.
    fn show_extensions_dialog(&mut self, ctx: &egui::Context) {
        if !self.extensions_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.extensions_modal_open = false;
            return;
        }

        let mut cmd_to_execute: Option<String> = None;
        let mut open = self.extensions_modal_open;
        let mut close_clicked = false;

        egui::Window::new("🧩 Community Extensions & Plugins")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.extensions_tab == 0, "📦 Installed Extensions").clicked() {
                        self.extensions_tab = 0;
                    }
                    if ui.selectable_label(self.extensions_tab == 1, "🌐 Community Showcase").clicked() {
                        self.extensions_tab = 1;
                    }
                    if ui.selectable_label(self.extensions_tab == 2, "🛒 Open VSX Registry").clicked() {
                        self.extensions_tab = 2;
                        if self.open_vsx_results.is_empty() && !self.open_vsx_searching {
                            self.search_open_vsx("rust");
                        }
                    }
                });
                ui.separator();

                if self.extensions_tab == 0 {
                    ui.label(egui::RichText::new("Installed Extensions & Active Plugins").strong());
                    ui.add_space(4.0);

                    let mut plugin_to_uninstall: Option<String> = None;
                    let mut vsix_to_uninstall: Option<String> = None;

                    let plugins = self.plugin_manager.installed_plugins();
                    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        if plugins.is_empty() {
                            ui.label(
                                egui::RichText::new("No active plugins registered.")
                                    .italics()
                                    .color(egui::Color32::from_gray(140)),
                            );
                        }
                        for plugin in &plugins {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&plugin.name).strong());
                                    ui.label(
                                        egui::RichText::new(format!("v{}", plugin.version))
                                            .color(egui::Color32::from_gray(140)),
                                    );
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("🗑️ Uninstall").clicked() {
                                            plugin_to_uninstall = Some(plugin.name.clone());
                                        }
                                        ui.label(
                                            egui::RichText::new("● Active")
                                                .color(egui::Color32::from_rgb(80, 200, 120)),
                                        );
                                    });
                                });
                                if !plugin.description.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&plugin.description)
                                            .color(egui::Color32::from_gray(180)),
                                    );
                                }
                                if !plugin.commands.is_empty() {
                                    ui.add_space(3.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Commands:");
                                        for cmd in &plugin.commands {
                                            if ui.small_button(format!("⚡ {}", cmd.title)).clicked() {
                                                cmd_to_execute = Some(cmd.id.clone());
                                            }
                                        }
                                    });
                                }
                            });
                            ui.add_space(4.0);
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(egui::RichText::new("Downloaded .vsix Packages").strong());
                        ui.add_space(2.0);

                        let vsix_list = Self::list_installed_vsix();
                        if vsix_list.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "No .vsix packages downloaded yet. Use the Open VSX Registry tab to find extensions.",
                                )
                                .italics()
                                .color(egui::Color32::from_gray(140)),
                            );
                        } else {
                            for (filename, size) in &vsix_list {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(filename).strong());
                                        ui.label(
                                            egui::RichText::new(format_size(*size as usize))
                                                .color(egui::Color32::from_gray(140)),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("🗑️ Uninstall").clicked() {
                                                vsix_to_uninstall = Some(filename.clone());
                                            }
                                        });
                                    });
                                });
                                ui.add_space(3.0);
                            }
                        }
                    });

                    if let Some(name) = plugin_to_uninstall {
                        self.uninstall_plugin(&name);
                    }
                    if let Some(filename) = vsix_to_uninstall {
                        self.uninstall_vsix(&filename);
                    }
                } else if self.extensions_tab == 1 {
                    ui.label(egui::RichText::new("Community Extensions & Contributions").strong());
                    ui.label(
                        egui::RichText::new("Discover and contribute community-created extensions for Karkinos.")
                            .color(egui::Color32::from_gray(160)),
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

                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        for (name, ver, desc, author) in &showcase {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(*name).strong());
                                    ui.label(
                                        egui::RichText::new(*ver).color(egui::Color32::from_gray(140)),
                                    );
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(
                                            egui::RichText::new(format!("by {}", author))
                                                .color(egui::Color32::from_rgb(130, 170, 255)),
                                        );
                                    });
                                });
                                ui.label(
                                    egui::RichText::new(*desc).color(egui::Color32::from_gray(180)),
                                );
                                ui.label(
                                    egui::RichText::new("Coming Soon / In Review")
                                        .italics()
                                        .color(egui::Color32::from_gray(130)),
                                );
                            });
                            ui.add_space(4.0);
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            "💡 Want to contribute an extension? Implement `karkinos_core::plugin::Plugin` and submit a PR!",
                        )
                        .italics()
                        .color(egui::Color32::from_rgb(180, 200, 230)),
                    );
                } else {
                    ui.label(egui::RichText::new("🛒 Open VSX Registry (open-vsx.org)").strong());
                    ui.label(
                        egui::RichText::new("Search, browse, and download community extensions and themes (.vsix).")
                            .color(egui::Color32::from_gray(160)),
                    );
                    ui.add_space(4.0);

                    let mut do_search = false;
                    let mut download_target: Option<karkinos_core::open_vsx::OpenVsxExtension> = None;
                    let mut vsix_to_uninstall_registry: Option<String> = None;

                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.open_vsx_query_input)
                                .hint_text("Search Open VSX (e.g. rust, theme)...")
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            do_search = true;
                        }
                        if ui.button("Search").clicked() {
                            do_search = true;
                        }
                        if self.open_vsx_searching {
                            ui.label(egui::RichText::new("Searching…").italics().color(egui::Color32::from_rgb(255, 190, 60)));
                        }
                    });

                    if let Some(ref st) = self.open_vsx_status {
                        ui.label(egui::RichText::new(st).color(egui::Color32::from_rgb(100, 200, 130)).small());
                    }
                    if let Some(ref err) = self.open_vsx_error {
                        ui.label(egui::RichText::new(format!("Error: {}", err)).color(egui::Color32::from_rgb(255, 90, 90)).small());
                    }

                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        if self.open_vsx_results.is_empty() && !self.open_vsx_searching {
                            ui.label(egui::RichText::new("No extensions found. Try searching for 'rust' or 'theme'.").italics());
                        }

                        for ext in &self.open_vsx_results {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&ext.display_name).strong());
                                    ui.label(egui::RichText::new(format!("v{}", ext.version)).color(egui::Color32::from_gray(140)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(
                                            egui::RichText::new(format!("by {}", ext.namespace))
                                                .color(egui::Color32::from_rgb(130, 170, 255)),
                                        );
                                    });
                                });

                                if !ext.description.is_empty() {
                                    ui.label(egui::RichText::new(&ext.description).color(egui::Color32::from_gray(180)));
                                }

                                ui.horizontal(|ui| {
                                    if ext.download_count > 0 {
                                        ui.label(egui::RichText::new(format!("📥 {} downloads", ext.download_count)).small().color(egui::Color32::from_gray(140)));
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let vsix_name = format!("{}-{}.vsix", ext.name, ext.version);
                                        if self.is_vsix_installed(ext) {
                                            if ui.small_button("🗑️ Uninstall").clicked() {
                                                vsix_to_uninstall_registry = Some(vsix_name);
                                            }
                                            ui.label(
                                                egui::RichText::new("✓ Installed")
                                                    .color(egui::Color32::from_rgb(80, 200, 120))
                                                    .strong(),
                                            );
                                        } else if !ext.download_url.is_empty() {
                                            if ui.small_button("📥 Download .vsix").clicked() {
                                                download_target = Some(ext.clone());
                                            }
                                        }
                                    });
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });

                    if do_search {
                        let q = self.open_vsx_query_input.clone();
                        self.search_open_vsx(&q);
                    }
                    if let Some(target) = download_target {
                        self.download_open_vsx_extension(&target);
                    }
                    if let Some(filename) = vsix_to_uninstall_registry {
                        self.uninstall_vsix(&filename);
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });
            });

        if close_clicked {
            open = false;
        }

        self.extensions_modal_open = open;

        if let Some(cmd_id) = cmd_to_execute {
            self.execute_plugin_command(&cmd_id);
            self.extensions_modal_open = false;
        }
    }
}

/// Procesa una imagen RGBA eliminando el fondo oscuro opaco y suavizando
/// los bordes con transparencia alfa (feathering), para que el logo se
/// mezcle de forma limpia sin contornos ni bordes cuadrados sobre
/// cualquier tema visual o color de fondo.
pub fn cutout_dark_background(img: &image::RgbaImage) -> egui::ColorImage {
    let (w, h) = (img.width(), img.height());
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for p in img.pixels() {
        let max_c = p[0].max(p[1]).max(p[2]);
        if max_c <= 5 {
            // Fondo oscuro / negro: 100% transparente para eliminar cualquier contorno rectangular
            pixels.push(egui::Color32::TRANSPARENT);
        } else if max_c < 25 {
            // Borde anti-alias del arte: desvanecimiento suave (feathering) respetando
            // la premultiplicación nativa de Color32
            let alpha = ((max_c as f32 - 5.0) / (25.0 - 5.0) * 255.0).round() as u8;
            pixels.push(egui::Color32::from_rgba_premultiplied(
                p[0], p[1], p[2], alpha,
            ));
        } else {
            // Arte / cangrejo visible: opaco con su color original
            pixels.push(egui::Color32::from_rgb(p[0], p[1], p[2]));
        }
    }
    egui::ColorImage {
        size: [w as usize, h as usize],
        pixels,
    }
}

/// Genera el `IconData` con fondo transparente recortado para la ventana del SO (taskbar / dock).
pub fn load_window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(LOGO_BYTES).ok()?.to_rgba8();
    let (width, height) = (img.width(), img.height());
    let mut raw = img.into_raw();
    for chunk in raw.chunks_exact_mut(4) {
        let max_c = chunk[0].max(chunk[1]).max(chunk[2]);
        if max_c <= 5 {
            chunk[0] = 0;
            chunk[1] = 0;
            chunk[2] = 0;
            chunk[3] = 0;
        } else if max_c < 25 {
            let alpha = ((max_c as f32 - 5.0) / (25.0 - 5.0) * 255.0).round() as u8;
            chunk[3] = alpha;
        } else {
            chunk[3] = 255;
        }
    }
    Some(egui::IconData { rgba: raw, width, height })
}

impl eframe::App for EditorApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // S10 y S5: persistir zoom y tema visual
        storage.set_string("editor_font_size", crate::app::font_size_to_string(self.font_size));
        storage.set_string("editor_theme", self.theme_mode.to_storage_str().to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show_save_progress(ctx);
        // S1: comprobación externa de disco (limitada a 1 stat/seg
        // dentro del método; aquí solo es invocarla por frame).
        self.doc.check_external_changes();
        // S2 (Rust IDE): Procesar respuestas pendientes del LSP (definición, hover)
        self.poll_lsp_results();
        // S3 (Roadmap v0.2.9): resultados del worker de Tree-sitter en
        // fondo — si aplicó spans nuevos, se repinta en seguida (soft).
        if self.poll_ast_results() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        // S4 (Roadmap v0.2.9): resultados del worker de Quick Open (escaneo
        // + scoring en fondo) — si aplicó algo nuevo, repintado suave.
        if self.quick_open.poll(&mut self.quick_open_services) {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        // S3 (Rust IDE): Procesar respuestas pendientes de Open VSX
        self.poll_open_vsx();
        // S4 (Rust IDE): Procesar respuestas y eventos del DAP (debugger)
        self.poll_dap_results();

        // S3: consolidación de chunks de undo por timeout de inactividad (~700ms).
        let now = std::time::Instant::now();
        if self.doc.chunk_tracker.check_idle_timeout(now) {
            self.doc.snapshot_if_changed();
        }
        if let Some(remaining) = self.doc.chunk_tracker.remaining_time(now) {
            ctx.request_repaint_after(remaining);
        }

        // Atajos de teclado globales
        let (
            want_save,
            want_save_as,
            want_undo,
            want_redo,
            want_quit,
            want_goto,
            want_find,
            want_new,
            want_quick_open,
            want_new_tab,
            want_close_tab,
            want_next_tab,
            want_prev_tab,
        ) = ctx.input(|i| {
            (
                i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::S),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Z),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Y),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Q),
                i.modifiers.ctrl && i.key_pressed(egui::Key::G),
                i.modifiers.ctrl && i.key_pressed(egui::Key::F),
                i.modifiers.ctrl && i.key_pressed(egui::Key::N),
                i.modifiers.ctrl && i.key_pressed(egui::Key::P),
                i.modifiers.ctrl && i.key_pressed(egui::Key::T),
                i.modifiers.ctrl && i.key_pressed(egui::Key::W),
                i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Tab),
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Tab),
            )
        });
        if want_new {
            self.try_new_document();
        }
        if want_new_tab {
            self.new_tab();
        }
        if want_close_tab {
            self.try_close_tab(self.doc.active_tab);
        }
        if want_next_tab {
            self.next_tab();
        }
        if want_prev_tab {
            self.prev_tab();
        }
        if want_quick_open {
            if self.quick_open.open {
                self.quick_open.close();
            } else {
                self.quick_open
                    .request_open(&self.doc.browse_dir, &mut self.quick_open_services);
            }
        }
        if want_save {
            self.doc.try_save();
        }
        if want_save_as {
            self.doc.save_as();
        }
        if want_undo {
            self.undo();
        }
        if want_redo {
            self.redo();
        }
        let (
            want_cargo_check,
            want_cargo_run,
            want_cargo_toggle,
            want_goto_def,
            want_hover,
            want_code_action,
            want_rename,
            want_breakpoint_toggle,
            want_debug_step_over,
            want_debug_step_in,
            want_debug_step_out,
            want_debug_stop,
        ) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F7),
                i.key_pressed(egui::Key::F5) && !i.modifiers.shift,
                i.modifiers.ctrl && i.key_pressed(egui::Key::Backslash),
                i.key_pressed(egui::Key::F12),
                i.key_pressed(egui::Key::F1) || (i.modifiers.ctrl && i.key_pressed(egui::Key::K)),
                (i.modifiers.ctrl && i.key_pressed(egui::Key::Period))
                    || (i.modifiers.alt && i.key_pressed(egui::Key::Enter)),
                i.key_pressed(egui::Key::F2),
                i.key_pressed(egui::Key::F9),
                i.key_pressed(egui::Key::F10),
                i.key_pressed(egui::Key::F11) && !i.modifiers.shift,
                i.key_pressed(egui::Key::F11) && i.modifiers.shift,
                i.key_pressed(egui::Key::F5) && i.modifiers.shift,
            )
        });
        if want_cargo_check {
            self.run_cargo(karkinos_core::cargo::CargoAction::Check);
        }
        if want_cargo_run {
            self.run_cargo(karkinos_core::cargo::CargoAction::Run);
        }
        if want_cargo_toggle {
            self.cargo.panel_open = !self.cargo.panel_open;
        }
        if want_goto_def {
            self.goto_definition();
        }
        if want_hover {
            self.request_hover();
        }
        if want_code_action {
            self.request_code_actions();
        }
        if want_rename {
            self.lsp.rename_input.clear();
            self.lsp.rename_dialog_open = true;
        }
        if want_breakpoint_toggle {
            let line = self.editor_state.cursor_line;
            self.toggle_breakpoint(line);
        }
        if want_debug_step_over {
            self.step_over();
        }
        if want_debug_step_in {
            self.step_in();
        }
        if want_debug_step_out {
            self.step_out();
        }
        if want_debug_stop {
            self.stop_debugging();
        }
        if want_quit {
            self.try_close(ctx);
        }
        // S4: Ctrl+G abre "Ir a línea" — precargado con la línea
        // actual para editar el número en vez de escribirlo de cero.
        if want_goto && !self.doc.close_pending {
            self.goto_input = format!("{}", self.editor_state.cursor_line + 1);
            self.goto_pending = true;
        }
        // S2: Ctrl+F abre/cierra la barra de búsqueda.
        if want_find {
            self.find_open = !self.find_open;
            if self.find_open {
                self.find_update_matches();
            }
        }
        // S2: Esc cierra la barra de búsqueda cuando está abierta.
        if self.find_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.find_open = false;
        }

        // S10: Ctrl+= (y Ctrl+Plus) agranda, Ctrl+- achica la fuente.
        let (want_zoom_in, want_zoom_out) = ctx.input(|i| {
            (
                i.modifiers.ctrl
                    && (i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus)),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Minus),
            )
        });
        if want_zoom_in {
            self.zoom(true);
        }
        if want_zoom_out {
            self.zoom(false);
        }

        // Lo mismo si el cierre vino del botón X de la ventana o de
        // Alt+F4 (no solo de nuestros botones/atajos): con cambios
        // sin guardar try_close lo CANCELA y planta el diálogo.
        let x_close_requested = ctx.input(|i| i.viewport().close_requested());
        if x_close_requested {
            self.try_close(ctx);
        }

        self.ensure_logo_texture(ctx);

        let theme = self.theme();

        // Le sacamos la decoración nativa a la ventana (ver main()),
        // así que esta barra ocupa exactamente el lugar donde antes
        // estaba la barra nativa del sistema operativo — con nuestro
        // logo, "Karkinos", y los mismos botones de siempre.
        // UI-2 (v0.2.20): Tier 1 — barra de título unificada (36 px). Logo y
        // nombre, pill de búsqueda y acciones de Cargo a la izquierda; tema,
        // extensiones, consola y controles de ventana a la derecha. Toda la
        // barra arrastra la ventana y el doble clic la maximiza.
        egui::TopBottomPanel::top("custom_title_bar")
            .exact_height(crate::title_bar::TITLE_BAR_HEIGHT)
            .frame(egui::Frame::none().fill(theme.surface_bg))
            .show(ctx, |ui| {
                let bar_rect = ui.max_rect();
                let drag_response =
                    ui.interact(bar_rect, ui.id().with("title_bar_drag"), egui::Sense::click_and_drag());
                if drag_response.double_clicked() {
                    let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                }
                if drag_response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    if let Some(tex) = &self.logo_texture {
                        ui.add(egui::Image::new((tex.id(), egui::vec2(18.0, 18.0))));
                    }
                    ui.add_space(6.0);
                    crate::title_bar::faux_bold_label(
                        ui,
                        "Karkinos",
                        theme.text,
                        egui::FontId::proportional(14.0),
                        egui::Sense::hover(),
                    );
                    ui.add_space(12.0);

                    // Pill de búsqueda: abre Quick Open (Ctrl+P).
                    let search_pill = ui.add_sized(
                        [260.0, 26.0],
                        egui::Button::new(
                            egui::RichText::new("🔍 Search files... (Ctrl+P)")
                                .color(theme.text_muted)
                                .size(12.0),
                        )
                        .fill(theme.surface_bg)
                        .rounding(egui::Rounding::same(6.0))
                        .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle)),
                    );
                    if search_pill.clicked() {
                        self.quick_open
                            .request_open(&self.doc.browse_dir, &mut self.quick_open_services);
                    }
                    ui.add_space(8.0);

                    // Acciones de Cargo, compactas, a la derecha de la pill.
                    if ui.button("▶ Run").on_hover_text("Run cargo run (F5)").clicked() {
                        self.run_cargo(karkinos_core::cargo::CargoAction::Run);
                    }
                    if ui.button("🐞 Debug").on_hover_text("Start / Resume Debugger (F5)").clicked() {
                        self.cargo.panel_open = true;
                        self.cargo.active_tab = 2;
                        self.start_debugging();
                    }
                    if ui.button("Check").on_hover_text("Run cargo check (F7)").clicked() {
                        self.run_cargo(karkinos_core::cargo::CargoAction::Check);
                    }
                    if ui.button("Test").on_hover_text("Run cargo test").clicked() {
                        self.run_cargo(karkinos_core::cargo::CargoAction::Test);
                    }
                    if ui.button("Clippy").on_hover_text("Run cargo clippy").clicked() {
                        self.run_cargo(karkinos_core::cargo::CargoAction::Clippy);
                    }

                    // Lado derecho: lo primero agregado queda más a la derecha.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if title_bar_icon_button(ui, TitleBarIcon::Close, &theme).clicked() {
                            self.try_close(ctx);
                        }
                        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        let icon = if is_maximized { TitleBarIcon::Restore } else { TitleBarIcon::Maximize };
                        if title_bar_icon_button(ui, icon, &theme).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                        }
                        if title_bar_icon_button(ui, TitleBarIcon::Minimize, &theme).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.add_space(6.0);

                        let panel_toggle_label = if self.cargo.panel_open { "Console ▾" } else { "Console ▸" };
                        if ui.button(panel_toggle_label).on_hover_text("Toggle Cargo Console (Ctrl+\\)").clicked() {
                            self.cargo.panel_open = !self.cargo.panel_open;
                        }

                        // Menú de extensiones: comunidad y plugins activos.
                        let mut chosen_cmd: Option<String> = None;
                        ui.menu_button("🧩 Extensions", |ui| {
                            if ui.button("📦 Installed Extensions…").clicked() {
                                self.extensions_modal_open = true;
                                self.extensions_tab = 0;
                                ui.close_menu();
                            }
                            if ui.button("🗑️ Uninstall / Manage Extensions…").clicked() {
                                self.extensions_modal_open = true;
                                self.extensions_tab = 0;
                                ui.close_menu();
                            }
                            if ui.button("🛒 Open VSX Registry…").clicked() {
                                self.extensions_modal_open = true;
                                self.extensions_tab = 2;
                                if self.open_vsx_results.is_empty() && !self.open_vsx_searching {
                                    self.search_open_vsx("rust");
                                }
                                ui.close_menu();
                            }
                            if ui.button("🌐 Browse Community Showcase…").clicked() {
                                self.extensions_modal_open = true;
                                self.extensions_tab = 1;
                                ui.close_menu();
                            }
                            ui.separator();
                            ui.label(egui::RichText::new("Quick Commands:").strong());
                            let commands = self.plugin_manager.available_commands();
                            for cmd in commands {
                                if ui.button(format!("⚡ {}", cmd.title)).on_hover_text(&cmd.description).clicked() {
                                    chosen_cmd = Some(cmd.id);
                                    ui.close_menu();
                                }
                            }
                        });
                        if let Some(cmd_id) = chosen_cmd {
                            self.execute_plugin_command(&cmd_id);
                        }

                        // Selector de temas visuales.
                        egui::ComboBox::from_id_source("theme_picker")
                            .selected_text(format!("🎨 {}", self.theme_mode.name()))
                            .show_ui(ui, |ui| {
                                for mode in crate::theme::ThemeMode::ALL {
                                    if ui.selectable_label(self.theme_mode == mode, mode.name()).clicked() {
                                        self.theme_mode = mode;
                                    }
                                }
                            });
                    });
                });
            });

        draw_resize_handles(ctx);

        // UI-2 (v0.2.20): Tier 2 — franja de pestañas y breadcrumb (30 px).
        // Abrir y guardar a la izquierda; breadcrumb del archivo activo a la
        // derecha. Pestaña activa: fondo del canvas, acento inferior de 2 px y
        // nombre en negrita; las inactivas quedan transparentes y en gris.
        egui::TopBottomPanel::top("tab_bar")
            .exact_height(crate::title_bar::TAB_STRIP_HEIGHT)
            .frame(egui::Frame::none().fill(theme.surface_bg))
            .show(ctx, |ui| {
                let strip = ui.max_rect();
                // Separador inferior de 1 px.
                ui.painter().line_segment(
                    [
                        egui::pos2(strip.left(), strip.bottom() - 0.5),
                        egui::pos2(strip.right(), strip.bottom() - 0.5),
                    ],
                    egui::Stroke::new(1.0_f32, theme.border_subtle),
                );

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(6.0);

                    // Selector de archivos (su popup se ancla a este botón).
                    let open_picker_btn = ui.button("📂 Open…");
                    if open_picker_btn.clicked() {
                        self.picker_open = !self.picker_open;
                    }
                    self.picker_button_rect = Some(open_picker_btn.interact_rect);

                    if ui.button("💾 Save (Ctrl+S)").clicked() {
                        self.doc.try_save();
                    }
                    ui.add_space(6.0);

                    let mut switch_to = None;
                    let mut close_target = None;

                    for (idx, tab) in self.doc.tabs.iter().enumerate() {
                        let is_active = idx == self.doc.active_tab;
                        let title = if is_active {
                            if let Some(ref p) = self.doc.current_file {
                                p.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("file")
                                    .to_string()
                            } else if !self.doc.file_path_input.is_empty() {
                                self.doc.file_path_input.clone()
                            } else {
                                "Untitled".to_string()
                            }
                        } else {
                            tab.display_title()
                        };
                        let dirty = is_active && self.doc.dirty;
                        let text_color = if is_active { theme.text } else { theme.text_muted };
                        let tab_rounding = egui::Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 };

                        // Reservamos el slot del hover antes del contenido: se pinta
                        // debajo del texto cuando ya conocemos el rectángulo.
                        let hover_slot = ui.painter().add(egui::Shape::Noop);
                        let tab_frame = egui::Frame::none()
                            .fill(if is_active { theme.bg } else { egui::Color32::TRANSPARENT })
                            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                            .rounding(tab_rounding);

                        let tab_response = tab_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let label = if is_active {
                                    crate::title_bar::faux_bold_label(
                                        ui,
                                        &title,
                                        text_color,
                                        egui::FontId::proportional(14.0),
                                        egui::Sense::click(),
                                    )
                                } else {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&title).color(text_color))
                                            .sense(egui::Sense::click()),
                                    )
                                };
                                if label.clicked() {
                                    switch_to = Some(idx);
                                }
                                if dirty {
                                    ui.label(egui::RichText::new("●").color(theme.accent).size(10.0));
                                }

                                // × gris; pasa a rojo cuando el mouse está encima.
                                let (x_rect, x_response) =
                                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                                let x_color = if x_response.hovered() {
                                    egui::Color32::from_rgb(248, 81, 73)
                                } else {
                                    theme.text_muted
                                };
                                ui.painter().text(
                                    x_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "×",
                                    egui::FontId::proportional(14.0),
                                    x_color,
                                );
                                if x_response.on_hover_text("Close Tab (Ctrl+W)").clicked() {
                                    close_target = Some(idx);
                                }
                            });
                        });

                        let tab_rect = tab_response.response.rect;
                        if is_active {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(tab_rect.left(), tab_rect.bottom() - 2.0),
                                    tab_rect.right_bottom(),
                                ),
                                0.0,
                                theme.accent,
                            );
                        } else if ui.rect_contains_pointer(tab_rect) {
                            ui.painter().set(
                                hover_slot,
                                egui::Shape::rect_filled(tab_rect, tab_rounding, theme.badge_bg),
                            );
                        }
                    }

                    // Nueva pestaña: botón redondo y discreto.
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("+").color(theme.text_muted))
                                .fill(egui::Color32::TRANSPARENT)
                                .rounding(egui::Rounding::same(10.0))
                                .min_size(egui::vec2(22.0, 22.0)),
                        )
                        .on_hover_text("New Tab (Ctrl+T)")
                        .clicked()
                    {
                        self.new_tab();
                    }

                    // Breadcrumb del archivo activo, alineado a la derecha.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.0);
                        let crumb = match &self.doc.current_file {
                            Some(path) => {
                                let segments =
                                    crate::title_bar::breadcrumb_segments(path, &self.doc.browse_dir);
                                if segments.is_empty() {
                                    "Untitled".to_string()
                                } else {
                                    segments.join(" > ")
                                }
                            }
                            None => "Untitled".to_string(),
                        };
                        ui.label(egui::RichText::new(crumb).color(theme.text_muted).size(12.0));
                    });

                    if let Some(idx) = switch_to {
                        self.switch_tab(idx);
                    }
                    if let Some(idx) = close_target {
                        self.try_close_tab(idx);
                    }
                });
            });

        // Dropdown con el listado de carpetas/archivos
        self.show_picker_popup(ctx);

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let modified = if self.doc.dirty { " [+]" } else { "" };
                let path_str = self
                    .doc
                    .current_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(unsaved)".to_string());
                ui.label(format!("{}{}", path_str, modified));
                ui.separator();
                ui.label(format_size(self.doc.view_bytes));
                // S7: posición del cursor, 1-based (convención de
                // editores; cursor_line/cursor_col son índices 0-based
                // dentro de la línea lógica).
                ui.separator();
                ui.label(format!(
                    "Ln {}, Col {}",
                    self.editor_state.cursor_line + 1,
                    self.editor_state.cursor_col + 1
                ));
                ui.separator();
                ui.label(self.current_language().name());
                // S1: el archivo cambió "por afuera" y el buffer no le
                // debe nada (sin cambios propios): ofrecer recarga.
                if self.doc.external_changed && !self.doc.dirty && self.doc.save_progress.is_none() {
                    ui.separator();
                    ui.label(egui::RichText::new("⚠ File changed on disk")
                        .color(egui::Color32::from_rgb(255, 178, 64)));
                    if ui.button("Reload").clicked() {
                        self.reload();
                    }
                }
                if !self.doc.status.is_empty() {
                    ui.separator();
                    ui.label(&self.doc.status);
                }
            });
        });

        // S2: barra de búsqueda justo arriba de la barra de estado.
        self.show_find_bar(ctx);
        // S1 (Rust IDE): panel inferior de ejecución y diagnósticos de Cargo (F7 / F5 / Ctrl+\).
        self.show_cargo_panel(ctx, &theme);

        // Sin márgenes propios: así el área de texto puede ocupar
        // el panel central entero, sin ese borde gris alrededor.
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme.bg))
            .show(ctx, |ui| {
                // Fondo minimalista con el logo, SOLO mientras el
                // buffer está vacío — apenas empezás a escribir,
                // desaparece (queda "detrás" del cursor).
                if self.doc.is_empty_buffer() {
                    self.draw_watermark(ui, ui.max_rect(), &theme);
                }

                // Editor propio (ver editor_widget.rs): dibuja SOLO
                // las líneas visibles en vez de relayoutear el texto
                // entero en cada tecla como hacía egui::TextEdit —
                // eso era lo que volvía lento al editor con archivos
                // grandes (1MB+).
                // S10: la fuente de verdad del zoom es App.font_size
                // (atajos Ctrl+=/- y botones +/- la tocaron mas
                // arriba en este MISMO frame): sincronizo el
                // EditorState justo antes de dibujar, para que el
                // tamaño aplique SIN esperar el frame siguiente.
                if (self.editor_state.font_size - self.font_size).abs() > f32::EPSILON {
                    self.editor_state.set_font_size(self.font_size);
                }
                let search = if self.find_open && !self.find_query.is_empty() && !self.find_matches.is_empty() {
                    Some(crate::editor_widget::SearchHighlight {
                        matches: &self.find_matches,
                        query_len: self.find_query.chars().count(),
                        current_idx: self.find_current,
                    })
                } else {
                    None
                };
                let language = self.current_language();
                let response = crate::editor_widget::show(
                    ui,
                    &mut self.doc.lines,
                    &mut self.editor_state,
                    &theme,
                    language,
                    search,
                );
                // S10: el Ctrl+Scroll zoomea DIRECTO en el EditorState
                // mientras dibuja; escribo el resultado de vuelta en
                // App (fuente de verdad) y aviso en la barra de estado.
                // OJO: este mirror es solo para lo que cambió DENTRO
                // de show() — los botones/atajos ya dejaron App nuevo
                // mas arriba y el state ya lo tomó arriba.
                if (self.editor_state.font_size - self.font_size).abs() > f32::EPSILON {
                    self.font_size = self.editor_state.font_size;
                    self.doc.status = format!("Zoom: {} px", self.editor_state.font_size);
                }
                // S3: corte previo si la acción fue estructural (Enter / Paste).
                if let Some(prev) = response.pre_break_snapshot {
                    self.doc.snapshot_text(&prev);
                }
                if response.changed {
                    self.doc.dirty = true;
                    // Una vez por edición real (no por frame): refresca
                    // el tamaño de la barra de estado.
                    self.doc.recalc_view_bytes();
                    self.update_ast();
                    self.update_lsp();
                    if self.find_open {
                        self.find_update_matches();
                    }
                    if response.force_snapshot {
                        self.doc.snapshot_if_changed();
                    } else {
                        self.doc.chunk_tracker.on_edit(crate::undo::EditAction::Typing, std::time::Instant::now());
                    }
                }
                // Al perder el foco (click afuera, etc.) tomamos un
                // snapshot para el historial de undo/redo.
                if response.lost_focus {
                    self.doc.snapshot_if_changed();
                }
                // S2: Sincronizar estado completo solo si hubo edición real;
                // si no, solo sincronizar metadatos ligeros para no clonar líneas ni undo_stack en cada frame.
                if response.changed {
                    self.sync_active_to_tab();
                } else {
                    self.sync_active_metadata_to_tab();
                }
            });

        // S1: al final, para que el diálogo SIEMPRE pinte por encima
        // de todos los paneles ya dibujados este frame.
        self.show_close_confirm(ctx);
        // S8: confirmación de cierre de pestaña con cambios sin guardar
        self.show_tab_close_confirm(ctx);
        // S4: igual que el confirm, arriba de todo (los paneles
        // dibujados antes no le pisan al campo de entrada).
        self.show_goto_line(ctx);
        // S7: diálogo modal de búsqueda rápida de archivos (Ctrl+P).
        self.show_quick_open(ctx);
        // Diálogo modal para nombrar archivo al guardar si no tiene nombre.
        self.show_save_name_dialog(ctx);
        // Diálogo modal para explorar y gestionar extensiones para la comunidad.
        self.show_extensions_dialog(ctx);
        // S4 (Rust IDE): Quick Fix / Code Actions (Ctrl+.)
        self.show_code_actions_modal(ctx);
        // S4 (Rust IDE): Symbol Rename (F2)
        self.show_rename_dialog(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cutout_dark_background_removes_contours_and_preserves_artwork() {
        let img = image::load_from_memory(LOGO_BYTES).expect("logo.png debe decodificar").to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let color_img = cutout_dark_background(&img);

        assert_eq!(color_img.size, [w, h]);
        assert_eq!(color_img.pixels.len(), w * h);

        // Las cuatro esquinas deben ser completamente transparentes
        assert_eq!(color_img.pixels[0], egui::Color32::TRANSPARENT);
        assert_eq!(color_img.pixels[w - 1], egui::Color32::TRANSPARENT);
        assert_eq!(color_img.pixels[(h - 1) * w], egui::Color32::TRANSPARENT);
        assert_eq!(color_img.pixels[h * w - 1], egui::Color32::TRANSPARENT);

        // Todo el perímetro de la imagen debe ser transparente (sin recuadro)
        for x in 0..w {
            assert_eq!(color_img.pixels[x], egui::Color32::TRANSPARENT, "borde superior en x={x}");
            assert_eq!(color_img.pixels[(h - 1) * w + x], egui::Color32::TRANSPARENT, "borde inferior en x={x}");
        }
        for y in 0..h {
            assert_eq!(color_img.pixels[y * w], egui::Color32::TRANSPARENT, "borde izquierdo en y={y}");
            assert_eq!(color_img.pixels[y * w + (w - 1)], egui::Color32::TRANSPARENT, "borde derecho en y={y}");
        }

        // El arte del centro debe conservar visibilidad y alfa
        let mut visible_pixels = 0usize;
        for p in &color_img.pixels {
            if p.a() > 0 {
                visible_pixels += 1;
            }
        }
        assert!(visible_pixels > 350_000, "el logo debe conservar sus píxeles visibles");
    }

    #[test]
    fn test_load_window_icon_has_transparency() {
        let icon = load_window_icon().expect("el ícono de ventana debe cargarse correctamente");
        assert!(icon.width > 0);
        assert!(icon.height > 0);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);

        // Esquina superior izquierda en RGBA (4 bytes): debe tener alpha = 0
        assert_eq!(icon.rgba[3], 0, "alpha de la esquina superior izquierda debe ser 0");
    }
}
