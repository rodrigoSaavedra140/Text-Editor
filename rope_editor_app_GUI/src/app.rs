use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;

use crate::editor_widget::EditorState;
use crate::undo::{Edit, UndoStack};

/// Estado compartido entre el hilo que escribe el archivo y la UI,
/// que solo lee esto para dibujar la barra de progreso.
pub struct SaveProgress {
    pub written: usize,
    pub total: usize,
    pub done: bool,
    pub error: Option<String>,
}

/// Estilo de fin de línea del archivo tal como vive en el disco (S9).
/// El buffer interno SIEMPRE trabaja con '\n': este estilo solo se
/// usa al serializar de vuelta al guardar, así que un archivo CRLF
/// vuelve a disco idéntico a como estaba.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

/// Detecta el estilo del archivo: CRLF SOLO si cada '\n' viene con su
/// '\r' delante. Un archivo mixto (algunos CRLF, otros LF) se conserva
/// como CRLF únicamente si es uniforme; mixed o suelto → LF, que es
/// el terminador que faltó.
pub fn detect_line_ending(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    if crlf > 0 && crlf == text.matches('\n').count() {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    }
}

/// Serializa el texto del buffer (que usa '\n') con el estilo del
/// archivo. Para LF lo deja tal cual; para CRLF reintroduce el '\r'
/// delante de cada '\n'.
pub fn serialize_line_ending(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => text.to_string(),
        // El buffer no contiene pares '\r\n' (invariante del set_text/
        // open_path), así que el replace directo no puede duplicarlos.
        LineEnding::Crlf => text.replace('\n', "\r\n"),
    }
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

pub struct EditorApp {
    // Buffer "vivo", partido en líneas — así el widget de edición
    // solo tiene que tocar la línea que cambió, no reconstruir todo
    // el texto en cada tecla (eso era lo que volvía lento a
    // egui::TextEdit con archivos grandes).
    pub lines: Vec<String>,
    pub editor_state: EditorState,
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
    // S4: diálogo "Ir a línea" abierto (Ctrl+G) + su campo de texto.
    pub goto_pending: bool,
    pub goto_input: String,
    // Bytes del texto actual, cacheado. Recalcular era O(total) en
    // CADA frame (sumaba el largo de todas las líneas para la barra
    // de estado); ahora solo se refresca cuando el texto cambia.
    pub view_bytes: usize,
    // Listado de la carpeta que se muestra en el desplegable,
    // cacheado: read_dir + abrir los primeros 8 KiB de cada archivo
    // era I/O de disco EN CADA FRAME con el desplegable abierto.
    // Se invalida al navegar a otra carpeta o cuando cambia el
    // mtime de la carpeta (archivos creados/borrados dentro).
    dir_cache: Option<(PathBuf, Option<SystemTime>, Vec<String>, Vec<String>)>,
}

impl Default for EditorApp {
    fn default() -> Self {
        EditorApp {
            lines: vec![String::new()],
            editor_state: EditorState::default(),
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
            line_ending: LineEnding::Lf,
            last_disk_mtime: None,
            close_pending: false,
            close_after_save: false,
            last_mtime_check: None,
            external_changed: false,
            goto_pending: false,
            goto_input: String::new(),
            view_bytes: 0,
            dir_cache: None,
        }
    }
}

impl EditorApp {
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

