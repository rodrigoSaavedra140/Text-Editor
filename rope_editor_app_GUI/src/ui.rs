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
            match error {
                Some(e) => self.status = format!("Error al guardar: {}", e),
                None => {
                    self.dirty = false;
                    self.status = "Guardado".to_string();
                }
            }
            self.save_progress = None;
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

        // Atajos de teclado globales
        let (want_save, want_undo, want_redo, want_quit) = ctx.input(|i| {
            (
                i.modifiers.ctrl && i.key_pressed(egui::Key::S),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Z),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Y),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Q),
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
            request_close(ctx, &mut self.closing_since);
        }

        // Lo mismo si el cierre vino del botón X de la ventana (no
        // solo de nuestros botones/atajos).
        let x_close_requested = ctx.input(|i| i.viewport().close_requested());
        if x_close_requested {
            request_close(ctx, &mut self.closing_since);
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
                            request_close(ctx, &mut self.closing_since);
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

                        let (dirs, files) = Self::list_dir(&self.browse_dir);

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
                ui.label(format_size(self.text.len()));
                if !self.status.is_empty() {
                    ui.separator();
                    ui.label(&self.status);
                }
            });
        });

        // Sin márgenes propios: así el área de texto puede ocupar
        // el panel central entero, sin ese borde gris alrededor que
        // dejaba antes (por el margen por defecto de CentralPanel +
        // el TextEdit dimensionado solo por su contenido/desired_rows
        // en vez de por el espacio disponible real).
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.text_area_bg))
            .show(ctx, |ui| {
                let available = ui.available_size();

                // Fondo minimalista con el logo y el nombre, SOLO
                // mientras el buffer está vacío — apenas empezás a
                // escribir, desaparece (queda "detrás" del cursor).
                if self.text.is_empty() {
                    self.draw_watermark(ui, ui.max_rect());
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let response = ui.add_sized(
                        available,
                        egui::TextEdit::multiline(&mut self.text)
                            .font(egui::TextStyle::Monospace)
                            .frame(false), // sin el borde/fondo propio del widget
                    );
                    if response.changed() {
                        self.dirty = true;
                    }
                    // Al perder el foco (click afuera, Tab, etc.)
                    // tomamos un snapshot para el historial de undo/redo.
                    if response.lost_focus() {
                        self.snapshot_if_changed();
                    }
                });
            });
    }
}