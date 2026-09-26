mod rope;
mod undo;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use rope::Rope;
use undo::{Edit, EditKind, UndoStack};

/// Logo de Karkinos, embebido en el binario en tiempo de compilación
/// (no se lee de disco en tiempo de ejecución — así el ícono y el
/// fondo minimalista funcionan siempre, sin depender de que el
/// archivo esté presente donde sea que se ejecute el programa).
const LOGO_BYTES: &[u8] = include_bytes!("../assets/logo.png");

/// Estado compartido entre el hilo que escribe el archivo y la UI,
/// que solo lee esto para dibujar la barra de progreso.
struct SaveProgress {
    written: usize,
    total: usize,
    done: bool,
    error: Option<String>,
}

struct EditorApp {
    text: String,          // buffer "vivo" que edita el widget de egui
    last_snapshot: String, // texto en el último snapshot guardado (para detectar cambios)
    undo_stack: UndoStack,
    file_path_input: String,
    current_file: Option<PathBuf>,
    dirty: bool,
    status: String,
    // Carpeta que está mostrando el desplegable de archivos ahora
    // mismo — no tiene por qué ser la misma que la carpeta desde
    // donde se lanzó el programa, porque se puede navegar.
    browse_dir: PathBuf,
    // Color de fondo del área de texto (el mismo negro que ya tenías).
    text_area_bg: egui::Color32,
    // Textura del logo, cargada una sola vez en el primer frame
    // (None hasta ese momento).
    logo_texture: Option<egui::TextureHandle>,
    // Some() mientras hay un guardado en curso en otro hilo.
    save_progress: Option<Arc<Mutex<SaveProgress>>>,
    // Momento en que se pidió el cierre prolijo — si pasa mucho
    // tiempo sin que la ventana realmente se cierre (WSLg no
    // responde bien a veces), forzamos la salida igual.
    closing_since: Option<Instant>,
}

impl Default for EditorApp {
    fn default() -> Self {
        EditorApp {
            text: String::new(),
            last_snapshot: String::new(),
            undo_stack: UndoStack::new(),
            file_path_input: String::new(),
            current_file: None,
            dirty: false,
            status: "Listo".to_string(),
            browse_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            text_area_bg: egui::Color32::from_rgb(10, 10, 10),
            logo_texture: None,
            save_progress: None,
            closing_since: None,
        }
    }
}

impl EditorApp {
    /// Si el texto cambió desde el último snapshot, guarda un Edit
    /// (Rope antes / Rope después) en el UndoStack.
    fn snapshot_if_changed(&mut self) {
        if self.text != self.last_snapshot {
            let before = Rope::new(&self.last_snapshot);
            let after = Rope::new(&self.text);
            self.undo_stack.push(Edit { kind: EditKind::Replace, position: 0, before, after });
            self.last_snapshot = self.text.clone();
            self.dirty = true;
        }
    }

    fn undo(&mut self) {
        self.snapshot_if_changed();
        if let Some(edit) = self.undo_stack.undo() {
            self.text = edit.before.to_string();
            self.last_snapshot = self.text.clone();
            self.status = "Deshecho".to_string();
        } else {
            self.status = "Nada para deshacer".to_string();
        }
    }

    fn redo(&mut self) {
        if let Some(edit) = self.undo_stack.redo() {
            self.text = edit.after.to_string();
            self.last_snapshot = self.text.clone();
            self.status = "Rehecho".to_string();
        } else {
            self.status = "Nada para rehacer".to_string();
        }
    }