    /// Reemplaza el contenido completo del buffer a partir de un
    /// String (abrir archivo, deshacer, rehacer). También se usa
    /// solo en momentos puntuales, no por cada tecla.
    /// Invariante del buffer (S9): no hay pares '\r\n' — si el texto
    /// trae, se normalizan a '\n' (un '\r' suelto a mitad de línea se
    /// conserva: es contenido del texto, no un salto).
    pub fn set_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n");
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(|l| l.to_string()).collect()
        };
        self.editor_state.cursor_line = 0;
        self.editor_state.cursor_col = 0;
        self.editor_state.sel_anchor = None;
        self.editor_state.scroll = 0.0;
        self.recalc_view_bytes();
    }

    /// Recalcula el tamaño "en vivo" que muestra la barra de estado.
    /// O(n) en memoria leída (sin alocar el texto entero), y solo se
    /// corre cuando el buffer cambia de verdad: al editar (una vez por
    /// tecla, no por frame), abrir, deshacer o rehacer.
    pub fn recalc_view_bytes(&mut self) {
        self.view_bytes = self.lines.iter().map(|l| l.len()).sum::<usize>()
            + self.lines.len().saturating_sub(1);
    }

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

    /// Si el texto cambió desde el último snapshot, guarda un Edit
    /// (texto antes / texto después) en el UndoStack.
    pub fn snapshot_if_changed(&mut self) {
        let current = self.current_text();
        if current != self.last_snapshot {
            self.undo_stack.push(Edit {
                before: self.last_snapshot.clone(),
                after: current.clone(),
            });
            self.last_snapshot = current;
            self.dirty = true;
        }
    }

    pub fn undo(&mut self) {
        self.snapshot_if_changed();
        if let Some(edit) = self.undo_stack.undo() {
            let text = edit.before.clone();
            self.set_text(&text);
            self.last_snapshot = text;
            self.status = "Deshecho".to_string();
        } else {
            self.status = "Nada para deshacer".to_string();
        }
    }

    pub fn redo(&mut self) {
        if let Some(edit) = self.undo_stack.redo() {
            let text = edit.after.clone();
            self.set_text(&text);
            self.last_snapshot = text;
            self.status = "Rehecho".to_string();
        } else {
            self.status = "Nada para rehacer".to_string();
        }
    }

    /// Abre `path` directamente (usado por el explorador de
    /// carpetas). Después de abrir, el campo "Archivo" solo muestra
    /// el nombre — la ruta completa se ve en la barra de abajo.
    pub fn open_path(&mut self, path: PathBuf) {
        match Self::read_file(&path) {
            Ok((content, ending, mtime)) => {
                self.set_text(&content);
                self.last_snapshot = content;
                self.line_ending = ending;
                self.last_disk_mtime = mtime;
                self.external_changed = false;
                self.last_mtime_check = None;
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

    /// Lee un archivo del disco: contenido ya normalizado a '\n' para
    /// el buffer, el estilo de fin de línea ORIGINAL y su mtime — los
    /// tres juntos, de una sola pasada de I/O.
    fn read_file(
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

    /// Guarda el documento con su estilo de fin de línea original
    /// (S9). Devuelve true SOLO si realmente puso un guardado en
    /// marcha (el callback de cierre lo usa: si no se pudo empezar
    /// a guardar —ruta vacía o otro guardado en curso— el diálogo
    /// de cierre no debe pegar cerrar solo).
    pub fn save(&mut self) -> bool {
        if self.save_progress.is_some() {
            return false; // ya hay un guardado en curso, no arranquemos otro encima
        }

        self.snapshot_if_changed();
        let path = if let Some(p) = &self.current_file {
            p.clone()
        } else {
            PathBuf::from(&self.file_path_input)
        };
        if path.as_os_str().is_empty() {
            self.status = "Escribí una ruta de archivo antes de guardar".to_string();
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
        } else if let Some(path) = &self.current_file {
            self.last_disk_mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        }
    }

    // ---- S1: cierre con cambios sin guardar ----

    /// El usuario pidió cerrar (X o Ctrl+Q) y el diálogo está abierto
    /// pero quiere cerrarlo sin decidir nada todavía: lo usan el
    /// botón "Cancelar" y la tecla Esc.
    pub fn cancel_close(&mut self) {
        self.close_pending = false;
        self.close_after_save = false;
    }

    /// Botón "Descartar" del diálogo: tiran los cambios sin guardar;
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

    /// Reabre el archivo del disco actual (botón "Recargar" de la
    /// barra). NO hace nada si hay cambios sin guardar: pisar el
    /// buffer perdería trabajo del usuario.
    pub fn reload(&mut self) {
        let Some(path) = self.current_file.clone() else {
            return;
        };
        if self.dirty {
            self.status = "Hay cambios sin guardar: guardá primero para recargar".to_string();
            return;
        }
        self.open_path(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- S9: preservar saltos de línea originales ----

    #[test]
    fn detecta_ending_crlf_lf_y_archivo_nuevo() {
        assert_eq!(detect_line_ending("a\r\nb"), LineEnding::Crlf);
        assert_eq!(detect_line_ending("a\nb"), LineEnding::Lf);
        assert_eq!(detect_line_ending(""), LineEnding::Lf); // documento nuevo
        // SI un \r suelto existe sin su \n, no lo contamos como CRLF.
        assert_eq!(detect_line_ending("a\rb\nc"), LineEnding::Lf);
    }

    #[test]
    fn serialize_reintroduce_crlf_y_deja_lf_tal_cual() {
        assert_eq!(
            serialize_line_ending("a\nb\nc", LineEnding::Crlf),
            "a\r\nb\r\nc"
        );
        assert_eq!(serialize_line_ending("a\nb", LineEnding::Lf), "a\nb");
    }

    #[test]
    fn abrir_archivo_crlf_stack_buffer_limpio_y_ending_recordado()
    -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("karkinos_crlf_{}.txt", std::process::id()));
        std::fs::write(&path, "uno\r\ndos\r\n\r\ntres")?;
        let mut app = EditorApp::default();
        app.open_path(path.clone());
        // Dentro del buffer quedan líneas SIN \r escondido.
        assert_eq!(app.lines, vec!["uno", "dos", "", "tres"]);
        // Y el serializer trae de vuelta el archivo EXACTO original.
        assert_eq!(app.current_text_with_ending(), "uno\r\ndos\r\n\r\ntres");
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn abrir_archivo_lf_preserva_lf() -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("karkinos_lf_{}.txt", std::process::id()));
        std::fs::write(&path, "a\nb")?;
        let mut app = EditorApp::default();
        app.open_path(path.clone());
        assert_eq!(app.current_text_with_ending(), "a\nb");
        std::fs::remove_file(path)?;
        Ok(())
    }

    // ---- S1: cierre pedido con cambios sin guardar ----

    #[test]
    fn cerrar_guardando_sin_ruta_no_cierra_por_debajo() {
        let mut app = EditorApp::default();
        app.dirty = true;
        // Sin ruta y sin archivo: save() no puede arrancar → el
        // diálogo NO debe "prometer" cerrar tras un guardado que
        // nunca pasa; tiene que seguir abierto esperando decisión.
        app.confirm_close_save();
        assert!(app.close_pending);
        assert!(!app.close_after_save);
    }

    #[test]
    fn note_save_done_refresca_mtime_y_cancela_cierre_en_error()
    -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("karkinos_save_{}.txt", std::process::id()));
        std::fs::write(&path, "contenido")?;
        let mut app = EditorApp::default();
        app.current_file = Some(path.clone());
        // Memo viejo (como si el buffer hubiera venido de un archivo
        // distinto): tras un guardado ok tiene que quedar "no stale".
        app.last_disk_mtime = None;
        let progress = Arc::new(Mutex::new(SaveProgress {
            written: 9,
            total: 9,
            done: true,
            error: None,
        }));
        app.save_progress = Some(Arc::clone(&progress));
        app.close_after_save = true;
        app.note_save_done();
        assert!(app.last_disk_mtime.is_some(), "post-guardado el memo tiene mtime nuevo");
        assert!(app.close_after_save, "success: sigue el plan de cerrar");

        // Con error: cancelar el cierre automático y NO refrescar memo.
        progress.lock().unwrap().error = Some("disco lleno".to_string());
        app.note_save_done();
        assert!(!app.close_after_save);
        std::fs::remove_file(path)?;
        Ok(())
    }

    // ---- S1: archivo cambiado en el disco (reload externo) ----

    #[test]
    fn mtime_viejo_con_actual_diferente_es_stale() {
        let t = SystemTime::now();
        // No cambió: memo igual.
        assert!(!mtime_is_stale(Some(t), Some(t)));
        // Cambió en el disco (actual más nuevo).
        assert!(mtime_is_stale(Some(t), Some(t + Duration::from_secs(3))));
        // Borrado del disco: no hay mtime actual pero SÍ recordamos uno.
        assert!(mtime_is_stale(None, Some(t)));
        // Archivo en disco cuyo mtime todavía no memorizamos: está
        // "sin cambios por afuera" por DEFAULT — ofrecer recargar
        // sobre un buffer que nunca se comparó con disco sería
        // riesgo de datos.
        assert!(!mtime_is_stale(Some(t), None));
        // Documento nunca ligado a disco: recién ahí tampoco.
        assert!(!mtime_is_stale(None, None));
    }
}
