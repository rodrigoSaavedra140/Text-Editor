use std::path::PathBuf;

use eframe::egui;

use crate::app::EditorApp;
use crate::title_bar::{draw_resize_handles, title_bar_icon_button, TitleBarIcon};
use crate::util::{format_size, request_close};
use crate::LOGO_BYTES;

// Métodos de EditorApp que dibujan cosas con egui. La lógica de
// negocio (abrir/guardar/deshacer) vive en app.rs — Rust permite
// repartir los métodos de un mismo struct en varios `impl` de
// distintos archivos, así que acá van los que necesitan `egui`.
impl EditorApp {
    /// Dibuja la ventana emergente de progreso mientras haya un
    /// guardado en curso, y la cierra sola cuando termina.
    fn show_save_progress(&mut self, ctx: &egui::Context) {
        let Some(progress) = &self.save_progress else {
            return;
        };
        let (written, total, done, error) = {
            let p = progress.lock().unwrap();
            (p.written, p.total, p.done, p.error.clone())
        };

        let fraction = if total == 0 { 1.0 } else { written as f32 / total as f32 };

        egui::Window::new("Guardando…")
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
            self.note_save_done();
            match error {
                Some(e) => self.status = format!("Error al guardar: {}", e),
                None => {
                    self.dirty = false;
                    self.status = "Guardado".to_string();
                }
            }
            self.save_progress = None;
        } else {
            ctx.request_repaint();
        }