    /// Abre `path` directamente (usado por el explorador de
    /// carpetas). Después de abrir, el campo "Archivo" solo muestra
    /// el nombre — la ruta completa se ve en la barra de abajo.
    fn open_path(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(content) => {
                self.text = content.clone();
                self.last_snapshot = content;
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    self.file_path_input = name.to_string();
                }
                self.current_file = Some(path);
                self.dirty = false;
                self.status = "Archivo abierto".to_string();
            }
            Err(e) => {
                self.status = format!("Error al abrir: {}", e);
            }
        }
    }

    /// Abre lo que haya escrito en el campo "Archivo" (botón "Abrir"
    /// o Enter). Interpreta ese texto como ruta relativa a la carpeta
    /// desde donde se lanzó el programa.
    fn open(&mut self) {
        self.open_path(PathBuf::from(&self.file_path_input));
    }

    /// Heurística simple para distinguir texto de binario (la misma
    /// que usa Git): si los primeros bytes del archivo no tienen
    /// ningún byte nulo, lo tratamos como texto. No depende de la
    /// extensión — agarra .md, .rs, .log, archivos sin extensión,
    /// etc., y deja afuera binarios de verdad (imágenes, ejecutables).
    fn is_probably_text(path: &std::path::Path) -> bool {
        use std::io::Read;
        if let Ok(mut file) = std::fs::File::open(path) {
            let mut buf = [0u8; 8192];
            if let Ok(n) = file.read(&mut buf) {
                return !buf[..n].contains(&0u8);
            }
        }
        false
    }

    /// Lista el contenido de `dir`: subcarpetas y archivos de texto,
    /// cada lista ordenada alfabéticamente por separado (subcarpetas
    /// primero al mostrarlas, como cualquier explorador de archivos).
    fn list_dir(dir: &std::path::Path) -> (Vec<String>, Vec<String>) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if path.is_dir() {
                    dirs.push(name);
                } else if path.is_file() && Self::is_probably_text(&path) {
                    files.push(name);
                }
            }
        }
        dirs.sort();
        files.sort();
        (dirs, files)
    }

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

    fn save(&mut self) {
        if self.save_progress.is_some() {
            return; // ya hay un guardado en curso, no arranquemos otro encima
        }

        self.snapshot_if_changed();
        let path = if let Some(p) = &self.current_file {
            p.clone()
        } else {
            PathBuf::from(&self.file_path_input)
        };
        if path.as_os_str().is_empty() {
            self.status = "Escribí una ruta de archivo antes de guardar".to_string();
            return;
        }

        let data = self.text.clone().into_bytes();
        let total = data.len();
        let progress = Arc::new(Mutex::new(SaveProgress { written: 0, total, done: false, error: None }));
        self.save_progress = Some(Arc::clone(&progress));
        self.current_file = Some(path.clone());

        // Escribimos en un hilo aparte, en pedacitos, reportando
        // cuánto se lleva escrito — así la ventana de progreso tiene
        // algo real que mostrar (y no bloqueamos la interfaz
        // mientras se guarda un archivo grande o un disco lento).
        thread::spawn(move || {
            let result = (|| -> std::io::Result<()> {
                use std::io::Write;
                let mut file = std::fs::File::create(&path)?;
                // Repartimos en ~20 pasos parejos (mínimo 1 byte por
                // paso) para que la barra se vea animarse incluso en
                // archivos chicos, en vez de completarse de golpe.
                let steps = 20usize;
                let chunk_size = (total / steps).max(1);
                let mut written = 0usize;
                while written < total {
                    let end = (written + chunk_size).min(total);
                    file.write_all(&data[written..end])?;
                    written = end;
                    if let Ok(mut p) = progress.lock() {
                        p.written = written;
                    }
                    thread::sleep(Duration::from_millis(15));
                }
                file.flush()
            })();

            if let Ok(mut p) = progress.lock() {
                p.done = true;
                if let Err(e) = result {
                    p.error = Some(e.to_string());
                }
            }
        });
    }
}

