//! S5 (Roadmap v0.2.9): servicio de documento/tab del editor.
//!
//! Extrae de `app.rs` el estado y la lógica "pura" del documento:
//! buffer activo (`lines`/`buffer`/undo/snapshot), el ciclo de vida de
//! archivos (abrir/leer/guardar/recargar/detección de cambios externos),
//! pestañas (`TabDocument`) y helpers de I/O. Los orquestadores que
//! además tocan presentación (`EditorState`), LSP o el worker de AST
//! (por ejemplo `load_tab`, `set_text`, `undo/redo`) siguen en
//! `app.rs`, que ahora coordina servicios en vez de concentrar todo.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use ropey::Rope;

use crate::buffer::TextBuffer;
use crate::editor_widget::EditorState;
use crate::undo::{ChunkTracker, Edit, UndoStack};

// S9: los estilos de fin de línea viven en `karkinos-core` (única
// definición, ya re-exportada por `karkinos_core`); app.rs dejaba aquí
// una copia duplicada que la T5a elimina.
use karkinos_core::{LineEnding, default_new_file_path, detect_line_ending, serialize_line_ending};

/// Estado compartido entre el hilo que escribe el archivo y la UI,
/// que solo lee esto para dibujar la barra de progreso.
pub struct SaveProgress {
    pub written: usize,
    pub total: usize,
    pub done: bool,
    pub error: Option<String>,
}

/// Detecta si el archivo cambió en el disco respecto a lo que
/// recordamos: mismo mtime = sin cambios; mtime distinto = alguien
/// lo tocó por afuera; ya no existe en el disco = tocado/borrado;
/// con mtime "None" (documento nunca ligado a disco) nunca está viejo.
pub fn mtime_is_stale(
    disk: Option<SystemTime>,
    memo: Option<SystemTime>,
) -> bool {
    match (disk, memo) {
        (Some(a), Some(b)) => a != b,
        (None, Some(_)) => true, // borrado o movido del disco
        _ => false,
    }
}

/// S8 (Etapa 2): Representación de un documento abierto en una pestaña.
#[derive(Debug, Clone)]
pub struct TabDocument {
    #[allow(dead_code)]
    pub id: usize,
    pub lines: Vec<String>,
    pub buffer: TextBuffer,
    pub editor_state: EditorState,
    pub last_snapshot: Rope,
    pub undo_stack: UndoStack,
    pub file_path_input: String,
    pub current_file: Option<PathBuf>,
    pub dirty: bool,
    pub line_ending: LineEnding,
    pub last_disk_mtime: Option<SystemTime>,
    pub external_changed: bool,
    pub last_mtime_check: Option<Instant>,
    pub view_bytes: usize,
}

impl TabDocument {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            lines: vec![String::new()],
            buffer: TextBuffer::new(),
            editor_state: EditorState::default(),
            last_snapshot: Rope::from_str(""),
            undo_stack: UndoStack::new(),
            file_path_input: String::new(),
            current_file: None,
            dirty: false,
            line_ending: LineEnding::Lf,
            last_disk_mtime: None,
            external_changed: false,
            last_mtime_check: None,
            view_bytes: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() || (self.lines.len() == 1 && self.lines[0].trim().is_empty())
    }

    pub fn display_title(&self) -> String {
        let name = if let Some(ref p) = self.current_file {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string()
        } else if !self.file_path_input.is_empty() {
            self.file_path_input.clone()
        } else {
            "Untitled".to_string()
        };

        if self.dirty {
            format!("{} •", name)
        } else {
            name
        }
    }
}

