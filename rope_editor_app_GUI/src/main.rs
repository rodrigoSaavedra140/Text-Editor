mod rope;
mod undo;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use rope::Rope;
use undo::{Edit, EditKind, UndoStack};

struct EditorApp {
    text: String,          // buffer "vivo" que edita el widget de egui
    last_snapshot: String, // texto en el último snapshot guardado (para detectar cambios)
    undo_stack: UndoStack,
    file_path_input: String,
    current_file: Option<PathBuf>,
    dirty: bool,
    status: String,
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

    fn open(&mut self) {
        let path = PathBuf::from(&self.file_path_input);
        match fs::read_to_string(&path) {
            Ok(content) => {
                self.text = content.clone();
                self.last_snapshot = content;
                self.current_file = Some(path);
                self.dirty = false;
                self.status = "Archivo abierto".to_string();
            }
            Err(e) => {
                self.status = format!("Error al abrir: {}", e);
            }
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

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Archivo:");
                ui.text_edit_singleline(&mut self.file_path_input);
                if ui.button("Abrir").clicked() {
                    self.open();
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
                ui.separator();
                if ui.button("Salir (Ctrl+Q)").clicked() {
                    request_close(ctx, &mut self.closing_since);
                }
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let modified = if self.dirty { " [+]" } else { "" };
            let name = self
                .current_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "[sin nombre]".to_string());
            ui.label(format!("{}{}  —  {}", name, modified, self.status));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(40),
                );
                if response.changed() {
                    self.dirty = true;
                }
                // Al perder el foco (click afuera, Tab, etc.) tomamos
                // un snapshot para el historial de undo/redo.
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

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native("Rope Editor", options, Box::new(|_cc| Box::new(EditorApp::default())))
}