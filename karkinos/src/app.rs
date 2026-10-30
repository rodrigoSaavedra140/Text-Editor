use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use ropey::Rope;

use crate::buffer::TextBuffer;
use crate::editor_widget::EditorState;
use crate::undo::{ChunkTracker, Edit, UndoStack};

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

// S10: rango y paso del zoom de la fuente del editor (clamp 8..32 px).
pub const ZOOM_MIN: f32 = 8.0;
pub const ZOOM_MAX: f32 = 32.0;
pub const ZOOM_STEP: f32 = 1.0;

/// Helper del selector de archivos (popup propio): puede quedarse
/// abierto o cerrar si el click completo de este frame NO cayó ni
/// adentro del popup ni sobre el botón que lo togglea. Sin rects
/// conocidos se resuelve CERRAR (estado seguro).
pub fn picker_should_stay_open(
    click: egui::Pos2,
    popup: Option<egui::Rect>,
    button: Option<egui::Rect>,
) -> bool {
    let inside = |r: Option<egui::Rect>| r.map(|r| r.contains(click)).unwrap_or(false);
    inside(popup) || inside(button)
}

/// Default file path for an unsaved file without a path: `untitled.txt` in `dir`.
pub fn default_new_file_path(
    dir: &std::path::Path,
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    for n in 0..1000u32 {
        let name = if n == 0 {
            "untitled.txt".to_string()
        } else {
            format!("untitled ({n}).txt")
        };
        let path = dir.join(name);
        if !exists(&path) {
            return path;
        }
    }
    dir.join("untitled-file.txt")
}