/// Servicio de documento: buffer activo + pestañas + ciclo de vida de
/// archivos. T5a: posee el estado y la lógica pura; los métodos que
/// además alimentan la presentación, el LSP o el AST quedan como
/// orquestadores en `app.rs`.
pub struct DocumentService {
    // Buffer "vivo", partido en líneas para render y edición por franjas.
    pub lines: Vec<String>,
    pub buffer: TextBuffer,
    pub last_snapshot: Rope, // snapshot del buffer en Rope (Copy-On-Write O(1) con UndoStack)
    pub undo_stack: UndoStack,
    pub file_path_input: String,
    pub current_file: Option<PathBuf>,
    pub dirty: bool,
    pub status: String,
    // Carpeta que está mostrando el desplegable de archivos ahora
    // mismo — no tiene por qué ser la misma que la carpeta desde
    // donde se lanzó el programa, porque se puede navegar.
    pub browse_dir: PathBuf,
    // Some() mientras hay un guardado en curso en otro hilo.
    pub save_progress: Option<Arc<Mutex<SaveProgress>>>,
    // Momento en que se pidió el cierre prolijo — si pasa mucho
    // tiempo sin que la ventana realmente se cierre (WSLg no
    // responde bien a veces), forzamos la salida igual.
    pub closing_since: Option<Instant>,
    // S9: estilo de fin de línea con el que se abrió el archivo en
    // disco (Lf para documentos nuevos).
    pub line_ending: LineEnding,
    // S9: mtime del archivo la última vez que lo leímos o guardamos
    // (None = documento nuevo, nunca ligado a disco).
    pub last_disk_mtime: Option<SystemTime>,
    // S1: cierre pedido con cambios sin guardar — el diálogo
    // Guardar / Descartar / Cancelar está visible.
    pub close_pending: bool,
    // S1: el usuario eligió Guardar en el diálogo — cerrar la
    // ventana en cuanto el guardado termine bien.
    pub close_after_save: bool,
    // S1: fecha del último stat de disco (limitamos las comprobaciones
    // a una por segundo para no hacer I/O en cada frame).
    pub last_mtime_check: Option<Instant>,
    // S1: el archivo en disco cambió por afuera y el buffer está
    // limpio — la barra ofrece "Recargar".
    pub external_changed: bool,
    // Bytes del texto actual, cacheados. Recalcular era O(total) en
    // CADA frame (sumaba el largo de todas las líneas para la barra
    // de estado); ahora solo se refresca cuando el texto cambia.
    pub view_bytes: usize,
    // S3: agrupamiento de ediciones en chunks por inactividad o Enter/Paste.
    pub chunk_tracker: ChunkTracker,
    // S8: confirmación de creación de nuevo documento con cambios sin guardar.
    pub new_doc_pending: bool,
    pub new_after_save: bool,
    // S8 (Etapa 2): Pestañas / Múltiples documentos abiertos simultáneamente.
    pub tabs: Vec<TabDocument>,
    pub active_tab: usize,
    pub next_tab_id: usize,
    pub tab_close_pending: Option<usize>,
    pub tab_close_after_save: Option<usize>,
    // Modal para nombrar archivo sin nombre al guardar.
    pub save_name_pending: bool,
    pub save_name_input: String,
    // Listado de la carpeta que se muestra en el desplegable,
    // cacheado: read_dir + abrir los primeros 8 KiB de cada archivo
    // era I/O de disco EN CADA FRAME con el desplegable abierto.
    // Se invalida al navegar a otra carpeta o cuando cambia el
    // mtime de la carpeta (archivos creados/borrados dentro).
    dir_cache: Option<(PathBuf, Option<SystemTime>, Vec<String>, Vec<String>)>,
}

impl Default for DocumentService {
    fn default() -> Self {
        DocumentService {
            lines: vec![String::new()],
            buffer: TextBuffer::new(),
            last_snapshot: Rope::from_str(""),
            undo_stack: UndoStack::new(),
            file_path_input: String::new(),
            current_file: None,
            dirty: false,
            status: "Ready".to_string(),
            browse_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            save_progress: None,
            closing_since: None,
            line_ending: LineEnding::Lf,
            last_disk_mtime: None,
            close_pending: false,
            close_after_save: false,
            last_mtime_check: None,
            external_changed: false,
            view_bytes: 0,
            chunk_tracker: ChunkTracker::default_timeout(),
            new_doc_pending: false,
            new_after_save: false,
            tabs: vec![TabDocument::new(1)],
            active_tab: 0,
            next_tab_id: 2,
            tab_close_pending: None,
            tab_close_after_save: None,
            save_name_pending: false,
            save_name_input: String::new(),
            dir_cache: None,
        }
    }
}