/// Tamaño legible del texto actual en el buffer (bytes UTF-8) — es el
/// tamaño "en vivo", incluye cambios sin guardar todavía, no el
/// tamaño del archivo en disco.
fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} bytes", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Pide el cierre prolijo de la ventana Y, en paralelo, lanza un hilo
/// del sistema operativo que va a matar el proceso a la fuerza al
/// segundo — sin importar si egui sigue llamando a update() o no
/// después de pedir el cierre (WSLg a veces deja de mandar frames en
/// cuanto la ventana empieza a cerrarse, lo que dejaba nuestra
/// salvaguarda anterior sin poder ejecutarse nunca).
fn request_close(ctx: &egui::Context, closing_since: &mut Option<Instant>) {
    if closing_since.is_some() {
        return; // ya se pidió el cierre, no lancemos un hilo por cada frame
    }
    *closing_since = Some(Instant::now());
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(1));
        std::process::exit(0);
    });
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

enum TitleBarIcon {
    Close,
    Minimize,
    Maximize,
    Restore,
}

/// Dibuja un botón de la barra de título con un ícono hecho a mano
/// (líneas/rectángulos), en vez de un emoji — los emojis dependen de
/// qué fuente tenga el sistema, y bajo WSLg a veces se ven mal o
/// distinto a como se ven en Windows de verdad.
fn title_bar_icon_button(ui: &mut egui::Ui, icon: TitleBarIcon) -> egui::Response {
    let size = egui::vec2(32.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if response.hovered() {
        let bg = if matches!(icon, TitleBarIcon::Close) {
            egui::Color32::from_rgb(196, 43, 28) // hover rojo, como en Windows
        } else {
            egui::Color32::from_white_alpha(25)
        };
        ui.painter().rect_filled(rect, 0.0, bg);
    }

    let stroke = egui::Stroke::new(1.2, egui::Color32::from_gray(225));
    let center = rect.center();
    let painter = ui.painter();

    match icon {
        TitleBarIcon::Close => {
            let h = 4.5;
            painter.line_segment([center + egui::vec2(-h, -h), center + egui::vec2(h, h)], stroke);
            painter.line_segment([center + egui::vec2(-h, h), center + egui::vec2(h, -h)], stroke);
        }
        TitleBarIcon::Minimize => {
            painter.line_segment(
                [center + egui::vec2(-5.0, 5.0), center + egui::vec2(5.0, 5.0)],
                stroke,
            );
        }
        TitleBarIcon::Maximize => {
            let square = egui::Rect::from_center_size(center, egui::vec2(9.0, 9.0));
            painter.rect_stroke(square, 0.0, stroke);
        }
        TitleBarIcon::Restore => {
            // El ícono clásico de "restaurar": dos ventanas superpuestas.
            let back = egui::Rect::from_center_size(center + egui::vec2(2.0, -2.0), egui::vec2(8.0, 8.0));
            let front = egui::Rect::from_center_size(center + egui::vec2(-1.5, 1.5), egui::vec2(8.0, 8.0));
            painter.rect_stroke(back, 0.0, stroke);
            // Tapamos la parte de atrás que "taparía" la ventana de
            // adelante, para que se vea como dos ventanas de verdad
            // superpuestas y no como dos cuadrados cruzados.
            painter.rect_filled(front, 0.0, egui::Color32::from_rgb(18, 18, 18));
            painter.rect_stroke(front, 0.0, stroke);
        }
    }

    response
}

/// Bordes invisibles en los 4 lados + 4 esquinas de la ventana, para
/// poder seguir redimensionando arrastrando el borde con el mouse
/// ahora que le sacamos la decoración nativa (que era la que hacía
/// esto automáticamente antes).
fn draw_resize_handles(ctx: &egui::Context) {
    const BORDER: f32 = 6.0;
    const CORNER: f32 = 12.0;

    let rect = ctx.input(|i| i.screen_rect());

    let handles: [(&str, egui::Rect, egui::CursorIcon, egui::ResizeDirection); 8] = [
        (
            "resize_n",
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), BORDER)),
            egui::CursorIcon::ResizeNorth,
            egui::ResizeDirection::North,
        ),
        (
            "resize_s",
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - BORDER),
                egui::vec2(rect.width(), BORDER),
            ),
            egui::CursorIcon::ResizeSouth,
            egui::ResizeDirection::South,
        ),
        (
            "resize_w",
            egui::Rect::from_min_size(rect.min, egui::vec2(BORDER, rect.height())),
            egui::CursorIcon::ResizeWest,
            egui::ResizeDirection::West,
        ),
        (
            "resize_e",
            egui::Rect::from_min_size(
                egui::pos2(rect.max.x - BORDER, rect.min.y),
                egui::vec2(BORDER, rect.height()),
            ),
            egui::CursorIcon::ResizeEast,
            egui::ResizeDirection::East,
        ),
        (
            "resize_nw",
            egui::Rect::from_min_size(rect.min, egui::vec2(CORNER, CORNER)),
            egui::CursorIcon::ResizeNorthWest,
            egui::ResizeDirection::NorthWest,
        ),
        (
            "resize_ne",
            egui::Rect::from_min_size(
                egui::pos2(rect.max.x - CORNER, rect.min.y),
                egui::vec2(CORNER, CORNER),
            ),
            egui::CursorIcon::ResizeNorthEast,
            egui::ResizeDirection::NorthEast,
        ),
        (
            "resize_sw",
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            egui::CursorIcon::ResizeSouthWest,
            egui::ResizeDirection::SouthWest,
        ),
        (
            "resize_se",
            egui::Rect::from_min_size(
                egui::pos2(rect.max.x - CORNER, rect.max.y - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            egui::CursorIcon::ResizeSouthEast,
            egui::ResizeDirection::SouthEast,
        ),
    ];

    for (id, handle_rect, cursor, direction) in handles {
        egui::Area::new(egui::Id::new(id))
            .fixed_pos(handle_rect.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(handle_rect, egui::Sense::drag());
                if response.hovered() {
                    ctx.set_cursor_icon(cursor);
                }
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            });
    }
}