/// Detecta si el archivo cambió en el disco respecto a lo que
/// recordamos: mismo mtime = sin cambios; mtime distinto = alguien
/// lo tocó por afuera; ya no existe en el disco = tocado/borrado;
/// con mtime "None" (documento nunca ligado a disco) nunca está viejo.
/// Política pura del zoom (S10): redondea al paso entero y recorta
/// al clamp [8, 32].
pub fn apply_zoom(current: f32, zoom_in: bool) -> f32 {
    let next = if zoom_in {
        current + ZOOM_STEP
    } else {
        current - ZOOM_STEP
    };
    let snapped = next.round();
    snapped.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// S10: serialización compacta para guardar el zoom en el Storage de
/// egui/eframe: un float con un decimal, ida y vuelta exacta.
pub fn font_size_to_string(size: f32) -> String {
    format!("{:.1}", size)
}

/// S10: decode inverso del valor guardado. Un Storage con basura
/// devuelve None y el llamador titula el default.
pub fn font_size_from_string(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().map(|v| v.clamp(ZOOM_MIN, ZOOM_MAX))
}

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

pub struct EditorApp {
    // Buffer "vivo", partido en líneas para render y edición por franjas.
    pub lines: Vec<String>,
    pub buffer: TextBuffer,
    pub editor_state: EditorState,
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
    // S3: agrupamiento de ediciones en chunks por inactividad o Enter/Paste.
    pub chunk_tracker: ChunkTracker,
    // S2: búsqueda y reemplazo (Ctrl+F).
    pub find_open: bool,
    pub find_query: String,
    pub find_replace: String,
    pub find_case_sensitive: bool,
    /// Posiciones de matches: (línea, columna-inicio en chars).
    pub find_matches: Vec<(usize, usize)>,
    /// Índice del match actual dentro de find_matches.
    pub find_current: usize,
    // S8: confirmación de creación de nuevo documento con cambios sin guardar.
    pub new_doc_pending: bool,
    pub new_after_save: bool,
    // Selector de carpetas propio (popup debrowse): se mantiene
    // abierto mientras se navega entre carpetas — el ComboBox de egui
    // cerraba el popup con CUALQUIER click adentro (navegar = reabrir
    // cada vez). button rect se registra cada frame en ui.rs.
    pub picker_open: bool,
    pub picker_button_rect: Option<egui::Rect>,
    // S10: tamaño en puntos de la fuente monoespaciada del editor
    // (clamp 8..32). Solo la fuente del editor, no toda la UI.
    pub font_size: f32,
    // S5 (Etapa 2): Tema visual activo (Dark, Monokai, Dracula).
    pub theme_mode: crate::theme::ThemeMode,
    // S7 (Etapa 2): Estado del modal de búsqueda rápida de archivos (Ctrl+P).
    pub quick_open: crate::quick_open::QuickOpenState,
    // S8 (Etapa 2): Pestañas / Múltiples documentos abiertos simultáneamente.
    pub tabs: Vec<TabDocument>,
    pub active_tab: usize,
    pub next_tab_id: usize,
    pub tab_close_pending: Option<usize>,
    pub tab_close_after_save: Option<usize>,
    // Modal para nombrar archivo sin nombre al guardar.
    pub save_name_pending: bool,
    pub save_name_input: String,
    dir_cache: Option<(PathBuf, Option<SystemTime>, Vec<String>, Vec<String>)>,
    // S4 (Etapa 4): Cliente Language Server Protocol (LSP)
    pub lsp_client: Option<karkinos_core::lsp::LspClient>,
    // S4 (Etapa 5): Gestor de extensiones y plugins
    pub plugin_manager: karkinos_core::plugin::PluginManager,
    pub extensions_modal_open: bool,
    pub extensions_tab: usize,
    // S1 (Rust IDE): Ejecutor nativo de pipelines de Cargo
    pub cargo_runner: karkinos_core::cargo::CargoRunner,
    pub cargo_panel_open: bool,
    pub cargo_active_tab: usize,
}

impl Default for EditorApp {
    fn default() -> Self {
        EditorApp {
            lines: vec![String::new()],
            buffer: TextBuffer::new(),
            editor_state: EditorState::default(),
            last_snapshot: Rope::from_str(""),
            undo_stack: UndoStack::new(),
            file_path_input: String::new(),
            current_file: None,
            dirty: false,
            status: "Ready".to_string(),
            browse_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            logo_texture: None,
            save_progress: None,
            closing_since: None,
            line_ending: LineEnding::Lf,
            last_disk_mtime: None,
            close_pending: false,
            close_after_save: false,
            new_doc_pending: false,
            new_after_save: false,
            picker_open: false,
            picker_button_rect: None,
            last_mtime_check: None,
            external_changed: false,
            goto_pending: false,
            goto_input: String::new(),
            view_bytes: 0,
            chunk_tracker: ChunkTracker::default_timeout(),
            find_open: false,
            find_query: String::new(),
            find_replace: String::new(),
            find_case_sensitive: false,
            find_matches: Vec::new(),
            find_current: 0,
            font_size: 14.0,
            theme_mode: crate::theme::ThemeMode::Dark,
            quick_open: crate::quick_open::QuickOpenState::default(),
            tabs: vec![TabDocument::new(1)],
            active_tab: 0,
            next_tab_id: 2,
            tab_close_pending: None,
            tab_close_after_save: None,
            save_name_pending: false,
            save_name_input: String::new(),
            dir_cache: None,
            lsp_client: None,
            plugin_manager: karkinos_core::plugin::PluginManager::with_default_plugins(),
            extensions_modal_open: false,
            extensions_tab: 0,
            cargo_runner: karkinos_core::cargo::CargoRunner::new(),
            cargo_panel_open: false,
            cargo_active_tab: 0,
        }
    }
}

impl EditorApp {
    /// S5 (Etapa 2): Retorna la paleta de colores del tema activo.
    pub fn theme(&self) -> crate::theme::Theme {
        self.theme_mode.theme()
    }

    /// S6 (Etapa 2): Retorna el lenguaje detectado para el documento actual.
    pub fn current_language(&self) -> crate::syntax::Language {
        if let Some(ref p) = self.current_file {
            crate::syntax::Language::from_path(p)
        } else {
            crate::syntax::Language::from_extension(
                std::path::Path::new(&self.file_path_input)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or(""),
            )
        }
    }

    /// S8 (Etapa 2): Guarda el estado actual del editor en la pestaña activa.
    pub fn sync_active_to_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.lines = self.lines.clone();
            tab.buffer = self.buffer.clone();
            tab.editor_state = self.editor_state.clone();
            tab.last_snapshot = self.last_snapshot.clone();
            tab.undo_stack = self.undo_stack.clone();
            tab.file_path_input = self.file_path_input.clone();
            tab.current_file = self.current_file.clone();
            tab.dirty = self.dirty;
            tab.line_ending = self.line_ending;
            tab.last_disk_mtime = self.last_disk_mtime;
            tab.external_changed = self.external_changed;
            tab.last_mtime_check = self.last_mtime_check;
            tab.view_bytes = self.view_bytes;
        }
    }

    /// S2: Sincronización ligera de metadatos (dirty, view_bytes, cursor) sin clonar lines ni undo_stack.
    pub fn sync_active_metadata_to_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.dirty = self.dirty;
            tab.view_bytes = self.view_bytes;
            tab.current_file = self.current_file.clone();
            tab.file_path_input = self.file_path_input.clone();
            tab.external_changed = self.external_changed;
            tab.editor_state.cursor_line = self.editor_state.cursor_line;
            tab.editor_state.cursor_col = self.editor_state.cursor_col;
            tab.editor_state.scroll = self.editor_state.scroll;
        }
    }

    /// S4: Avanza a la siguiente pestaña de manera cíclica.
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            let next = (self.active_tab + 1) % self.tabs.len();
            self.switch_tab(next);
        }
    }

    /// S4: Retrocede a la pestaña anterior de manera cíclica.
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            let prev = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
            self.switch_tab(prev);
        }
    }

    /// S8 (Etapa 2): Carga los datos de la pestaña `idx` en el estado activo del editor.
    pub fn load_tab(&mut self, idx: usize) {
        if let Some(tab) = self.tabs.get(idx) {
            self.active_tab = idx;
            self.lines = tab.lines.clone();
            self.buffer = tab.buffer.clone();
            self.editor_state = tab.editor_state.clone();
            self.last_snapshot = tab.last_snapshot.clone();
            self.undo_stack = tab.undo_stack.clone();
            self.file_path_input = tab.file_path_input.clone();
            self.current_file = tab.current_file.clone();
            self.dirty = tab.dirty;
            self.line_ending = tab.line_ending;
            self.last_disk_mtime = tab.last_disk_mtime;
            self.external_changed = tab.external_changed;
            self.last_mtime_check = tab.last_mtime_check;
            self.view_bytes = tab.view_bytes;
            self.chunk_tracker.reset();
            self.find_matches.clear();

            // S3: If AST hasn't been initialized yet for a Rust file, initialize it once
            if self.current_language() == crate::syntax::Language::Rust && self.editor_state.ast_engine.is_none() {
                self.update_ast();
            }

            // S4: Fast in-memory diagnostics lookup from existing singleton LSP client without process re-spawning
            if let (Some(ref client), Some(ref path)) = (&self.lsp_client, &self.current_file) {
                let uri = format!("file://{}", path.display());
                self.editor_state.diagnostics = client.diagnostics_for_file(&uri);
            }
        }
    }

    /// S8 (Etapa 2): Cambia a la pestaña indicada, sincronizando la actual previamente.
    pub fn switch_tab(&mut self, new_idx: usize) {
        if new_idx == self.active_tab || new_idx >= self.tabs.len() {
            return;
        }
        self.sync_active_to_tab();
        self.load_tab(new_idx);
        self.status = format!("Tab: {}", self.tabs[self.active_tab].display_title());
    }

    /// S8 (Etapa 2): Crea una nueva pestaña en blanco y la activa.
    pub fn new_tab(&mut self) {
        self.sync_active_to_tab();
        let new_tab = TabDocument::new(self.next_tab_id);
        self.next_tab_id += 1;
        self.tabs.push(new_tab);
        self.load_tab(self.tabs.len() - 1);
        self.status = "New tab created".to_string();
    }

    /// S8 (Etapa 2): Cierra la pestaña en `idx`. Si es la única pestaña, la resetea en blanco.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }

        if self.tabs.len() == 1 {
            let reset_tab = TabDocument::new(self.next_tab_id);
            self.next_tab_id += 1;
            self.tabs[0] = reset_tab;
            self.load_tab(0);
            return;
        }

        self.sync_active_to_tab();
        self.tabs.remove(idx);

        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if idx < self.active_tab {
            self.active_tab -= 1;
        }

        self.load_tab(self.active_tab);
    }

    /// S8 (Etapa 2): Intenta cerrar una pestaña. Si tiene cambios sin guardar,
    /// pide confirmación estableciendo `tab_close_pending`.
    pub fn try_close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }

        let is_dirty = if idx == self.active_tab {
            self.dirty
        } else {
            self.tabs[idx].dirty
        };

        if is_dirty {
            self.tab_close_pending = Some(idx);
        } else {
            self.close_tab(idx);
        }
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
        self.buffer.set_text(&text);
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(|l| l.to_string()).collect()
        };
        self.editor_state.cursor_line = 0;
        self.editor_state.cursor_col = 0;
        self.editor_state.sel_anchor = None;
        self.editor_state.scroll = 0.0;
        self.chunk_tracker.reset();
        self.recalc_view_bytes();
        self.update_ast();
        self.update_lsp();
    }

    /// Recalcula el tamaño "en vivo" que muestra la barra de estado.
    /// O(n) en memoria leída (sin alocar el texto entero), y solo se
    /// corre cuando el buffer cambia de verdad: al editar (una vez por
    /// tecla, no por frame), abrir, deshacer o rehacer.
    pub fn recalc_view_bytes(&mut self) {
        self.view_bytes = self.lines.iter().map(|l| l.len()).sum::<usize>()
            + self.lines.len().saturating_sub(1);
    }

    /// S3 (Etapa 3): Actualiza el motor de sintaxis semántico AST y los rangos de plegado.
    pub fn update_ast(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        if is_rust {
            if self.editor_state.ast_engine.is_none() {
                if let Ok(engine) = karkinos_core::syntax_ast::AstEngine::new(karkinos_core::syntax_ast::AstLanguage::Rust) {
                    self.editor_state.ast_engine = Some(engine);
                }
            }
            let code = self.current_text();
            if let Some(ref mut engine) = self.editor_state.ast_engine {
                engine.update_from_text(&code);
                self.editor_state.folding_ranges = engine.folding_ranges();
            }
        } else {
            self.editor_state.ast_engine = None;
            self.editor_state.folding_ranges.clear();
        }
    }

    /// S4 (Etapa 4): Actualiza diagnósticos y estado del cliente LSP.
    pub fn update_lsp(&mut self) {
        if self.current_language() != crate::syntax::Language::Rust {
            self.editor_state.diagnostics.clear();
            return;
        }

        if self.lsp_client.is_none() {
            let client = karkinos_core::lsp::LspClient::spawn("rust-analyzer", &[], &self.browse_dir)
                .unwrap_or_else(|_| karkinos_core::lsp::LspClient::mock());
            self.lsp_client = Some(client);
        }

        if let (Some(ref client), Some(ref path)) = (&self.lsp_client, &self.current_file) {
            let uri = format!("file://{}", path.display());
            let text = self.current_text();
            client.did_change(&uri, 2, &text);
            self.editor_state.diagnostics = client.diagnostics_for_file(&uri);

            if let Some(diag) = self.editor_state.diagnostics.iter().find(|d| d.range.start.line == self.editor_state.cursor_line) {
                self.editor_state.hover_text = Some(diag.message.clone());
                self.status = format!("LSP: {}", diag.message);
            }
        }
    }

    /// S4 (Etapa 5): Ejecuta un comando registrado por plugins sobre el documento activo.
    pub fn execute_plugin_command(&mut self, cmd_id: &str) -> bool {
        let mut doc = karkinos_core::Document::new(self.active_tab);
        doc.file_path = self.current_file.clone();
        doc.set_text(&self.current_text());

        let mut ctx = karkinos_core::plugin::PluginContext::new();
        ctx.active_file = self.current_file.clone();

        let handled = self.plugin_manager.execute_command(cmd_id, &mut doc, &mut ctx);
        if handled {
            let new_text = doc.buffer.to_string();
            if new_text != self.current_text() {
                self.set_text(&new_text);
                self.dirty = true;
            }
            if let Some(msg) = ctx.status_message {
                self.status = msg;
            }
        }
        handled
    }

    /// S1 (Rust IDE): Ejecuta un comando de Cargo en el directorio de trabajo del proyecto
    pub fn run_cargo(&mut self, action: karkinos_core::cargo::CargoAction) {
        let dir = self.current_file
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.browse_dir)
            .to_path_buf();
        self.cargo_panel_open = true;
        let _ = self.cargo_runner.spawn(action, &dir, &[]);
    }

    /// S1 (Rust IDE): Salta a la ubicación de un diagnóstico de Cargo en el editor
    pub fn jump_to_cargo_diagnostic(&mut self, diag: &karkinos_core::cargo::CargoDiagnostic) {
        if let Some(ref path) = diag.file {
            let full_path = if path.is_absolute() {
                path.clone()
            } else {
                self.browse_dir.join(path)
            };
            self.open_path(full_path);
            self.editor_state.cursor_line = diag.line;
            self.editor_state.cursor_col = diag.col;
            self.editor_state.needs_autoscroll = true;
            self.status = format!("Jumped to {}: line {}", diag.level, diag.line + 1);
        }
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

    /// S3 (revisión 2026-10-29): restaura el texto de un paso undo/redo
    /// SIN resetear la vista. set_text sirve a abrir/nuevo documento
    /// (arrancar de arriba es correcto ahí), pero para deshacer el
    /// salto al principio del documento es una regresión: se conserva
    /// el cursor (clampeado al nuevo buffer) y el scroll (la caché del
    /// widget lo recorta al nuevo máx en el próximo frame).
    fn restore_text_keeping_view(&mut self, text: &str) {
        let (line, col, scroll) = (
            self.editor_state.cursor_line,
            self.editor_state.cursor_col,
            self.editor_state.scroll,
        );
        self.set_text(text);
        self.editor_state.scroll = scroll;
        let last_line = self.lines.len().saturating_sub(1);
        if line > last_line {
            // La línea vieja ya no existe: el contenido Ty se ajusta a
            // que el cursor aterrice al FIN de la última línea nueva.
            self.editor_state.cursor_line = last_line;
            self.editor_state.cursor_col =
                self.lines[last_line].chars().count();
        } else {
            self.editor_state.cursor_line = line;
            let line_len = self.lines[line].chars().count();
            self.editor_state.cursor_col = col.min(line_len);
        }
    }

    pub fn undo(&mut self) {
        self.snapshot_if_changed();
        self.chunk_tracker.reset();
        if let Some(edit) = self.undo_stack.undo() {
            let snap = edit.before.clone();
            let text = snap.to_string();
            self.restore_text_keeping_view(&text);
            self.last_snapshot = snap;
            self.status = "Undone".to_string();
        } else {
            self.status = "Nothing to undo".to_string();
        }
    }

    pub fn redo(&mut self) {
        self.chunk_tracker.reset();
        if let Some(edit) = self.undo_stack.redo() {
            let snap = edit.after.clone();
            let text = snap.to_string();
            self.restore_text_keeping_view(&text);
            self.last_snapshot = snap;
            self.status = "Redone".to_string();
        } else {
            self.status = "Nothing to redo".to_string();
        }
    }

    // ---- S2: búsqueda y reemplazo ----

    /// Recalcula find_matches recorriendo todas las líneas.
    pub fn find_update_matches(&mut self) {
        self.find_matches.clear();
        if self.find_query.is_empty() {
            self.find_current = 0;
            return;
        }
        let query = if self.find_case_sensitive {
            self.find_query.clone()
        } else {
            self.find_query.to_lowercase()
        };
        for (li, line) in self.lines.iter().enumerate() {
            let hay = if self.find_case_sensitive {
                line.clone()
            } else {
                line.to_lowercase()
            };
            let qlen = query.chars().count();
            let hchars: Vec<char> = hay.chars().collect();
            let qchars: Vec<char> = query.chars().collect();
            if qlen == 0 || hchars.len() < qlen {
                continue;
            }
            for start in 0..=(hchars.len() - qlen) {
                if hchars[start..start + qlen] == qchars[..] {
                    self.find_matches.push((li, start));
                }
            }
        }
        if self.find_current >= self.find_matches.len() {
            self.find_current = 0;
        }
    }

    /// Navega al match actual (mueve cursor + autoscroll).
    pub fn find_goto_current(&mut self) {
        if let Some(&(line, col)) = self.find_matches.get(self.find_current) {
            let end_col = col + self.find_query.chars().count();
            self.editor_state.sel_anchor = Some((line, col));
            self.editor_state.cursor_line = line;
            self.editor_state.cursor_col = end_col;
            self.editor_state.request_autoscroll();
        }
    }

    /// Siguiente match (envuelve al principio).
    pub fn find_next(&mut self) {
        if self.find_matches.is_empty() {
            return;
        }
        self.find_current = (self.find_current + 1) % self.find_matches.len();
        self.find_goto_current();
    }

    /// Match anterior (envuelve al final).
    pub fn find_prev(&mut self) {
        if self.find_matches.is_empty() {
            return;
        }
        if self.find_current == 0 {
            self.find_current = self.find_matches.len() - 1;
        } else {
            self.find_current -= 1;
        }
        self.find_goto_current();
    }

    /// Reemplaza el match actual y avanza al siguiente.
    pub fn find_replace_current(&mut self) {
        if self.find_matches.is_empty() {
            return;
        }
        self.snapshot_if_changed();
        if let Some(&(line, col)) = self.find_matches.get(self.find_current) {
            let qlen = self.find_query.chars().count();
            let sb = crate::editor_widget::char_to_byte_pub(&self.lines[line], col);
            let eb = crate::editor_widget::char_to_byte_pub(&self.lines[line], col + qlen);
            self.lines[line].replace_range(sb..eb, &self.find_replace);
            self.dirty = true;
            self.recalc_view_bytes();
            self.snapshot_if_changed();
            self.find_update_matches();
            if !self.find_matches.is_empty() {
                if self.find_current >= self.find_matches.len() {
                    self.find_current = 0;
                }
                self.find_goto_current();
            }
        }
    }

    /// Reemplaza TODOS los matches de una vez (de atrás hacia adelante
    /// para no invalidar las posiciones).
    pub fn find_replace_all(&mut self) {
        if self.find_matches.is_empty() {
            return;
        }
        self.snapshot_if_changed();
        let qlen = self.find_query.chars().count();
        let count = self.find_matches.len();
        // Reemplazar de atrás hacia adelante evita recalcular offsets.
        for &(line, col) in self.find_matches.iter().rev() {
            let sb = crate::editor_widget::char_to_byte_pub(&self.lines[line], col);
            let eb = crate::editor_widget::char_to_byte_pub(&self.lines[line], col + qlen);
            self.lines[line].replace_range(sb..eb, &self.find_replace);
        }
        self.dirty = true;
        self.recalc_view_bytes();
        self.snapshot_if_changed();
        self.find_update_matches();
        self.status = format!("Replaced {} occurrences", count);
    }

    /// Abre `path` directamente (usado por el explorador de
    /// carpetas). Después de abrir, el campo "Archivo" solo muestra
    /// el nombre — la ruta completa se ve en la barra de abajo.
    pub fn open_path(&mut self, path: PathBuf) {
        // S8 (Etapa 2): Si ya está abierto en alguna pestaña, activarla
        self.sync_active_to_tab();
        if let Some(pos) = self.tabs.iter().position(|t| t.current_file.as_ref() == Some(&path)) {
            self.switch_tab(pos);
            self.status = "File already open in tab".to_string();
            return;
        }

        // Si la pestaña actual está intacta y vacía, la reutilizamos; si no, creamos nueva pestaña
        let can_reuse_current = self.current_file.is_none() && !self.dirty && self.lines == [""];
        if !can_reuse_current {
            let new_tab = TabDocument::new(self.next_tab_id);
            self.next_tab_id += 1;
            self.tabs.push(new_tab);
            self.active_tab = self.tabs.len() - 1;
        }

        match Self::read_file(&path) {
            Ok((content, ending, mtime)) => {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    self.file_path_input = name.to_string();
                }
                self.current_file = Some(path);
                self.set_text(&content);
                self.last_snapshot = Rope::from_str(&content);
                self.line_ending = ending;
                self.last_disk_mtime = mtime;
                self.external_changed = false;
                self.last_mtime_check = None;
                self.dirty = false;
                self.status = "File opened".to_string();
                self.sync_active_to_tab();
            }
            Err(e) => {
                self.status = format!("Error opening file: {}", e);
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
    #[allow(dead_code)]
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
    /// Si el campo se dejó en blanco o contiene solo espacios, guarda automáticamente como `nuevo.txt`
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
    /// de cierre no debe pegar cerrar solo).
    pub fn save(&mut self) -> bool {
        if self.save_progress.is_some() {
            return false; // ya hay un guardado en curso, no arranquemos otro encima
        }

        self.snapshot_if_changed();
        let path = if let Some(p) = &self.current_file {
            p.clone()
        } else if self.file_path_input.is_empty() {
            // Archivo nuevo sin nombre: pide a default_new_file_path
            // el primero libre "nuevo.txt"/"nuevo (N).txt" en la
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

    // ---- S8: nuevo documento ----

    /// S8: Limpia el editor para trabajar en un nuevo documento: buffer
    /// vacío, sin archivo asociado, sin cambios sin guardar, y resetea el
    /// historial de deshacer.
    pub fn new_document(&mut self) {
        self.set_text("");
        self.current_file = None;
        self.file_path_input.clear();
        self.dirty = false;
        self.last_snapshot = Rope::from_str("");
        self.last_disk_mtime = None;
        self.line_ending = LineEnding::Lf;
        self.undo_stack = UndoStack::new();
        self.chunk_tracker.reset();
        self.find_matches.clear();
        self.status = "New document".to_string();
        self.sync_active_to_tab();
    }

    /// S8: Intenta crear un nuevo documento. Si el buffer actual está sucio
    /// o guardándose, pide confirmación previa en lugar de descartar datos.
    pub fn try_new_document(&mut self) {
        if self.dirty || self.save_progress.is_some() {
            self.new_doc_pending = true;
        } else {
            self.new_document();
        }
    }

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

    /// Reabre el archivo del disco actual (botón "Recargar" de la
    /// barra). NO hace nada si hay cambios sin guardar: pisar el
    /// buffer perdería trabajo del usuario.
    pub fn reload(&mut self) {
        let Some(path) = self.current_file.clone() else {
            return;
        };
        if self.dirty {
            self.status = "Unsaved changes present: please save before reloading".to_string();
            return;
        }
        self.open_path(path);
    }

    /// S10: cambia el tamaño de la fuente del editor un paso (zoom in
    /// / out con clamp 8..32) y lo informa en la barra de estado. El
    /// re-layout (wrap_chars, scroll, gutter) cae solo: el widget
    /// mide el ancho/alto de glifo en cada frame con el FontId nuevo.
    pub fn zoom(&mut self, zoom_in: bool) {
        let next = apply_zoom(self.font_size, zoom_in);
        if (next - self.font_size).abs() < f32::EPSILON {
            return; // ya en el tope: no spamear el status
        }
        self.font_size = next;
        self.status = format!("Zoom: {} px", next.round());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- S10: Zoom de fuente ----

    #[test]
    fn zoom_reseta_tamano_y_respeta_el_clamp() {
        const EPS: f32 = 0.001;
        // Zoom in avanza un paso...
        assert!((apply_zoom(14.0, true) - 15.0).abs() < EPS);
        // ...y zoom out retrocede.
        assert!((apply_zoom(14.0, false) - 13.0).abs() < EPS);
        // Clamp por abajo: en 8.0 ya no baja más.
        assert!((apply_zoom(8.0, false) - 8.0).abs() < EPS);
        // Clamp por arriba: en 32.0 ya no sube más.
        assert!((apply_zoom(32.0, true) - 32.0).abs() < EPS);
        // Fuera de rango por valores espurio: se recorta al entrar.
        assert!((apply_zoom(50.0, true) - 32.0).abs() < EPS);
        assert!((apply_zoom(1.0, false) - 8.0).abs() < EPS);
    }

    #[test]
    fn zoom_redondea_a_pasos_enteros() {
        const EPS: f32 = 0.001;
        assert!((apply_zoom(14.3, true) - 15.0).abs() < EPS);
        assert!((apply_zoom(14.3, false) - 13.0).abs() < EPS);
    }

    #[test]
    fn zoom_metodo_actualiza_font_size_y_avisa_en_status() {
        const EPS: f32 = 0.001;
        let mut app = EditorApp::default();
        let base = app.font_size;
        app.zoom(true);
        assert!((app.font_size - (base + 1.0)).abs() < EPS);
        assert!(app.status.contains("Zoom"));

        app.zoom(false);
        assert!((app.font_size - base).abs() < EPS);

        // sincroniza persistence: el payload que se guarda y se lee
        // del Storage es exactamente el tamaño actual.
        app.font_size = 17.5;
        assert_eq!(font_size_to_string(app.font_size), "17.5");
        assert_eq!(font_size_from_string("17.5"), Some(17.5));
        assert_eq!(font_size_from_string("basura"), None);

        // S10: clamp también al cargar: un Storage corrupto machaca
        // la fuente inválida en lugar de crashear o dibujar gigante.
        app.font_size = font_size_from_string("999.0").unwrap_or_default();
        assert!((app.font_size - ZOOM_MAX).abs() < EPS);
    }



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
    fn cerrar_guardando_sin_ruta_guarda_como_nuevo_txt_y_cierra()
    -> std::io::Result<()> {
        let dir = std::env::temp_dir()
            .join(format!("karkinos_close_default_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let _ = std::fs::remove_file(dir.join("untitled.txt"));

        let mut app = EditorApp::default();
        app.browse_dir = dir.clone();
        app.dirty = true;
        // Sin ruta y sin archivo: save() ELIGE "untitled.txt" como
        // nombre default (bugfix 2026-10-29), así que el diálogo
        // ahora sí puede prometer el cierre: el guardado arranca
        // sobre un destino concreto en vez de abortar.
        app.confirm_close_save();
        assert!(!app.close_pending, "el diálogo sí puede cerrar: el save arrancó");
        assert!(app.close_after_save);
        // El destino queda fijado a dir/untitled.txt antes del async.
        assert_eq!(app.current_file, Some(dir.join("untitled.txt")));
        if let Some(progress) = &app.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let _ = std::fs::remove_file(dir.join("untitled.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
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

    // ---- S3: undo/redo NO deben saltar al principio del documento ----

    #[test]
    fn undo_y_redo_conservan_cursor_y_scroll_en_su_lugar() {
        let mut app = EditorApp::default();
        app.set_text("uno\ndos\ntres");
        app.snapshot_if_changed();

        // Simulo que el usuario tipeó más y la vista se movió abajo.
        app.set_text("uno\ndos\ntres\nCUATRO");
        app.snapshot_if_changed();
        app.editor_state.cursor_line = 1;
        app.editor_state.cursor_col = 1;
        app.editor_state.scroll = 99.0;

        app.undo();
        // El texto vuelve al paso anterior...
        assert_eq!(app.current_text(), "uno\ndos\ntres");
        // ...pero la vista NO se resetea a (0,0) / scroll 0.
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 1);
        assert!((app.editor_state.scroll - 99.0).abs() < f32::EPSILON);

        app.redo();
        assert_eq!(app.current_text(), "uno\ndos\ntres\nCUATRO");
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 1);
        assert!((app.editor_state.scroll - 99.0).abs() < f32::EPSILON);
    }

    #[test]
    fn undo_restaura_cursor_clamp_fuera_del_buffer_reparado() {
        let mut app = EditorApp::default();
        app.set_text("sola una linea");
        app.snapshot_if_changed();
        app.set_text("uno\ndos\ntres");
        app.snapshot_if_changed();
        // Captura el estado del cursor fuera de rango (para ver como
        // aterrizaría sobre el buffer deshecho de UNA sola línea).
        app.editor_state.cursor_line = 2;
        app.editor_state.scroll = 40.0;

        app.undo();
        // El buffer deshecho tiene UNA línea: la 2 no existe y el
        // cursor aterriza al FIN de esa línea, no crashea ni va a 0.
        assert_eq!(app.current_text(), "sola una linea");
        assert_eq!(app.editor_state.cursor_line, 0);
        assert_eq!(app.editor_state.cursor_col, "sola una linea".chars().count());
        assert!((app.editor_state.scroll - 40.0).abs() < f32::EPSILON);
    }


    // ---- Bugfix 2026-10-29: guardar archivo nuevo sin nombre ----

    #[test]
    fn nombre_default_nuevo_txt_y_sin_pisar_colisiones() {
        use std::collections::HashSet;
        use std::path::{Path, PathBuf};

        let dir = Path::new("/tmp/karkinos_test");
        fn mk_exists<'a>(
            existentes: &'a HashSet<&'static str>,
        ) -> impl Fn(&Path) -> bool + 'a {
            move |p: &Path| existentes.contains(p.file_name().and_then(|n| n.to_str()).unwrap_or(""))
        }

        // Caso feliz: carpeta libre → dir/untitled.txt
        assert_eq!(
            default_new_file_path(dir, &mk_exists(&HashSet::new())),
            PathBuf::from("/tmp/karkinos_test/untitled.txt")
        );

        // Colisión con untitled.txt → untitled (1).txt
        let mut existentes = HashSet::new();
        existentes.insert("untitled.txt");
        assert_eq!(
            default_new_file_path(dir, &mk_exists(&existentes)),
            PathBuf::from("/tmp/karkinos_test/untitled (1).txt")
        );

        // Colisión en cadena → cuenta mientras el nombre exista.
        existentes.insert("untitled (1).txt");
        existentes.insert("untitled (2).txt");
        assert_eq!(
            default_new_file_path(dir, &mk_exists(&existentes)),
            PathBuf::from("/tmp/karkinos_test/untitled (3).txt")
        );
    }

    #[test]
    fn save_con_input_vacio_guarda_como_nuevo_txt() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_save_default_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // limpio cualquier resto de corridas anteriores
        let _ = std::fs::remove_file(dir.join("untitled.txt"));

        let mut app = EditorApp::default();
        app.browse_dir = dir.clone();
        app.set_text("contenido sin nombre");
        app.dirty = true;
        // current_file None y file_path_input vacío: el caso reportado.
        app.file_path_input.clear();

        assert!(app.save());
        // Elige dir/untitled.txt y muestra en el input el nombre elegido.
        assert_eq!(app.current_file, Some(dir.join("untitled.txt")));
        assert_eq!(app.file_path_input, "untitled.txt");
        // Espera acotado al guardado asincrónico hasta que el disco
        // tenga el contenido completo.
        use std::io::Read;
        let path = dir.join("untitled.txt");
        let mut contenido = String::new();
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            if let Ok(mut f) = std::fs::File::open(&path) {
                f.read_to_string(&mut contenido)?;
                if contenido.contains("contenido sin nombre") {
                    break;
                }
            }
        }
        assert!(contenido.contains("contenido sin nombre"), "el archivo necesita terminar el guardado async");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }


    #[test]
    fn picker_se_queda_abierto_solo_si_el_click_es_adentro() {
        use egui::{pos2, Rect};

        let popup = Rect::from_min_max(pos2(100.0, 100.0), pos2(300.0, 400.0));
        let button = Rect::from_min_max(pos2(10.0, 10.0), pos2(80.0, 30.0));

        // Click adentro del popup (navegó una carpeta): sigue abierto.
        assert!(picker_should_stay_open(pos2(150.0, 200.0), Some(popup), Some(button)));
        // Click sobre el botón de toggle: él mismo maneja su estado.
        assert!(picker_should_stay_open(pos2(40.0, 20.0), Some(popup), Some(button)));
        // Click bien lejos (editor, otro lado de la ventana): cerrar.
        assert!(!picker_should_stay_open(pos2(700.0, 500.0), Some(popup), Some(button)));
        // Sin rects conocidos aún: cerrar (estado seguro).
        assert!(!picker_should_stay_open(pos2(150.0, 200.0), None, None));
    }

    // ---- S2: Buscar y reemplazar (Ctrl+F) ----

    #[test]
    fn buscar_coincidencias_case_sensitive_e_insensitive() {
        let mut app = EditorApp::default();
        app.set_text("Hola mundo\nhola otro mundo\nHOLA final");

        // Case-insensitive (default)
        app.find_query = "hola".to_string();
        app.find_case_sensitive = false;
        app.find_update_matches();
        assert_eq!(app.find_matches, vec![(0, 0), (1, 0), (2, 0)]);

        // Case-sensitive
        app.find_case_sensitive = true;
        app.find_update_matches();
        assert_eq!(app.find_matches, vec![(1, 0)]);

        // Sin query: lista vacía
        app.find_query = "".to_string();
        app.find_update_matches();
        assert!(app.find_matches.is_empty());
    }

    #[test]
    fn navegacion_siguiente_anterior_find_matches() {
        let mut app = EditorApp::default();
        app.set_text("uno\nuno\nuno");
        app.find_query = "uno".to_string();
        app.find_update_matches();
        assert_eq!(app.find_matches.len(), 3);
        assert_eq!(app.find_current, 0);

        app.find_next();
        assert_eq!(app.find_current, 1);
        app.find_next();
        assert_eq!(app.find_current, 2);
        // Wrap around
        app.find_next();
        assert_eq!(app.find_current, 0);

        // Prev wrap around
        app.find_prev();
        assert_eq!(app.find_current, 2);
        app.find_prev();
        assert_eq!(app.find_current, 1);
    }

    #[test]
    fn reemplazo_actual_y_reemplazo_todo_con_undo() {
        let mut app = EditorApp::default();
        app.set_text("gato y perro y gato");
        app.find_query = "gato".to_string();
        app.find_replace = "lobo".to_string();
        app.find_update_matches();
        assert_eq!(app.find_matches.len(), 2);

        // Reemplazar primer match
        app.find_replace_current();
        assert_eq!(app.current_text(), "lobo y perro y gato");

        // Reemplazar todo lo que queda
        app.find_replace_all();
        assert_eq!(app.current_text(), "lobo y perro y lobo");

        // Undo revierte el replace all
        app.undo();
        assert_eq!(app.current_text(), "lobo y perro y gato");

        // Undo revierte el replace individual
        app.undo();
        assert_eq!(app.current_text(), "gato y perro y gato");
    }

    // ---- S8: Botón Nuevo documento ----

    #[test]
    fn nuevo_documento_resetea_buffer_y_metadatos() {
        let mut app = EditorApp::default();
        app.set_text("texto previo");
        app.current_file = Some(PathBuf::from("dummy.txt"));
        app.file_path_input = "dummy.txt".to_string();
        app.dirty = true;
        app.new_document();

        assert_eq!(app.current_text(), "");
        assert_eq!(app.lines, vec![String::new()]);
        assert_eq!(app.current_file, None);
        assert_eq!(app.file_path_input, "");
        assert!(!app.dirty);
        assert_eq!(app.status, "New document");
    }

    #[test]
    fn try_nuevo_documento_con_dirty_pide_confirmacion() {
        let mut app = EditorApp::default();
        app.dirty = true;
        app.try_new_document();
        assert!(app.new_doc_pending);

        // Sin cambios: crea de una sin pedir confirmación
        let mut app_clean = EditorApp::default();
        app_clean.try_new_document();
        assert!(!app_clean.new_doc_pending);
    }

    // ---- S8 (Etapa 2): Pestañas / Múltiples documentos ----

    #[test]
    fn tabs_creacion_cambio_y_cierre() {
        let mut app = EditorApp::default();
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_tab, 0);

        app.set_text("Texto en pestaña 1");
        app.dirty = true;

        app.new_tab();
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);
        assert_eq!(app.current_text(), "");

        app.set_text("Texto en pestaña 2");
        app.switch_tab(0);
        assert_eq!(app.active_tab, 0);
        assert_eq!(app.current_text(), "Texto en pestaña 1");

        app.switch_tab(1);
        assert_eq!(app.active_tab, 1);
        assert_eq!(app.current_text(), "Texto en pestaña 2");

        app.close_tab(0);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_tab, 0);
        assert_eq!(app.current_text(), "Texto en pestaña 2");
    }

    #[test]
    fn tabs_open_path_no_duplica_y_reutiliza_vacia() -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path_a = dir.join(format!("karkinos_tab_a_{}.txt", std::process::id()));
        let path_b = dir.join(format!("karkinos_tab_b_{}.txt", std::process::id()));
        std::fs::write(&path_a, "archivo A")?;
        std::fs::write(&path_b, "archivo B")?;

        let mut app = EditorApp::default();
        // Abrir en pestaña inicial limpia debe reutilizarla
        app.open_path(path_a.clone());
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.current_file, Some(path_a.clone()));

        // Abrir segundo archivo debe crear pestaña nueva
        app.open_path(path_b.clone());
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);
        assert_eq!(app.current_file, Some(path_b.clone()));

        // Abrir el archivo A nuevamente debe cambiar a la pestaña 0 sin duplicar
        app.open_path(path_a.clone());
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 0);
        assert_eq!(app.current_file, Some(path_a.clone()));

        std::fs::remove_file(path_a)?;
        std::fs::remove_file(path_b)?;
        Ok(())
    }

    // ---- Guardado de archivo sin nombre con diálogo modal ----

    #[test]
    fn try_save_sin_nombre_abre_modal_y_guarda_con_nombre_o_default() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_try_save_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;

        let mut app = EditorApp::default();
        app.browse_dir = dir.clone();
        app.set_text("codigo de prueba");
        app.dirty = true;

        // 1. try_save() en archivo sin nombre debe abrir el modal y no guardar aún
        assert!(!app.try_save());
        assert!(app.save_name_pending);

        // 2. Confirmar con nombre "mi_programa.rs"
        app.save_name_input = "mi_programa.rs".to_string();
        assert!(app.confirm_save_name());
        assert!(!app.save_name_pending);
        assert_eq!(app.current_file, Some(dir.join("mi_programa.rs")));
        assert_eq!(app.file_path_input, "mi_programa.rs");

        // Esperar a que termine el guardado asíncrono
        if let Some(progress) = &app.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        // 3. Otro documento sin nombre: dejar en blanco debe asignar "untitled.txt"
        let mut app2 = EditorApp::default();
        app2.browse_dir = dir.clone();
        app2.set_text("otro texto");
        app2.dirty = true;
        assert!(!app2.try_save());
        assert!(app2.save_name_pending);

        // Dejar vacío
        app2.save_name_input = "   ".to_string();
        assert!(app2.confirm_save_name());
        assert!(!app2.save_name_pending);
        assert_eq!(app2.current_file, Some(dir.join("untitled.txt")));
        assert_eq!(app2.file_path_input, "untitled.txt");

        if let Some(progress) = &app2.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let _ = std::fs::remove_file(dir.join("mi_programa.rs"));
        let _ = std::fs::remove_file(dir.join("untitled.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn test_tabs_cycling_and_dirty_detection() {
        let mut app = EditorApp::default();
        app.new_tab();
        app.new_tab();
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.active_tab, 2);

        // Ciclar adelante
        app.next_tab();
        assert_eq!(app.active_tab, 0);
        app.next_tab();
        assert_eq!(app.active_tab, 1);

        // Ciclar atrás
        app.prev_tab();
        assert_eq!(app.active_tab, 0);
        app.prev_tab();
        assert_eq!(app.active_tab, 2);

        // Detección de pestañas sucias
        assert_eq!(app.dirty_tabs_count(), 0);
        assert_eq!(app.first_dirty_tab_index(), None);

        // Marcar pestaña 1 como sucia
        app.tabs[1].dirty = true;
        assert_eq!(app.dirty_tabs_count(), 1);
        assert_eq!(app.first_dirty_tab_index(), Some(1));

        // Si la activa (2) también está sucia, first_dirty_tab_index debe priorizar la activa
        app.dirty = true;
        assert_eq!(app.dirty_tabs_count(), 2);
        assert_eq!(app.first_dirty_tab_index(), Some(2));
    }

    #[test]
    fn test_save_as_changes_target_file() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_save_as_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;

        let mut app = EditorApp::default();
        app.browse_dir = dir.clone();
        app.set_text("codigo inicial");
        app.current_file = Some(dir.join("archivo_original.txt"));
        app.file_path_input = "archivo_original.txt".to_string();

        app.save_as();
        assert!(app.save_name_pending);
        assert_eq!(app.save_name_input, "archivo_original.txt");

        app.save_name_input = "archivo_copia.txt".to_string();
        assert!(app.confirm_save_name());
        assert_eq!(app.current_file, Some(dir.join("archivo_copia.txt")));

        if let Some(progress) = &app.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let _ = std::fs::remove_file(dir.join("archivo_copia.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn test_editor_app_ast_engine_and_folding_ranges() {
        let mut app = EditorApp::default();
        app.current_file = Some(PathBuf::from("main.rs"));
        app.set_text("fn calculate() -> i32 {\n    let a = 10;\n    a * 2\n}");

        assert!(app.editor_state.ast_engine.is_some());
        assert!(!app.editor_state.folding_ranges.is_empty());
        let has_func = app.editor_state.folding_ranges.iter().any(|r| r.start_line == 0 && r.end_line == 3);
        assert!(has_func, "should detect function folding range");
    }

    #[test]
    fn test_editor_app_lsp_diagnostics() {
        use karkinos_core::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut app = EditorApp::default();
        let path = PathBuf::from("main.rs");
        app.current_file = Some(path);

        let client = LspClient::mock();
        client.set_diagnostics_for_file(
            "file://main.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 0, character: 3 },
                    end: LspPosition { line: 0, character: 7 },
                },
                severity: Some(DiagnosticSeverity::Error),
                code: Some("E0433".to_string()),
                source: Some("rustc".to_string()),
                message: "failed to resolve: use of undeclared crate".to_string(),
            }],
        );

        app.lsp_client = Some(client);
        app.update_lsp();

        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].severity, Some(DiagnosticSeverity::Error));
        assert!(app.status.contains("failed to resolve"));
    }

    #[test]
    fn test_editor_app_plugin_execution() {
        let mut app = EditorApp::default();
        app.set_text("rust programming language");

        let handled = app.execute_plugin_command("uppercase_document");
        assert!(handled);
        assert_eq!(app.current_text(), "RUST PROGRAMMING LANGUAGE");
        assert!(app.status.contains("UPPERCASE"));

        let word_count_handled = app.execute_plugin_command("word_count");
        assert!(word_count_handled);
        assert!(app.status.contains("3 words"));
    }

    #[test]
    fn test_tab_switching_preserves_lsp_singleton_and_ast() {
        use karkinos_core::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut app = EditorApp::default();
        app.current_file = Some(PathBuf::from("tab1.rs"));
        app.set_text("fn tab_one() {}");

        let client = LspClient::mock();
        client.set_diagnostics_for_file(
            "file://tab1.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 2 },
                },
                severity: Some(DiagnosticSeverity::Warning),
                code: None,
                source: Some("rustc".to_string()),
                message: "tab1 warning".to_string(),
            }],
        );
        client.set_diagnostics_for_file(
            "file://tab2.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 2 },
                },
                severity: Some(DiagnosticSeverity::Error),
                code: None,
                source: Some("rustc".to_string()),
                message: "tab2 error".to_string(),
            }],
        );
        app.lsp_client = Some(client);
        app.update_lsp();
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab1 warning");

        // Create tab 2 with Rust code
        app.new_tab();
        app.current_file = Some(PathBuf::from("tab2.rs"));
        app.set_text("fn tab_two() {}");
        app.update_lsp();
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab2 error");

        // Switching back to tab 1 must NOT drop LSP client and must load tab1 diagnostics
        app.switch_tab(0);
        assert!(app.lsp_client.is_some(), "LSP client singleton must remain alive");
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab1 warning");
        assert!(app.editor_state.ast_engine.is_some(), "AST engine must remain cached");

        // Switching to tab 2 must also preserve LSP client and load tab2 diagnostics
        app.switch_tab(1);
        assert!(app.lsp_client.is_some(), "LSP client singleton must remain alive");
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab2 error");
        assert!(app.editor_state.ast_engine.is_some(), "AST engine must remain cached");
    }

    #[test]
    fn test_editor_app_cargo_runner_and_navigation() {
        use karkinos_core::cargo::CargoDiagnostic;

        let mut app = EditorApp::default();
        assert!(!app.cargo_panel_open);

        let diag = CargoDiagnostic {
            level: "error".to_string(),
            message: "mismatched types".to_string(),
            file: Some(PathBuf::from("test_target.rs")),
            line: 7,
            col: 4,
            rendered: "error[E0308]: mismatched types".to_string(),
        };

        app.jump_to_cargo_diagnostic(&diag);
        assert_eq!(app.editor_state.cursor_line, 7);
        assert_eq!(app.editor_state.cursor_col, 4);
        assert!(app.editor_state.needs_autoscroll);
        assert!(app.status.contains("Jumped to error: line 8"));
    }
}