impl DocumentService {
    /// Listado de `self.browse_dir` con caché (ver el campo
    /// `dir_cache`): mismo estado que antes, salvo que en frames
    /// repetidos con el desplegable abierto no hay I/O de disco.
    pub fn cached_list_dir(&mut self) -> (Vec<String>, Vec<String>) {
        let mtime = std::fs::metadata(&self.browse_dir)
            .ok()
            .and_then(|m| m.modified().ok());
        let hit = match &self.dir_cache {
            Some((dir, mt, dirs, files)) if *dir == self.browse_dir && *mt == mtime => {
                Some((dirs.clone(), files.clone()))
            }
            _ => None,
        };
        if let Some(hit) = hit {
            return hit;
        }
        let listing = Self::list_dir(&self.browse_dir);
        self.dir_cache = Some((self.browse_dir.clone(), mtime, listing.0.clone(), listing.1.clone()));
        listing
    }

    pub fn is_empty_buffer(&self) -> bool {
        self.lines.len() <= 1 && self.lines.first().map(|l| l.is_empty()).unwrap_or(true)
    }

    /// S3: Registra un snapshot explícito para un texto dado. Si coincide
    /// con el último snapshot, no hace nada (evita pasos vacíos).
    pub fn snapshot_text(&mut self, text: &str) {
        let next_snapshot = Rope::from_str(text);
        if next_snapshot != self.last_snapshot {
            self.undo_stack.push(Edit {
                before: self.last_snapshot.clone(),
                after: next_snapshot.clone(),
            });
            self.last_snapshot = next_snapshot;
            self.dirty = true;
            self.chunk_tracker.reset();
        }
    }

    /// Si el texto cambió desde el último snapshot, guarda un Edit
    /// (texto antes / texto después) en el UndoStack.
    pub fn snapshot_if_changed(&mut self) {
        let current = self.current_text();
        self.snapshot_text(&current);
    }

    /// Reconstruye el texto completo a partir de `lines` — es O(n),
    /// así que solo se llama en momentos puntuales (snapshot, guardar),
    /// nunca en cada tecla.
    pub fn current_text(&self) -> String {
        self.lines.join("\n")
    }

    /// El texto del buffer serializado con el estilo de fin de línea
    /// que registró el archivo al abrirse (S9): así un .txt que llega
    /// con CRLF sale a disco EXACTAMENTE como estaba, sin '\r'
    /// escondidos en el medio (el buffer solo usa '\n').
    pub fn current_text_with_ending(&self) -> String {
        let text = self.current_text();
        serialize_line_ending(&text, self.line_ending)
    }

    /// Recalcula el tamaño "en vivo" que muestra la barra de estado.
    /// O(n) en memoria leída (sin alocar el texto entero), y solo se
    /// corre cuando el buffer cambia de verdad: al editar (una vez por
    /// tecla, no por frame), abrir, deshacer o rehacer.
    pub fn recalc_view_bytes(&mut self) {
        self.view_bytes = self.lines.iter().map(|l| l.len()).sum::<usize>()
            + self.lines.len().saturating_sub(1);
    }

