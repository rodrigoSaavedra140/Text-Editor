mod rope;
mod undo;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use rope::Rope;
use undo::{Edit, EditKind, UndoStack};

/// Logo de Karkinos, embebido en el binario en tiempo de compilación
/// (no se lee de disco en tiempo de ejecución — así el ícono y el
/// fondo minimalista funcionan siempre, sin depender de que el
/// archivo esté presente donde sea que se ejecute el programa).
const LOGO_BYTES: &[u8] = include_bytes!("../assets/logo.png");

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
        match fs::write(&path, &self.text) {
            Ok(()) => {
                self.current_file = Some(path);
                self.dirty = false;
                self.status = "Guardado".to_string();
            }
            Err(e) => {
                self.status = format!("Error al guardar: {}", e);
            }
        }
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

        // Barra de marca propia (logo + "Karkinos"), debajo de la
        // barra nativa del sistema operativo — bajo WSLg esa barra
        // nativa a veces queda vacía, sin ícono ni texto (aunque los
        // botones de minimizar/maximizar/cerrar sí funcionan), así
        // que el logo y el nombre quedan acá en vez de ahí. No le
        // saco la decoración nativa a la ventana: así no perdemos el
        // arrastre del borde para cambiar el tamaño.
        egui::TopBottomPanel::top("brand_bar")
            .exact_height(30.0)
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(18, 18, 18)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    if let Some(tex) = &self.logo_texture {
                        ui.add(egui::Image::new((tex.id(), egui::vec2(18.0, 18.0))));
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Karkinos").strong());
                });
            });

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
        .with_resizable(true);

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