/// Detecta si estamos corriendo dentro de WSL (y no en Linux nativo).
/// WSL define variables de entorno propias, y su kernel se identifica
/// a sí mismo con "microsoft" en /proc/version — revisamos las dos
/// señales por si alguna versión de WSL no define la otra.
#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        return true;
    }
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let v = version.to_lowercase();
        if v.contains("microsoft") || v.contains("wsl") {
            return true;
        }
    }
    false
}

// En Windows nativo o macOS esto no aplica — LIBGL_ALWAYS_SOFTWARE es
// específico de Mesa/Linux, así que directamente no hace nada ahí.
#[cfg(not(target_os = "linux"))]
fn is_wsl() -> bool {
    false
}

fn main() -> eframe::Result<()> {
    // WSLg usa una GPU virtual cuyo contexto OpenGL a veces falla en
    // silencio (la ventana nunca llega a abrirse, sin ningún error).
    // Forzamos renderizado por software SOLO si detectamos WSL — en
    // Linux nativo con una GPU de verdad, esto solo haría que ande
    // más lento sin necesidad.
    if is_wsl() {
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([900.0, 650.0]) // tamaño de referencia antes de maximizar
        .with_maximized(true)
        .with_resizable(true)
        .with_decorations(false); // barra de título propia, ver custom_title_bar en update()

    // Ícono de la barra de título / taskbar, a partir del mismo logo
    // embebido. Si por algún motivo el PNG no decodifica bien, seguimos
    // sin ícono en vez de no abrir la app.
    if let Ok(img) = image::load_from_memory(LOGO_BYTES) {
        let img = img.to_rgba8();
        let (width, height) = (img.width(), img.height());
        viewport = viewport.with_icon(egui::IconData { rgba: img.into_raw(), width, height });
    }

    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Karkinos", options, Box::new(|_cc| Box::new(EditorApp::default())))
}