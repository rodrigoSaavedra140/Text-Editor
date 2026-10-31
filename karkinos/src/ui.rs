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
    /// UI-3 (v0.2.21): velo a pantalla completa mientras hay un modal activo.
    /// Es un área de `Order::Middle` sin interacción, creada antes de la
    /// tarjeta: egui dibuja las áreas en su orden de memoria, así que la
    /// tarjeta queda encima. Una capa sin área caería en un orden no
    /// determinista junto a los paneles de fondo.
    fn paint_modal_backdrop(ctx: &egui::Context) {
        let id = egui::Id::new("modal_backdrop");
        let _ = egui::Area::new(id)
            .order(egui::Order::Middle)
            .interactable(false)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |_ui| {});
        ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, id))
            .rect_filled(ctx.screen_rect(), 0.0, egui::Color32::from_black_alpha(130));
    }

    /// UI-3 (v0.2.21): pastilla de atajo de teclado, p. ej. "ESC to close".
    fn kbd_hint(ui: &mut egui::Ui, text: &str, theme: &crate::theme::Theme) {
        egui::Frame::none()
            .fill(theme.badge_bg)
            .rounding(egui::Rounding::same(4.0))
            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).color(theme.text_muted).size(11.0));
            });
    }

    fn show_code_actions_modal(&mut self, ctx: &egui::Context) {
        if !self.lsp.code_actions_menu_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.lsp.code_actions_menu_open = false;
            return;
        }

        let theme = self.theme_mode.theme();
        let mut applied_action = None;
        let mut close_modal = false;

        // UI-3 (v0.2.21): tarjeta flotante sin barra de título; el título
        // pasa a ser una línea muted dentro de la tarjeta.
        egui::Window::new("code_actions_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .min_width(420.0)
            .max_width(420.0)
            .frame(crate::theme::modal_card_frame(&theme))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("💡 Quick Fix / Code Actions (Ctrl+.)")
                        .color(theme.text_muted)
                        .size(12.0),
                );
                ui.add_space(6.0);

                if self.lsp.code_actions.is_empty() {
                    ui.label("No quick fixes or assists available at cursor.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            close_modal = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            Self::kbd_hint(ui, "ESC to close", &theme);
                        });
                    });
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
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close_modal = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            Self::kbd_hint(ui, "ESC to close", &theme);
                        });
                    });
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

        let theme = self.theme_mode.theme();
        let mut apply = false;
        let mut cancel = false;

        // UI-3 (v0.2.21): tarjeta flotante sin barra de título.
        egui::Window::new("rename_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .min_width(380.0)
            .max_width(380.0)
            .frame(crate::theme::modal_card_frame(&theme))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("✏ Rename Symbol (F2)")
                        .color(theme.text_muted)
                        .size(12.0),
                );
                ui.add_space(4.0);
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
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    // A la derecha; el primero agregado queda más a la derecha.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Self::kbd_hint(ui, "↵ to rename", &theme);
                        Self::kbd_hint(ui, "ESC to cancel", &theme);
                    });
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

    /// Barra lateral de navegación (v0.2.33): explorador, búsqueda global,
    /// extensiones, consola Cargo y debugger. Reemplaza la ausencia de una
    /// barra izquierda que el blueprint de F6 daba por hecha.
    fn show_activity_bar(&mut self, ctx: &egui::Context, theme: &crate::theme::Theme) {
        egui::SidePanel::left("activity_bar")
            .exact_width(44.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme.surface_bg)
                    .inner_margin(egui::Margin::symmetric(4.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let button = |ui: &mut egui::Ui, icon: &str| {
                        ui.add_sized(
                            [36.0, 36.0],
                            egui::Button::new(egui::RichText::new(icon).size(18.0)).frame(false),
                        )
                    };
                    if button(ui, "📁").on_hover_text("Explorer").clicked() {
                        self.toggle_explorer();
                    }
                    if button(ui, "🔎")
                        .on_hover_text("Search in Workspace (Ctrl+Shift+F)")
                        .clicked()
                    {
                        self.open_workspace_search();
                    }
                    if button(ui, "ƒ")
                        .on_hover_text("Document Symbols (Ctrl+Shift+O)")
                        .clicked()
                    {
                        self.open_symbol_picker();
                    }
                    if button(ui, "🧩").on_hover_text("Extensions").clicked() {
                        self.extensions_modal_open = true;
                        self.extensions_tab = 0;
                    }
                    if button(ui, "⚡")
                        .on_hover_text("Cargo Console (Ctrl+`)")
                        .clicked()
                    {
                        self.cargo.panel_open = !self.cargo.panel_open;
                    }
                    if button(ui, "🐞").on_hover_text("Debugger").clicked() {
                        self.cargo.panel_open = true;
                        self.cargo.active_tab = 2;
                    }
                });
            });
    }

    /// Barra lateral: explorador de archivos del proyecto. Lista plana de
    /// archivos (el recorrido ya excluye target/.git/node_modules) con clic
    /// para abrir.
    fn show_explorer_panel(&mut self, ctx: &egui::Context, theme: &crate::theme::Theme) {
        if !self.explorer.open {
            return;
        }
        let mut open_file = None;
        let mut refresh = false;
        let mut close = false;

        egui::SidePanel::left("explorer_panel")
            .resizable(true)
            .default_width(240.0)
            .min_width(180.0)
            .frame(
                egui::Frame::none()
                    .fill(theme.panel_bg)
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📁 Explorer").color(theme.text).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").on_hover_text("Close").clicked() {
                            close = true;
                        }
                        if ui.button("⟳").on_hover_text("Refresh").clicked() {
                            refresh = true;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for path in &self.explorer.files {
                            let relative = path
                                .strip_prefix(&self.doc.browse_dir)
                                .unwrap_or(path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            let row = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(relative)
                                        .color(theme.text_muted)
                                        .size(11.5),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if row.clicked() {
                                open_file = Some(path.clone());
                            }
                        }
                    });
            });

        if close {
            self.explorer.open = false;
        }
        if refresh {
            self.explorer.loaded_for = None;
            self.refresh_explorer();
        }
        if let Some(path) = open_file {
            self.open_path(path);
        }
    }

    /// F7 (Roadmap v0.2.34): modal del selector de símbolos (Ctrl+Shift+O).
    /// Lista filtrable de lo que extrajo el AST, con insignia por tipo.
    fn show_symbol_picker(&mut self, ctx: &egui::Context, theme: &crate::theme::Theme) {
        if !self.symbol_picker.open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.symbol_picker.open = false;
            return;
        }

        let filtered: Vec<karkinos_core::syntax_ast::DocumentSymbol> =
            self.filtered_symbols().into_iter().cloned().collect();
        let count = filtered.len();
        if self.symbol_picker.selected >= count {
            self.symbol_picker.selected = count.saturating_sub(1);
        }

        let mut jump: Option<karkinos_core::syntax_ast::DocumentSymbol> = None;
        let mut close = false;

        egui::Window::new("symbol_picker_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .min_width(520.0)
            .max_width(520.0)
            .frame(crate::theme::modal_card_frame(theme))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("ƒ Document Symbols (Ctrl+Shift+O)")
                        .color(theme.text_muted)
                        .size(12.0),
                );
                ui.add_space(4.0);

                let input = ui.add_sized(
                    [ui.available_width(), 30.0],
                    egui::TextEdit::singleline(&mut self.symbol_picker.query)
                        .id_source("symbol_picker_input")
                        .hint_text("Filter symbols..."),
                );
                input.request_focus();
                if input.changed() {
                    self.symbol_picker.selected = 0;
                }

                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && self.symbol_picker.selected > 0
                {
                    self.symbol_picker.selected -= 1;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown))
                    && self.symbol_picker.selected + 1 < count
                {
                    self.symbol_picker.selected += 1;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && count > 0 {
                    jump = Some(filtered[self.symbol_picker.selected].clone());
                }

                ui.separator();
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    for (idx, symbol) in filtered.iter().enumerate() {
                        let selected = idx == self.symbol_picker.selected;
                        let text = format!(
                            "{} {}  ·  line {}",
                            symbol.kind.label(),
                            symbol.name,
                            symbol.line + 1
                        );
                        let row = ui.selectable_label(
                            selected,
                            egui::RichText::new(text).color(if selected {
                                theme.text
                            } else {
                                theme.text_muted
                            }),
                        );
                        if row.clicked() {
                            jump = Some(symbol.clone());
                        }
                    }
                });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Self::kbd_hint(ui, "↵ to jump", theme);
                        Self::kbd_hint(ui, "ESC to close", theme);
                    });
                });
            });

        if close {
            self.symbol_picker.open = false;
        }
        if let Some(symbol) = jump {
            self.jump_to_symbol(&symbol);
        }
    }

    /// F6 (Roadmap v0.2.32): panel lateral de búsqueda global en el workspace.
    /// Resultados en streaming por archivo, agrupados y colapsables; un clic
    /// abre el archivo y posiciona el cursor en la coincidencia.
    fn show_workspace_search(&mut self, ctx: &egui::Context, theme: &crate::theme::Theme) {
        if !self.workspace_search.open {
            return;
        }

        let mut close_panel = false;
        let mut edited = false;
        let mut trigger = false;
        let mut jump: Option<(std::path::PathBuf, karkinos_core::SearchMatch)> = None;
        let mut toggle: Option<std::path::PathBuf> = None;

        egui::SidePanel::right("workspace_search_panel")
            .resizable(true)
            .default_width(340.0)
            .min_width(240.0)
            .frame(
                egui::Frame::none()
                    .fill(theme.surface_bg)
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("🔎 Search in Workspace")
                            .color(theme.text)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("✕")
                            .on_hover_text("Close (Ctrl+Shift+F)")
                            .clicked()
                        {
                            close_panel = true;
                        }
                    });
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    let input = ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut self.workspace_search.input)
                            .id_source("workspace_search_input")
                            .hint_text("Search... (Enter)"),
                    );
                    if self.workspace_search.focus_requested {
                        input.request_focus();
                        self.workspace_search.focus_requested = false;
                    }
                    if input.changed() {
                        edited = true;
                    }
                    if input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        trigger = true;
                    }
                });

                ui.horizontal(|ui| {
                    if ui
                        .toggle_value(&mut self.workspace_search.case_sensitive, "Aa")
                        .on_hover_text("Case sensitive")
                        .changed()
                    {
                        trigger = true;
                    }
                    if ui
                        .toggle_value(&mut self.workspace_search.match_word, "\\b")
                        .on_hover_text("Whole word")
                        .changed()
                    {
                        trigger = true;
                    }
                    if ui
                        .toggle_value(&mut self.workspace_search.use_regex, ".*")
                        .on_hover_text("Use regex")
                        .changed()
                    {
                        trigger = true;
                    }
                    if ui.button("Search").clicked() {
                        trigger = true;
                    }
                });
                ui.separator();

                let status = if let Some(err) = &self.workspace_search.error {
                    format!("⚠ {err}")
                } else if self.workspace_search.searching {
                    "Searching…".to_string()
                } else {
                    format!(
                        "{} matches in {} files ({} scanned)",
                        self.workspace_search.total_matches,
                        self.workspace_search.results.len(),
                        self.workspace_search.files_scanned
                    )
                };
                ui.label(egui::RichText::new(status).color(theme.text_muted).size(11.0));
                ui.add_space(2.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for file in &self.workspace_search.results {
                        let expanded = !self.workspace_search.collapsed.contains(&file.path);
                        let header = format!(
                            "{} {} ({})",
                            if expanded { "▾" } else { "▸" },
                            file.relative_display,
                            file.matches.len()
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(header).color(theme.text).size(12.0),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            toggle = Some(file.path.clone());
                        }
                        if !expanded {
                            continue;
                        }
                        for m in &file.matches {
                            let text = format!("{:>4}: {}", m.line_number, m.line_content.trim());
                            let row = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text)
                                        .color(theme.text_muted)
                                        .size(11.0)
                                        .monospace(),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if row.clicked() {
                                jump = Some((file.path.clone(), m.clone()));
                            }
                        }
                    }
                });
            });

        if close_panel {
            self.workspace_search.open = false;
        }
        if let Some(path) = toggle {
            if !self.workspace_search.collapsed.remove(&path) {
                self.workspace_search.collapsed.insert(path);
            }
        }
        if let Some((path, m)) = jump {
            self.jump_to_search_match(&path, &m);
        }
        if edited {
            self.workspace_search.last_edit_at = Some(std::time::Instant::now());
        }
        if trigger {
            self.trigger_workspace_search();
        }
    }

    /// F5 (Roadmap v0.2.31): dibuja el editor en uno o dos paneles. Cada panel
    /// tiene su propio cursor y scroll; ambos editan el mismo buffer del tab
    /// activo. Comparar dos pestañas distintas queda fuera de alcance: el
    /// buffer y el undo siguen siendo uno solo.
    fn show_editor_panes(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::theme::Theme,
    ) -> crate::editor_widget::EditorResponse {
        let language = self.current_language();
        let Some(layout) = self.split.clone() else {
            let search = if self.find_open && !self.find_query.is_empty() && !self.find_matches.is_empty() {
                Some(crate::editor_widget::SearchHighlight {
                    matches: &self.find_matches,
                    query_len: self.find_query.chars().count(),
                    current_idx: self.find_current,
                })
            } else {
                None
            };
            return crate::editor_widget::show(
                ui,
                &mut self.doc.lines,
                &mut self.editor_state,
                theme,
                language,
                search,
            );
        };

        const DIVIDER: f32 = 6.0;
        let full = ui.available_rect_before_wrap();
        let direction = layout.direction;
        let (first_rect, divider_rect, second_rect) = match direction {
            crate::app::SplitDirection::Vertical => {
                let usable = (full.width() - DIVIDER).max(0.0);
                let first_w = (usable * layout.ratio).clamp(80.0, (usable - 80.0).max(80.0));
                let x = full.left() + first_w;
                (
                    egui::Rect::from_min_max(full.min, egui::pos2(x, full.bottom())),
                    egui::Rect::from_min_max(
                        egui::pos2(x, full.top()),
                        egui::pos2(x + DIVIDER, full.bottom()),
                    ),
                    egui::Rect::from_min_max(egui::pos2(x + DIVIDER, full.top()), full.max),
                )
            }
            crate::app::SplitDirection::Horizontal => {
                let usable = (full.height() - DIVIDER).max(0.0);
                let first_h = (usable * layout.ratio).clamp(60.0, (usable - 60.0).max(60.0));
                let y = full.top() + first_h;
                (
                    egui::Rect::from_min_max(full.min, egui::pos2(full.right(), y)),
                    egui::Rect::from_min_max(
                        egui::pos2(full.left(), y),
                        egui::pos2(full.right(), y + DIVIDER),
                    ),
                    egui::Rect::from_min_max(egui::pos2(full.left(), y + DIVIDER), full.max),
                )
            }
        };

        let mut first_ui = ui.child_ui(first_rect, *ui.layout());
        let search_first = if self.find_open && !self.find_query.is_empty() && !self.find_matches.is_empty() {
            Some(crate::editor_widget::SearchHighlight {
                matches: &self.find_matches,
                query_len: self.find_query.chars().count(),
                current_idx: self.find_current,
            })
        } else {
            None
        };
        let first = crate::editor_widget::show(
            &mut first_ui,
            &mut self.doc.lines,
            &mut self.editor_state,
            theme,
            language,
            search_first,
        );

        let mut second_ui = ui.child_ui(second_rect, *ui.layout());
        let search_second = if self.find_open && !self.find_query.is_empty() && !self.find_matches.is_empty() {
            Some(crate::editor_widget::SearchHighlight {
                matches: &self.find_matches,
                query_len: self.find_query.chars().count(),
                current_idx: self.find_current,
            })
        } else {
            None
        };
        let second = {
            let split = self.split.as_mut().expect("split activo");
            crate::editor_widget::show(
                &mut second_ui,
                &mut self.doc.lines,
                &mut split.secondary,
                theme,
                language,
                search_second,
            )
        };

        // Divisor arrastrable entre los dos paneles.
        let divider =
            ui.interact(divider_rect, ui.id().with("split_divider"), egui::Sense::click_and_drag());
        if divider.hovered() || divider.dragged() {
            ui.ctx().set_cursor_icon(match direction {
                crate::app::SplitDirection::Vertical => egui::CursorIcon::ResizeHorizontal,
                crate::app::SplitDirection::Horizontal => egui::CursorIcon::ResizeVertical,
            });
        }
        if divider.dragged() {
            if let Some(pos) = divider.interact_pointer_pos() {
                let ratio = match direction {
                    crate::app::SplitDirection::Vertical => {
                        (pos.x - full.left()) / full.width().max(1.0)
                    }
                    crate::app::SplitDirection::Horizontal => {
                        (pos.y - full.top()) / full.height().max(1.0)
                    }
                };
                self.set_split_ratio(ratio);
            }
        }

        crate::editor_widget::EditorResponse {
            changed: first.changed || second.changed,
            lost_focus: first.lost_focus || second.lost_focus,
            pre_break_snapshot: first.pre_break_snapshot.or(second.pre_break_snapshot),
            force_snapshot: first.force_snapshot || second.force_snapshot,
        }
    }

    /// S2: barra de búsqueda y reemplazo debajo del editor (Ctrl+F).
    /// UI-5 (v0.2.23): tarjeta flotante en la esquina superior derecha con sombra y bordes redondeados.
    fn show_find_bar(&mut self, ctx: &egui::Context) {
        if !self.find_open {
            return;
        }
        let theme = self.theme_mode.theme();

        egui::Window::new("find_replace_floating")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20.0, 72.0))
            .frame(
                egui::Frame::none()
                    .fill(theme.elevated_bg)
                    .rounding(egui::Rounding::same(8.0))
                    .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0),
                        blur: 16.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(140),
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🔍").color(theme.text_muted));
                    let find_field = ui.add(
                        egui::TextEdit::singleline(&mut self.find_query)
                            .desired_width(160.0)
                            .hint_text("Find…"),
                    );
                    if find_field.changed() {
                        self.find_update_matches();
                        if !self.find_matches.is_empty() {
                            self.find_goto_current();
                        }
                    }
                    if find_field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.find_next();
                        find_field.request_focus();
                    }

                    let total = self.find_matches.len();
                    let match_str = if total > 0 {
                        format!("{}/{}", self.find_current + 1, total)
                    } else if !self.find_query.is_empty() {
                        "0/0".to_string()
                    } else {
                        "".to_string()
                    };
                    if !match_str.is_empty() {
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new(match_str).color(theme.text_muted).size(11.0),
                            &theme,
                        );
                    }

                    if ui.small_button("◀").on_hover_text("Previous match (Shift+Enter)").clicked() {
                        self.find_prev();
                    }
                    if ui.small_button("▶").on_hover_text("Next match (Enter)").clicked() {
                        self.find_next();
                    }

                    let mut case = self.find_case_sensitive;
                    if ui.checkbox(&mut case, "Aa").on_hover_text("Match case").changed() {
                        self.find_case_sensitive = case;
                        self.find_update_matches();
                        if !self.find_matches.is_empty() {
                            self.find_goto_current();
                        }
                    }

                    if ui.small_button("✕").on_hover_text("Close (Esc)").clicked() {
                        self.find_open = false;
                    }
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("✏").color(theme.text_muted));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.find_replace)
                            .desired_width(160.0)
                            .hint_text("Replace…"),
                    );
                    if ui.small_button("Replace").clicked() {
                        self.find_replace_current();
                    }
                    if ui.small_button("All").clicked() {
                        self.find_replace_all();
                    }
                });
            });
    }

    /// UI-4 (v0.2.22): botón de pestaña segmentada para el dock inferior.
    fn dock_segmented_tab(
        ui: &mut egui::Ui,
        is_active: bool,
        label: impl Into<egui::WidgetText>,
        theme: &crate::theme::Theme,
    ) -> egui::Response {
        let bg = if is_active { theme.elevated_bg } else { egui::Color32::TRANSPARENT };
        let frame = egui::Frame::none()
            .fill(bg)
            .rounding(egui::Rounding::same(4.0))
            .inner_margin(egui::Margin::symmetric(10.0, 4.0));
        let response = frame
            .show(ui, |ui| {
                ui.add(egui::Label::new(label).wrap(false).sense(egui::Sense::click()))
            })
            .inner;
        if is_active {
            let rect = response.rect;
            let accent_rect = egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.max.y + 1.0),
                egui::pos2(rect.max.x, rect.max.y + 3.0),
            );
            ui.painter().rect_filled(accent_rect, 1.0, theme.accent);
        }
        response
    }

    /// S1 (Rust IDE): Panel inferior para ejecución y diagnósticos de Cargo (F7 / F5 / Ctrl+\).
    /// UI-4 (v0.2.22): dock inferior segmentado con diseño moderno, píldoras de estado y logs claros.
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
            .frame(
                egui::Frame::none()
                    .fill(theme.surface_bg)
                    .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    let (status_text, status_color) = if self.cargo.active_tab == 0 {
                        if self.terminal.is_alive() {
                            ("Running", egui::Color32::from_rgb(63, 185, 80))
                        } else {
                            ("Idle", egui::Color32::from_gray(140))
                        }
                    } else {
                        match status {
                            karkinos_core::cargo::CargoStatus::Idle => ("Idle", egui::Color32::from_gray(140)),
                            karkinos_core::cargo::CargoStatus::Running => ("Running…", egui::Color32::from_rgb(255, 190, 60)),
                            karkinos_core::cargo::CargoStatus::Success => ("Success", egui::Color32::from_rgb(63, 185, 80)),
                            karkinos_core::cargo::CargoStatus::Failed(_code) => {
                                ("Failed", egui::Color32::from_rgb(248, 81, 73))
                            }
                            karkinos_core::cargo::CargoStatus::Killed => ("Terminated", egui::Color32::from_gray(160)),
                        }
                    };

                    let pill_prefix = if self.cargo.active_tab == 0 { "💻" } else { "🦀" };
                    crate::title_bar::status_pill(
                        ui,
                        egui::RichText::new(format!("{} {}", pill_prefix, status_text))
                            .color(status_color)
                            .strong()
                            .size(11.0),
                        theme,
                    );

                    if let Some(action) = self.cargo.runner.current_action {
                        ui.label(
                            egui::RichText::new(action.display_name())
                                .italics()
                                .color(theme.text_muted)
                                .size(11.0),
                        );
                    } else if let Some(ref cmd) = self.cargo.runner.current_command_line {
                        ui.label(
                            egui::RichText::new(cmd)
                                .italics()
                                .color(theme.text_muted)
                                .size(11.0),
                        );
                    }

                    ui.separator();

                    let term_tab_label = format!("💻 {}", self.terminal.shell_name());
                    if Self::dock_segmented_tab(
                        ui,
                        self.cargo.active_tab == 0,
                        egui::RichText::new(term_tab_label)
                            .color(if self.cargo.active_tab == 0 { theme.text } else { theme.text_muted })
                            .size(12.0),
                        theme,
                    )
                    .clicked()
                    {
                        self.cargo.active_tab = 0;
                    }

                    let diag_tab_label = format!("🔍 Diagnostics ({} err, {} warn)", errors_count, warnings_count);
                    let diag_color = if errors_count > 0 {
                        egui::Color32::from_rgb(248, 81, 73)
                    } else if warnings_count > 0 {
                        egui::Color32::from_rgb(227, 179, 65)
                    } else if self.cargo.active_tab == 1 {
                        theme.text
                    } else {
                        theme.text_muted
                    };

                    if Self::dock_segmented_tab(
                        ui,
                        self.cargo.active_tab == 1,
                        egui::RichText::new(diag_tab_label).color(diag_color).size(12.0),
                        theme,
                    )
                    .clicked()
                    {
                        self.cargo.active_tab = 1;
                    }

                    let dap_label = if self.dap.client.as_ref().map(|c| c.state().is_stopped).unwrap_or(false) {
                        "🐞 Debugger (STOPPED)"
                    } else if self.dap.client.is_some() {
                        "🐞 Debugger (Running)"
                    } else {
                        "🐞 Debugger"
                    };
                    if Self::dock_segmented_tab(
                        ui,
                        self.cargo.active_tab == 2,
                        egui::RichText::new(dap_label)
                            .color(if self.cargo.active_tab == 2 { theme.text } else { theme.text_muted })
                            .size(12.0),
                        theme,
                    )
                    .clicked()
                    {
                        self.cargo.active_tab = 2;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("▾ Collapse")
                            .on_hover_text("Close Console (Ctrl+`)")
                            .clicked()
                        {
                            close_panel = true;
                        }
                        if ui.small_button("🗑 Clear").on_hover_text("Clear console (Ctrl+L)").clicked() {
                            if self.cargo.active_tab == 0 {
                                self.terminal.clear();
                            } else {
                                self.cargo.runner.clear();
                            }
                        }
                        if self.cargo.active_tab == 0 {
                            if ui.small_button("🔄 Restart").on_hover_text("Restart interactive terminal session").clicked() {
                                let cwd = self.current_working_dir();
                                self.terminal.restart(&cwd);
                            }
                        } else if self.cargo.runner.is_running() {
                            if ui.small_button("⏹ Stop").clicked() {
                                self.cargo.runner.kill();
                            }
                        }
                    });
                });

                ui.separator();

                if self.cargo.active_tab == 0 {
                    self.show_interactive_terminal(ui, ctx, theme);
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

    /// S5 (Roadmap v0.2.25): Renderiza el emulador de terminal interactiva PTY (vt100 + portable-pty).
    fn show_interactive_terminal(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &crate::theme::Theme) {
        // OPT-1 (v0.2.26): nada de repintado a 30 ms por frame. La terminal
        // sólo se repinta rápido mientras llega salida del PTY (la revisión
        // avanza); en reposo baja a 150 ms para el cursor, y sin sesión viva
        // no se pide repintado periódico.

        let cwd = self.current_working_dir();
        let font_id = egui::FontId::monospace(12.0);
        let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'M')).max(6.0);
        let line_height = ui.fonts(|f| f.row_height(&font_id)).max(14.0);

        let available_size = ui.available_size();
        let cols = ((available_size.x - 16.0) / char_width).floor().max(20.0) as u16;
        let rows = ((available_size.y - 12.0) / line_height).floor().max(5.0) as u16;

        self.terminal.ensure_running(&cwd, cols, rows);
        let revision_before = self.terminal.revision();

        egui::Frame::none()
            .fill(theme.bg)
            .inner_margin(egui::Margin::same(4.0))
            .show(ui, |ui| {
                let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let painter = ui.painter_at(rect);

                if resp.clicked() {
                    resp.request_focus();
                }

                // Si el área de la terminal tiene foco o está hovered, procesa eventos de teclado
                if resp.has_focus() || resp.hovered() {
                    let events = ui.input(|i| i.events.clone());
                    for event in events {
                        match event {
                            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                                if self.terminal.handle_key_event(key, &modifiers) {
                                    ctx.request_repaint();
                                }
                            }
                            egui::Event::Text(ref text) => {
                                let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                                if !ctrl && !text.is_empty() {
                                    let _ = self.terminal.send_str(text);
                                    ctx.request_repaint();
                                }
                            }
                            egui::Event::Paste(ref text) => {
                                let _ = self.terminal.send_str(text);
                                ctx.request_repaint();
                            }
                            _ => {}
                        }
                    }
                }

                // Renderiza la cuadrícula 2D del emulador VT100
                if let Some(snap) = self.terminal.snapshot() {
                    let origin_x = rect.min.x + 6.0;
                    let origin_y = rect.min.y + 4.0;

                    for (row_idx, row_cells) in snap.grid.iter().enumerate() {
                        if row_idx as f32 * line_height > rect.height() {
                            break;
                        }
                        let y = origin_y + row_idx as f32 * line_height;

                        // 1. Fondos de celda no predeterminados
                        for (col_idx, cell) in row_cells.iter().enumerate() {
                            if col_idx as f32 * char_width > rect.width() {
                                break;
                            }
                            if cell.bg != karkinos_core::terminal::TerminalColor::Default {
                                let cell_rect = egui::Rect::from_min_size(
                                    egui::pos2(origin_x + col_idx as f32 * char_width, y),
                                    egui::vec2(char_width, line_height),
                                );
                                painter.rect_filled(cell_rect, 0.0, Self::terminal_color_to_egui(cell.bg, false, theme));
                            }
                        }

                        // 2. Caracteres y estilos agrupados en corridas para máximo rendimiento
                        let mut run_text = String::new();
                        let mut run_start_col = 0;
                        let mut current_fg = karkinos_core::terminal::TerminalColor::Default;
                        let mut current_bold = false;

                        for (col_idx, cell) in row_cells.iter().enumerate() {
                            if col_idx as f32 * char_width > rect.width() {
                                break;
                            }
                            let fg = cell.fg;
                            let bold = cell.bold;
                            if fg != current_fg || bold != current_bold {
                                if !run_text.is_empty() {
                                    let x = origin_x + run_start_col as f32 * char_width;
                                    let color = Self::terminal_color_to_egui(current_fg, current_bold, theme);
                                    painter.text(egui::pos2(x, y), egui::Align2::LEFT_TOP, &run_text, font_id.clone(), color);
                                    run_text.clear();
                                }
                                current_fg = fg;
                                current_bold = bold;
                                run_start_col = col_idx;
                            }
                            run_text.push_str(if cell.contents.is_empty() { " " } else { &cell.contents });
                        }
                        if !run_text.is_empty() {
                            let x = origin_x + run_start_col as f32 * char_width;
                            let color = Self::terminal_color_to_egui(current_fg, current_bold, theme);
                            painter.text(egui::pos2(x, y), egui::Align2::LEFT_TOP, &run_text, font_id.clone(), color);
                        }

                        // 3. Cursor del emulador
                        if row_idx as u16 == snap.cursor_pos.0 && !snap.hide_cursor {
                            let cursor_col = snap.cursor_pos.1 as usize;
                            let cursor_rect = egui::Rect::from_min_size(
                                egui::pos2(origin_x + cursor_col as f32 * char_width, y),
                                egui::vec2(char_width, line_height),
                            );
                            painter.rect_filled(cursor_rect, 1.0, theme.accent.gamma_multiply(0.7));
                        }
                    }
                }
            });

        // OPT-1: repintado proporcional a la actividad real del PTY.
        if self.terminal.is_alive() {
            let delay_ms = if self.terminal.revision() != revision_before { 30 } else { 150 };
            ctx.request_repaint_after(std::time::Duration::from_millis(delay_ms));
        }
    }

    /// Convierte colores de celda de terminal a `egui::Color32`.
    fn terminal_color_to_egui(
        color: karkinos_core::terminal::TerminalColor,
        bold: bool,
        theme: &crate::theme::Theme,
    ) -> egui::Color32 {
        match color {
            karkinos_core::terminal::TerminalColor::Default => {
                if bold {
                    egui::Color32::WHITE
                } else {
                    theme.text
                }
            }
            karkinos_core::terminal::TerminalColor::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
            karkinos_core::terminal::TerminalColor::Idx(idx) => match idx {
                0 => egui::Color32::from_rgb(0, 0, 0),
                1 => egui::Color32::from_rgb(205, 49, 49),
                2 => egui::Color32::from_rgb(13, 188, 121),
                3 => egui::Color32::from_rgb(229, 229, 16),
                4 => egui::Color32::from_rgb(36, 114, 200),
                5 => egui::Color32::from_rgb(188, 63, 188),
                6 => egui::Color32::from_rgb(17, 168, 205),
                7 => egui::Color32::from_rgb(229, 229, 229),
                8 => egui::Color32::from_rgb(102, 102, 102),
                9 => egui::Color32::from_rgb(241, 76, 76),
                10 => egui::Color32::from_rgb(35, 209, 139),
                11 => egui::Color32::from_rgb(245, 245, 67),
                12 => egui::Color32::from_rgb(59, 142, 234),
                13 => egui::Color32::from_rgb(214, 112, 214),
                14 => egui::Color32::from_rgb(41, 184, 219),
                15 => egui::Color32::from_rgb(255, 255, 255),
                16..=231 => {
                    let i = idx - 16;
                    let r = (i / 36) * 51;
                    let g = ((i % 36) / 6) * 51;
                    let b = (i % 6) * 51;
                    egui::Color32::from_rgb(r, g, b)
                }
                232..=255 => {
                    let v = (idx - 232) * 10 + 8;
                    egui::Color32::from_rgb(v, v, v)
                }
            },
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

        let theme = self.theme_mode.theme();
        let mut is_open = true;
        let mut query_changed = false;
        let mut open_selected = false;

        // UI-3 (v0.2.21): paleta flotante sin barra de título, de 560 px y
        // anclada arriba al centro. Entrada de 36 px con 🔍 y pastillas de atajo.
        egui::Window::new("quick_open_modal")
            .open(&mut is_open)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .min_width(560.0)
            .max_width(560.0)
            .frame(crate::theme::modal_card_frame(&theme))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🔍").color(theme.text_muted));
                    let input_resp = ui.add_sized(
                        [ui.available_width() - 190.0, 36.0],
                        egui::TextEdit::singleline(&mut self.quick_open.query)
                            .hint_text("Search files... (↑/↓ navigate)"),
                    );
                    if self.quick_open.request_focus {
                        input_resp.request_focus();
                        self.quick_open.request_focus = false;
                    }
                    if input_resp.changed() {
                        query_changed = true;
                    }
                    // A la derecha; el primero agregado queda más a la derecha.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Self::kbd_hint(ui, "↵ to select", &theme);
                        Self::kbd_hint(ui, "ESC to close", &theme);
                    });
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

                        // Fila de 32 px: fondo redondeado al seleccionar o al pasar el mouse.
                        let (rect, row) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 32.0),
                            egui::Sense::click(),
                        );
                        if is_selected {
                            ui.painter().rect_filled(rect, egui::Rounding::same(6.0), theme.surface_bg);
                        } else if row.hovered() {
                            ui.painter().rect_filled(rect, egui::Rounding::same(6.0), theme.badge_bg);
                        }
                        let name_rect = ui.painter().text(
                            egui::pos2(rect.left() + 10.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            file_name,
                            egui::FontId::proportional(14.0),
                            theme.text,
                        );
                        if !parent.is_empty() {
                            ui.painter().text(
                                egui::pos2(name_rect.right() + 10.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                parent,
                                egui::FontId::proportional(12.0),
                                theme.text_muted,
                            );
                        }
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
        // F1 (Roadmap v0.2.27): marcas de diff contra HEAD — el worker corre
        // `git` en fondo; acá solo se pide al abrir/guardar y se aplica lo
        // que llegó.
        self.request_git_diff_if_needed();
        if self.poll_git_diff_results() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        // F8 (Roadmap v0.2.35): rama + blame inline (consulta perezosa).
        self.refresh_git_branch();
        if self.poll_blame_results() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.update_inline_blame();
        if self.editor_state.blame_text.is_none() && self.doc.current_file.is_some() {
            // Despertar para cumplir el dwell de 300 ms del blame.
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        // F6 (Roadmap v0.2.32): debounce de 250 ms + streaming del worker.
        if self.workspace_search.open {
            if let Some(at) = self.workspace_search.last_edit_at {
                if at.elapsed() >= std::time::Duration::from_millis(250) {
                    self.workspace_search.last_edit_at = None;
                    self.trigger_workspace_search();
                }
            }
            if self.poll_workspace_search() {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            if self.workspace_search.searching {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
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
                // F5: la consola pasa a Ctrl+`; Ctrl+\ / Ctrl+Shift+\ son split.
                i.modifiers.ctrl && i.key_pressed(egui::Key::Backtick),
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
        // F5 (Roadmap v0.2.31): Ctrl+\ parte vertical, Ctrl+Shift+\ horizontal.
        {
            let (split_vertical, split_horizontal) = ctx.input(|i| {
                (
                    i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Backslash),
                    i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Backslash),
                )
            });
            if split_vertical {
                self.toggle_split(crate::app::SplitDirection::Vertical);
            } else if split_horizontal {
                self.toggle_split(crate::app::SplitDirection::Horizontal);
            }
        }
        // F6 (Roadmap v0.2.32): Ctrl+Shift+F abre la búsqueda global.
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::F)) {
            self.open_workspace_search();
        }
        // F7 (Roadmap v0.2.34): Ctrl+Shift+O abre el selector de símbolos.
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O)) {
            self.open_symbol_picker();
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
                    // F6 (Roadmap v0.2.32): búsqueda global del workspace.
                    if ui
                        .button("🔎")
                        .on_hover_text("Search in Workspace (Ctrl+Shift+F)")
                        .clicked()
                    {
                        self.open_workspace_search();
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
                        if ui.button(panel_toggle_label).on_hover_text("Toggle Cargo Console (Ctrl+`)").clicked() {
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

        // UI-4 (v0.2.22): barra de estado refinada de 22 px con micro-píldoras e indicadores claros.
        egui::TopBottomPanel::bottom("status")
            .exact_height(crate::title_bar::STATUS_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme.surface_bg)
                    .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_space(4.0);

                    // Indicador de guardado / dirty
                    if self.doc.dirty {
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new("Modified ●").color(theme.accent).size(11.0),
                            &theme,
                        );
                    } else {
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new("Saved").color(egui::Color32::from_rgb(63, 185, 80)).size(11.0),
                            &theme,
                        );
                    }

                    // F8 (Roadmap v0.2.35): rama actual del repo.
                    if let Some(branch) = &self.git_status.branch {
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new(format!("⎇ {branch}"))
                                .color(theme.text_muted)
                                .size(11.0),
                            &theme,
                        );
                    }

                    let path_str = self
                        .doc
                        .current_file
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(unsaved)".to_string());
                    crate::title_bar::status_pill(
                        ui,
                        egui::RichText::new(&path_str).color(theme.text).size(11.0),
                        &theme,
                    );

                    crate::title_bar::status_pill(
                        ui,
                        egui::RichText::new(format!(
                            "Ln {}, Col {}",
                            self.editor_state.cursor_line + 1,
                            self.editor_state.cursor_col + 1
                        ))
                        .color(theme.text_muted)
                        .size(11.0),
                        &theme,
                    );

                    crate::title_bar::status_pill(
                        ui,
                        egui::RichText::new(format_size(self.doc.view_bytes))
                            .color(theme.text_muted)
                            .size(11.0),
                        &theme,
                    );

                    if self.doc.external_changed && !self.doc.dirty && self.doc.save_progress.is_none() {
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new("⚠ Changed on disk")
                                .color(egui::Color32::from_rgb(255, 178, 64))
                                .size(11.0),
                            &theme,
                        );
                        if ui.small_button("Reload").clicked() {
                            self.reload();
                        }
                    }

                    if !self.doc.status.is_empty() {
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new(&self.doc.status).color(theme.text).size(11.0),
                            &theme,
                        );
                    }

                    // Grupo alineado a la derecha
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);

                        // Encoding
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new("UTF-8").color(theme.text_muted).size(11.0),
                            &theme,
                        );

                        // Line Ending
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new(self.doc.line_ending.as_str()).color(theme.text_muted).size(11.0),
                            &theme,
                        );

                        // Language badge
                        let lang = self.current_language();
                        let lang_badge = match lang {
                            crate::syntax::Language::Rust => "🦀 Rust 2021".to_string(),
                            crate::syntax::Language::JavaScript => format!("JS {}", lang.name()),
                            crate::syntax::Language::Html => format!("🌐 {}", lang.name()),
                            crate::syntax::Language::Css => format!("🎨 {}", lang.name()),
                            crate::syntax::Language::Java => format!("☕ {}", lang.name()),
                            crate::syntax::Language::Json => format!("{{ }} {}", lang.name()),
                            crate::syntax::Language::Markdown => format!("📝 {}", lang.name()),
                            crate::syntax::Language::Sql => format!("🗄 {}", lang.name()),
                            _ => format!("📄 {}", lang.name()),
                        };
                        crate::title_bar::status_pill(
                            ui,
                            egui::RichText::new(lang_badge).color(theme.text).size(11.0),
                            &theme,
                        );

                        // LSP Status pill
                        if lang == crate::syntax::Language::Rust {
                            let (lsp_text, lsp_color) = if self.lsp.client.is_some() {
                                ("● LSP Ready", egui::Color32::from_rgb(63, 185, 80))
                            } else {
                                ("○ LSP Off", theme.text_muted)
                            };
                            crate::title_bar::status_pill(
                                ui,
                                egui::RichText::new(lsp_text).color(lsp_color).size(11.0),
                                &theme,
                            );
                        }
                    });
                });
            });

        // S2: barra de búsqueda justo arriba de la barra de estado.
        self.show_find_bar(ctx);
        // S1 (Rust IDE): panel inferior de ejecución y diagnósticos de Cargo (F7 / F5 / Ctrl+`).
        self.show_cargo_panel(ctx, &theme);
        // Barra lateral (v0.2.33): navegación y explorador antes del área central.
        self.show_activity_bar(ctx, &theme);
        self.show_explorer_panel(ctx, &theme);
        // F6 (Roadmap v0.2.32): panel lateral de búsqueda global.
        self.show_workspace_search(ctx, &theme);

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
                    if let Some(split) = self.split.as_mut() {
                        split.secondary.set_font_size(self.font_size);
                    }
                }
                // F5 (Roadmap v0.2.31): uno o dos paneles, con su propio
                // cursor y scroll sobre el mismo buffer.
                let response = self.show_editor_panes(ui, &theme);
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
        // UI-3 (v0.2.21): velo de fondo mientras haya un modal de búsqueda, acciones o rename.
        if self.quick_open.open || self.lsp.code_actions_menu_open || self.lsp.rename_dialog_open {
            Self::paint_modal_backdrop(ctx);
        }
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
        // F7 (Roadmap v0.2.34): selector de símbolos del documento.
        self.show_symbol_picker(ctx, &theme);
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