        // S1: si el usuario pidió guardar-Y-cerrar desde el diálogo y
        // el guardado terminó bien, recién acá se cierra la ventana.
        if self.close_after_save && self.save_progress.is_none() {
            self.close_after_save = false;
            request_close(ctx, &mut self.closing_since);
        }
    }

    /// S1: ventana de confirmación "¿cerrar con cambios sin
    /// guardar?", visible mientras `close_pending`. Tres caminos:
    /// Guardar (guarda y cierra al terminar), Descartar (cierra sin
    /// guardar) y Cancelar (vuelve al editor sin tocar nada). NADA
    /// mata el proceso mientras el diálogo esté abierto.
    fn show_close_confirm(&mut self, ctx: &egui::Context) {
        if !self.close_pending {
            return;
        }
        // Esc = Cancelar, convención universal de diálogos.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_close();
            return;
        }
        let mut open = true;
        egui::Window::new("Cambios sin guardar")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label("Tenés cambios sin guardar en este documento.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Guardar").clicked() {
                        self.confirm_close_save();
                    }
                    if ui.button("Descartar").clicked() {
                        self.confirm_close_discard();
                        request_close(ctx, &mut self.closing_since);
                    }
                    if ui.button("Cancelar").clicked() {
                        self.cancel_close();
                    }
                });
            });
        if !open {
            // La X del propio diálogo cuenta como Cancelar.
            self.cancel_close();
        }
    }

    /// Cierre de ventana con puertas de datos: si hay cambios sin
    /// guardar (o un guardado en pleno vuelo) NO cierra nada — planta
    /// el diálogo S1 y, si el cierre vino del sistema (X/Alt+F4 del
    /// WM), lo cancela para que la ventana siga viva mientras el
    /// usuario decide. Sin cambios: cierre de toda la vida.
    fn try_close(&mut self, ctx: &egui::Context) {
        if self.dirty || self.save_progress.is_some() {
            self.close_pending = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else {
            request_close(ctx, &mut self.closing_since);
        }
    }

    /// S4: ventana pequeña "Ir a línea" de Ctrl+G. Enter aplica,
    /// Esc cierra; la X del propio diálogo cuenta como cerrar.
    /// Números inválidos no mueven el cursor: solo avisan.
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
        egui::Window::new("Ir a línea")
            // La X del diálogo escribe false directamente en el flag.
            .open(&mut self.goto_pending)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0))
            .show(ctx, |ui| {
                ui.set_min_width(240.0);
                ui.horizontal(|ui| {
                    ui.label("Línea:");
                    let field =
                        ui.add(egui::TextEdit::singleline(&mut self.goto_input).char_limit(12));
                    // El campo pide el foco cada frame del diálogo:
                    // escribir y Enter salen solos.
                    field.request_focus();
                    if ui.button("Ir").clicked() {
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
            match crate::util::parse_goto_line(&self.goto_input, self.lines.len()) {
                Some(idx) => {
                    self.editor_state.cursor_line = idx;
                    self.editor_state.cursor_col = 0;
                    self.editor_state.sel_anchor = Some((idx, 0));
                    self.editor_state.request_autoscroll();
                    self.status = format!("Línea {}", idx + 1);
                }
                None => self.status = "Escribí un número de línea para saltar".to_string(),
            }
        }
    }

    /// Carga el logo como textura de egui, una sola vez (la primera
    /// vez que hace falta dibujarlo).
    fn ensure_logo_texture(&mut self, ctx: &egui::Context) {
        if self.logo_texture.is_some() {
            return;
        }
        if let Ok(img) = image::load_from_memory(LOGO_BYTES) {
            let img = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            self.logo_texture =
                Some(ctx.load_texture("karkinos_logo", color_image, egui::TextureOptions::default()));
        }
    }

    /// Fondo minimalista: SOLO el logo, bien tenue, centrado — se
    /// dibuja mientras el buffer está vacío, y queda "detrás" de
    /// donde vas a empezar a escribir. El nombre "Karkinos" ya se
    /// muestra en la barra de título propia, así que acá no hace
    /// falta repetirlo.
    fn draw_watermark(&self, ui: &egui::Ui, rect: egui::Rect) {
        let painter = ui.painter();
        let center = rect.center();

        if let Some(tex) = &self.logo_texture {
            let logo_side = (rect.height() * 0.32).min(260.0);
            let logo_rect = egui::Rect::from_center_size(center, egui::vec2(logo_side, logo_side));
            painter.image(
                tex.id(),
                logo_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::from_white_alpha(45), // bien tenue, no compite con el texto que escribas
            );
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show_save_progress(ctx);
        // S1: comprobación externa de disco (limitada a 1 stat/seg
        // dentro del método; aquí solo es invocarla por frame).
        self.check_external_changes();

        // Atajos de teclado globales
        let (want_save, want_undo, want_redo, want_quit, want_goto) = ctx.input(|i| {
            (
                i.modifiers.ctrl && i.key_pressed(egui::Key::S),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Z),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Y),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Q),
                i.modifiers.ctrl && i.key_pressed(egui::Key::G),
            )
        });
        if want_save {
            self.save();
        }
        if want_undo {
            self.undo();
        }
        if want_redo {
            self.redo();
        }
        if want_quit {
            self.try_close(ctx);
        }
        // S4: Ctrl+G abre "Ir a línea" — precargado con la línea
        // actual para editar el número en vez de escribirlo de cero.
        if want_goto && !self.close_pending {
            self.goto_input = format!("{}", self.editor_state.cursor_line + 1);
            self.goto_pending = true;
        }

        // Lo mismo si el cierre vino del botón X de la ventana o de
        // Alt+F4 (no solo de nuestros botones/atajos): con cambios
        // sin guardar try_close lo CANCELA y planta el diálogo.
        let x_close_requested = ctx.input(|i| i.viewport().close_requested());
        if x_close_requested {
            self.try_close(ctx);
        }

        self.ensure_logo_texture(ctx);

        // Le sacamos la decoración nativa a la ventana (ver main()),
        // así que esta barra ocupa exactamente el lugar donde antes
        // estaba la barra nativa del sistema operativo — con nuestro
        // logo, "Karkinos", y los mismos botones de siempre.
        egui::TopBottomPanel::top("custom_title_bar")
            .exact_height(32.0)
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(18, 18, 18)))
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
                    ui.label(egui::RichText::new("Karkinos").strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if title_bar_icon_button(ui, TitleBarIcon::Close).clicked() {
                        self.try_close(ctx);
                    }

                        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        let icon = if is_maximized { TitleBarIcon::Restore } else { TitleBarIcon::Maximize };
                        if title_bar_icon_button(ui, icon).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                        }

                        if title_bar_icon_button(ui, TitleBarIcon::Minimize).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });

        draw_resize_handles(ctx);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let file_input_response = ui.add(
                    egui::TextEdit::singleline(&mut self.file_path_input)
                        .hint_text("Ingrese nombre de archivo o nombre de nuevo archivo"),
                );
                // Sin botón "Abrir": escribir el nombre y apretar
                // Enter abre ese archivo directamente.
                if file_input_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.open();
                }

                // Desplegable con navegación de carpetas: subcarpetas
                // para entrar (📁), "⬆ subir" para volver al padre, y
                // archivos de texto que se abren con un click.
                let mut navigate_to: Option<PathBuf> = None;
                let mut open_file: Option<PathBuf> = None;
                egui::ComboBox::from_id_source("file_picker")
                    .selected_text("Abrir…")
                    .show_ui(ui, |ui| {
                        ui.label(format!("📂 {}", self.browse_dir.display()));
                        ui.separator();

                        if let Some(parent) = self.browse_dir.parent() {
                            if ui.selectable_label(false, "⬆ ..").clicked() {
                                navigate_to = Some(parent.to_path_buf());
                            }
                        }

                        // Caché del listado (ver cached_list_dir en
                        // app.rs): sin read_dir ni peek de archivos
                        // por frame con el desplegable abierto.
                        let (dirs, files) = self.cached_list_dir();

                        for name in &dirs {
                            if ui.selectable_label(false, format!("📁 {}", name)).clicked() {
                                navigate_to = Some(self.browse_dir.join(name));
                            }
                        }
                        for name in &files {
                            if ui.selectable_label(false, name).clicked() {
                                open_file = Some(self.browse_dir.join(name));
                            }
                        }
                        if dirs.is_empty() && files.is_empty() {
                            ui.label("(carpeta vacía)");
                        }
                    });
                if let Some(dir) = navigate_to {
                    self.browse_dir = dir;
                }
                if let Some(path) = open_file {
                    self.open_path(path);
                }

                if ui.button("Guardar (Ctrl+S)").clicked() {
                    self.save();
                }
                ui.separator();
                if ui.button("Deshacer (Ctrl+Z)").clicked() {
                    self.undo();
                }
                if ui.button("Rehacer (Ctrl+Y)").clicked() {
                    self.redo();
                }
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let modified = if self.dirty { " [+]" } else { "" };
                let path_str = self
                    .current_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(sin guardar todavía)".to_string());
                ui.label(format!("{}{}", path_str, modified));
                ui.separator();
                ui.label(format_size(self.view_bytes));
                // S7: posición del cursor, 1-based (convención de
                // editores; cursor_line/cursor_col son índices 0-based
                // dentro de la línea lógica).
                ui.separator();
                ui.label(format!(
                    "Ln {}, Col {}",
                    self.editor_state.cursor_line + 1,
                    self.editor_state.cursor_col + 1
                ));
                // S1: el archivo cambió "por afuera" y el buffer no le
                // debe nada (sin cambios propios): ofrecer recarga.
                if self.external_changed && !self.dirty && self.save_progress.is_none() {
                    ui.separator();
                    ui.label(egui::RichText::new("⚠ El archivo cambió en el disco")
                        .color(egui::Color32::from_rgb(255, 178, 64)));
                    if ui.button("Recargar").clicked() {
                        self.reload();
                    }
                }
                if !self.status.is_empty() {
                    ui.separator();
                    ui.label(&self.status);
                }
            });
        });

        // Sin márgenes propios: así el área de texto puede ocupar
        // el panel central entero, sin ese borde gris alrededor.
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.text_area_bg))
            .show(ctx, |ui| {
                // Fondo minimalista con el logo, SOLO mientras el
                // buffer está vacío — apenas empezás a escribir,
                // desaparece (queda "detrás" del cursor).
                if self.is_empty_buffer() {
                    self.draw_watermark(ui, ui.max_rect());
                }

                // Editor propio (ver editor_widget.rs): dibuja SOLO
                // las líneas visibles en vez de relayoutear el texto
                // entero en cada tecla como hacía egui::TextEdit —
                // eso era lo que volvía lento al editor con archivos
                // grandes (1MB+).
                let response = crate::editor_widget::show(
                    ui,
                    &mut self.lines,
                    &mut self.editor_state,
                    self.text_area_bg,
                );
                if response.changed {
                    self.dirty = true;
                    // Una vez por edición real (no por frame): refresca
                    // el tamaño de la barra de estado.
                    self.recalc_view_bytes();
                }
                // Al perder el foco (click afuera, etc.) tomamos un
                // snapshot para el historial de undo/redo.
                if response.lost_focus {
                    self.snapshot_if_changed();
                }
            });

        // S1: al final, para que el diálogo SIEMPRE pinte por encima
        // de todos los paneles ya dibujados este frame.
        self.show_close_confirm(ctx);
        // S4: igual que el confirm, arriba de todo (los paneles
        // dibujados antes no le pisan al campo de entrada).
        self.show_goto_line(ctx);
    }
}