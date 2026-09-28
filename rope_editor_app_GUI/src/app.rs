use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::rope::Rope;
use crate::undo::{Edit, EditKind, UndoStack};

/// Estado compartido entre el hilo que escribe el archivo y la UI,
/// que solo lee esto para dibujar la barra de progreso.
pub struct SaveProgress {
    pub written: usize,
    pub total: usize,
    pub done: bool,
    pub error: Option<String>,
}

pub struct EditorApp {
    pub text: String,          // buffer "vivo" que edita el widget de egui
    pub last_snapshot: String, // texto en el último snapshot guardado (para detectar cambios)
    pub undo_stack: UndoStack,
    pub file_path_input: String,
    pub current_file: Option<PathBuf>,
    pub dirty: bool,
    pub status: String,
    // Carpeta que está mostrando el desplegable de archivos ahora
    // mismo — no tiene por qué ser la misma que la carpeta desde
    // donde se lanzó el programa, porque se puede navegar.
    pub browse_dir: PathBuf,
    // Color de fondo del área de texto.
    pub text_area_bg: egui::Color32,
    // Textura del logo, cargada una sola vez en el primer frame
    // (None hasta ese momento).
    pub logo_texture: Option<egui::TextureHandle>,
    // Some() mientras hay un guardado en curso en otro hilo.
    pub save_progress: Option<Arc<Mutex<SaveProgress>>>,
    // Momento en que se pidió el cierre prolijo — si pasa mucho
    // tiempo sin que la ventana realmente se cierre (WSLg no
    // responde bien a veces), forzamos la salida igual.
    pub closing_since: Option<Instant>,
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
    pub fn snapshot_if_changed(&mut self) {
        if self.text != self.last_snapshot {
            let before = Rope::new(&self.last_snapshot);
            let after = Rope::new(&self.text);
            self.undo_stack.push(Edit { kind: EditKind::Replace, position: 0, before, after });
            self.last_snapshot = self.text.clone();
            self.dirty = true;
        }
    }

    pub fn undo(&mut self) {
        self.snapshot_if_changed();
        if let Some(edit) = self.undo_stack.undo() {
            self.text = edit.before.to_string();
            self.last_snapshot = self.text.clone();
            self.status = "Deshecho".to_string();
        } else {
            self.status = "Nada para deshacer".to_string();
        }
    }

    pub fn redo(&mut self) {
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
    pub fn open_path(&mut self, path: PathBuf) {
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
    pub fn open(&mut self) {
        self.open_path(PathBuf::from(&self.file_path_input));
    }

    /// Heurística simple para distinguir texto de binario (la misma
    /// que usa Git): si los primeros bytes del archivo no tienen
    /// ningún byte nulo, lo tratamos como texto. No depende de la
    /// extensión — agarra .md, .rs, .log, archivos sin extensión,
    /// etc., y deja afuera binarios de verdad (imágenes, ejecutables).
    pub fn is_probably_text(path: &std::path::Path) -> bool {
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
    pub fn list_dir(dir: &std::path::Path) -> (Vec<String>, Vec<String>) {
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

    pub fn save(&mut self) {
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