    /// Lee un archivo del disco: contenido ya normalizado a '\n' para
    /// el buffer, el estilo de fin de línea ORIGINAL y su mtime — los
    /// tres juntos, de una sola pasada de I/O.
    pub fn read_file(
        path: &std::path::Path,
    ) -> std::io::Result<(String, LineEnding, Option<SystemTime>)> {
        use std::io::Read;

        let mut bytes = Vec::new();
        fs::File::open(path)?.read_to_end(&mut bytes)?;
        let raw = String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let ending = detect_line_ending(&raw);
        let content = raw.replace("\r\n", "\n");
        let mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        Ok((content, ending, mtime))
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

    /// Intenta guardar el documento. Si el buffer actual no tiene archivo ni nombre asignado,
    /// abre el diálogo modal para que el usuario ingrese un nombre personalizado.
    pub fn try_save(&mut self) -> bool {
        if self.current_file.is_none() && self.file_path_input.trim().is_empty() {
            self.save_name_input.clear();
            self.save_name_pending = true;
            false
        } else {
            self.save()
        }
    }

    /// Confirma el nombre ingresado en el diálogo modal y procede al guardado.
    /// Si el campo se dejó en blanco o contiene solo espacios, guarda automáticamente como `untitled.txt`
    /// (o correlativo sin pisar archivos existentes).
    pub fn confirm_save_name(&mut self) -> bool {
        self.save_name_pending = false;
        let name = self.save_name_input.trim();
        if !name.is_empty() {
            self.file_path_input = name.to_string();
            self.current_file = None;
        }
        self.save()
    }

    /// S5: Abre el diálogo modal para guardar el documento actual con un nombre nuevo ("Guardar como...").
    pub fn save_as(&mut self) {
        if let Some(name) = self
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            self.save_name_input = name.to_string();
        } else {
            self.save_name_input = self.file_path_input.clone();
        }
        self.save_name_pending = true;
    }

    /// Cancela el diálogo de guardado con nombre sin modificar el estado.
    pub fn cancel_save_name(&mut self) {
        self.save_name_pending = false;
        self.save_name_input.clear();
    }

    /// Guarda el documento con su estilo de fin de línea original
    /// (S9). Devuelve true SOLO si realmente puso un guardado en
    /// marcha (el callback de cierre lo usa: si no se pudo empezar
    /// a guardar —ruta vacía o otro guardado en curso— el diálogo
    /// de cierre no debe cerrar solo).
    pub fn save(&mut self) -> bool {
        if self.save_progress.is_some() {
            return false; // ya hay un guardado en curso, no arranquemos otro encima
        }

        self.snapshot_if_changed();
        let path = if let Some(p) = &self.current_file {
            p.clone()
        } else if self.file_path_input.is_empty() {
            // Archivo nuevo sin nombre: pide a default_new_file_path
            // el primero libre "untitled.txt"/"untitled (N).txt" en la
            // carpeta mostrada por el explorador. El path quedará ABSOLUTO
            // (no a través del input, que muestra solo el nombre).
            let exists = |p: &std::path::Path| std::fs::metadata(p).is_ok();
            let p = default_new_file_path(&self.browse_dir, &exists);
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                self.file_path_input = name.to_string();
            }
            p
        } else {
            let p = PathBuf::from(&self.file_path_input);
            if p.is_relative() {
                self.browse_dir.join(p)
            } else {
                p
            }
        };
        if path.as_os_str().is_empty() {
            self.status = "Please provide a file path before saving".to_string();
            return false;
        }

        let data = self.current_text_with_ending().into_bytes();
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
        true
    }

    /// Fin de un guardado: refresca el mtime recordado del archivo
    /// (el disco cambió al escribir!) para que la detección de
    /// cambios externos (S1) no marque como "tocado por afuera" el
    /// archivo que ACABAMOS de guardar. Con error no refresca y, si
    /// la ventana estaba esperando cerrar tras guardar, se queda
    /// abierta para que el usuario vea el problema.
    pub fn note_save_done(&mut self) {
        let had_error = match &self.save_progress {
            Some(p) => p.lock().map(|p| p.error.is_some()).unwrap_or(false),
            None => false,
        };
        if had_error {
            self.close_after_save = false;
            self.new_after_save = false;
        } else if let Some(path) = &self.current_file {
            self.last_disk_mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        }
    }

    // ---- S1 / S8: confirmación con cambios sin guardar ----

    /// El usuario pidió cancelar el diálogo de confirmación: lo usan
    /// el botón "Cancelar" y la tecla Esc.
    pub fn cancel_close(&mut self) {
        self.close_pending = false;
        self.close_after_save = false;
        self.new_doc_pending = false;
        self.new_after_save = false;
    }

    /// Botón "Descartar" del diálogo: tira los cambios sin guardar;
    /// el llamador (ui.rs) ejecuta a continuación el cierre real de
    /// la ventana (necesita el egui::Context).
    pub fn confirm_close_discard(&mut self) {
        self.close_pending = false;
        self.close_after_save = false;
    }

    /// Botón "Guardar" del diálogo: re-lanzar guardado y cerrar en
    /// cuanto baje. Si el guardado no pudo arrancar (ruta vacía,
    /// otro guardado en curso) el diálogo queda abierto: mentirle
    /// cerrando la ventana perdería datos.
    pub fn confirm_close_save(&mut self) {
        self.close_pending = false;
        self.close_after_save = true;
        if !self.save() {
            self.close_after_save = false;
            self.close_pending = true;
        }
    }

    // ---- S8: nuevo documento ----

    /// Confirmación "Guardar" al intentar abrir un nuevo documento.
    pub fn confirm_new_doc_save(&mut self) {
        self.new_doc_pending = false;
        self.new_after_save = true;
        if !self.save() {
            self.new_after_save = false;
            self.new_doc_pending = true;
        }
    }

    // ---- S1: archivo cambiado por afuera ----

    /// Compara el mtime del archivo en disco con el recordado de la
    /// última apertura o del último guardado (S1). Limitado a UN
    /// stat por segundo: corre en cada frame y una syscall por cada
    /// frame sería I/O innecesario.
    pub fn check_external_changes(&mut self) {
        // Mientras se guarda, el archivo en disco está SIEMPRE
        // "distinto" del memo: esperar a que el guardado baje para
        // no ofrecer "Recargar" a la mitad de un write propio.
        if self.save_progress.is_some() {
            return;
        }
        let now = Instant::now();
        if let Some(last) = self.last_mtime_check {
            if now.duration_since(last) < Duration::from_secs(1) {
                return;
            }
        }
        self.last_mtime_check = Some(now);
        let Some(path) = self.current_file.clone() else {
            self.external_changed = false;
            return;
        };
        let disk = fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        self.external_changed = mtime_is_stale(disk, self.last_disk_mtime);
    }

    /// S1/S8: Retorna el índice de la primera pestaña que tenga cambios sin guardar.
    /// Prioriza la pestaña activa si está sucia para atender primero lo que ve el usuario.
    pub fn first_dirty_tab_index(&self) -> Option<usize> {
        if self.dirty {
            return Some(self.active_tab);
        }
        self.tabs.iter().enumerate().position(|(idx, t)| {
            if idx == self.active_tab {
                self.dirty
            } else {
                t.dirty
            }
        })
    }

    /// S1/S8: Retorna la cantidad total de pestañas con cambios sin guardar.
    pub fn dirty_tabs_count(&self) -> usize {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(idx, t)| {
                if *idx == self.active_tab {
                    self.dirty
                } else {
                    t.dirty
                }
            })
            .count()
    }

    /// Retorna `true` si todas las pestañas marcadas como sucias están vacías (sin contenido real).
    pub fn all_dirty_tabs_are_empty(&self) -> bool {
        self.tabs.iter().enumerate().all(|(idx, t)| {
            let is_dirty = if idx == self.active_tab { self.dirty } else { t.dirty };
            let is_empty = if idx == self.active_tab {
                self.is_buffer_empty()
            } else {
                t.is_empty()
            };
            !is_dirty || is_empty
        })
    }

    /// Retorna `true` si el buffer actual no contiene texto real (vacío o solo whitespace).
    pub fn is_buffer_empty(&self) -> bool {
        self.lines.is_empty() || (self.lines.len() == 1 && self.lines[0].trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtime_is_stale_detecta_borrado_y_tocados() {
        let t0 = std::time::SystemTime::UNIX_EPOCH;
        let t1 = t0 + std::time::Duration::from_secs(1);
        assert!(!mtime_is_stale(Some(t0), Some(t0)));
        assert!(mtime_is_stale(Some(t1), Some(t0)));
        assert!(mtime_is_stale(None, Some(t0)));
        assert!(!mtime_is_stale(None, None));
        assert!(!mtime_is_stale(Some(t0), None));
    }
}
