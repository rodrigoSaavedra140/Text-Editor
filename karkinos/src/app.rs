use std::path::PathBuf;

use eframe::egui;

use crate::editor_widget::EditorState;
use crate::document_service::{DocumentService, TabDocument};
use karkinos_core::LineEnding;
use ropey::Rope;
use crate::undo::UndoStack;



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



pub struct EditorApp {
    // S5a (Roadmap v0.2.9): servicio de documento (buffer, pestañas,
    // ciclo de vida de archivos). Toda la lógica pura vive en
    // document_service.rs; app.rs orquesta con presentación/LSP/AST.
    pub doc: crate::document_service::DocumentService,
    // S5b (Roadmap v0.2.9): servicio LSP (conexión + protocolo de pedidos).
    pub lsp: crate::lsp_service::LspService,
    // S5c (Roadmap v0.2.9): servicio de ejecución de Cargo (subproceso + panel).
    pub cargo: crate::cargo_service::CargoService,
    // S5d (Roadmap v0.2.9): servicio del debugger DAP (sesión + breakpoints).
    pub dap: crate::dap_service::DapService,
    // S5 (Roadmap v0.2.25): servicio de terminal interactiva PTY.
    pub terminal: crate::terminal_service::TerminalService,
    pub editor_state: EditorState,
    // Textura del logo, cargada una sola vez en el primer frame
    // (None hasta ese momento).
    pub logo_texture: Option<egui::TextureHandle>,
    // S4: diálogo "Ir a línea" abierto (Ctrl+G) + su campo de texto.
    pub goto_pending: bool,
    pub goto_input: String,
    // Listado de la carpeta que se muestra en el desplegable,
    // cacheado: read_dir + abrir los primeros 8 KiB de cada archivo
    // era I/O de disco EN CADA FRAME con el desplegable abierto.
    // Se invalida al navegar a otra carpeta o cuando cambia el
    // mtime de la carpeta (archivos creados/borrados dentro).
    // S2: búsqueda y reemplazo (Ctrl+F).
    pub find_open: bool,
    pub find_query: String,
    pub find_replace: String,
    pub find_case_sensitive: bool,
    /// Posiciones de matches: (línea, columna-inicio en chars).
    pub find_matches: Vec<(usize, usize)>,
    /// Índice del match actual dentro de find_matches.
    pub find_current: usize,
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
    // S4 (Roadmap v0.2.9): servicios de fondo del Quick Open (indexador
    // + scorer). UNA pareja por app; sirve al modal del tab activo.
    pub quick_open_services: crate::quick_open::QuickOpenServices,
    // S4 (Etapa 4): Cliente Language Server Protocol (LSP)
    // S4 (Etapa 5): Gestor de extensiones y plugins
    pub plugin_manager: karkinos_core::plugin::PluginManager,
    pub extensions_modal_open: bool,
    pub extensions_tab: usize,
    // S3 (Rust IDE): Open VSX Marketplace
    pub open_vsx_query_input: String,
    pub open_vsx_results: Vec<karkinos_core::open_vsx::OpenVsxExtension>,
    pub open_vsx_total: usize,
    pub open_vsx_searching: bool,
    pub open_vsx_error: Option<String>,
    pub open_vsx_status: Option<String>,
    open_vsx_rx: Option<std::sync::mpsc::Receiver<Result<karkinos_core::open_vsx::OpenVsxSearchResult, String>>>,
    open_vsx_download_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    // S1 (Rust IDE): Ejecutor nativo de pipelines de Cargo
    // S4 (Rust IDE): Code Actions, Inlay Hints & Rename
    // S4 (Rust IDE): DAP Native Debugger & Breakpoints
    // S3 (Roadmap v0.2.9): worker de fondo para el parseo Tree-sitter.
    // Vive en EditorApp (UN worker por app, sirve al tab activo; al
    // cambiar de pestaña se re-pide el parseo del contenido cargado).
    pub ast_worker: crate::ast_worker::AstWorker,
    // Revisión monótonica de pedidos al worker (porisión de "qué snapshot")
    pub ast_request_rev: u64,
    // Última revisión del worker aplicada al estado — evita revivir
    // resultados viejos si llegan después de un parseo posterior.
    pub ast_applied_rev: u64,
    // F1 (Roadmap v0.2.27): worker de diff contra HEAD. El pedido se
    // dispara al abrir y al guardar (archivo limpio), nunca mientras se
    // escribe, para no correr `git` en cada tecla.
    pub git_diff_worker: crate::git_diff_worker::GitDiffWorker,
    // Última ruta para la que se pidió diff y si en el frame anterior el
    // buffer estaba sucio: juntos evitan pedidos repetidos.
    git_diff_requested_path: Option<std::path::PathBuf>,
    git_diff_was_dirty: bool,
    // F5 (Roadmap v0.2.31): split del área central. `None` = un solo panel.
    pub split: Option<SplitLayout>,
}

/// F5 (Roadmap v0.2.31): eje por el que se parte el área central.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

/// F5: estado del split. El panel primario sigue siendo `EditorApp.editor_state`
/// (con el tab activo); el secundario tiene su propio cursor y scroll sobre el
/// MISMO buffer. Comparar dos pestañas distintas queda fuera de alcance.
#[derive(Debug, Clone)]
pub struct SplitLayout {
    pub direction: SplitDirection,
    pub ratio: f32,
    pub secondary: EditorState,
}

impl Default for EditorApp {
    fn default() -> Self {
        EditorApp {
            editor_state: EditorState::default(),
            logo_texture: None,
            goto_pending: false,
            goto_input: String::new(),
            doc: DocumentService::default(),
            lsp: crate::lsp_service::LspService::default(),
            cargo: crate::cargo_service::CargoService::default(),
            dap: crate::dap_service::DapService::default(),
            terminal: crate::terminal_service::TerminalService::default(),
            find_open: false,
            find_query: String::new(),
            find_replace: String::new(),
            find_case_sensitive: false,
            find_matches: Vec::new(),
            find_current: 0,
            picker_open: false,
            picker_button_rect: None,
            font_size: 14.0,
            theme_mode: crate::theme::ThemeMode::Dark,
            quick_open: crate::quick_open::QuickOpenState::default(),
            quick_open_services: crate::quick_open::QuickOpenServices::default(),
            plugin_manager: karkinos_core::plugin::PluginManager::with_default_plugins(),
            extensions_modal_open: false,
            extensions_tab: 0,
            open_vsx_query_input: "rust".to_string(),
            open_vsx_results: Vec::new(),
            open_vsx_total: 0,
            open_vsx_searching: false,
            open_vsx_error: None,
            open_vsx_status: None,
            open_vsx_rx: None,
            open_vsx_download_rx: None,
            ast_worker: crate::ast_worker::AstWorker::spawn(),
            ast_request_rev: 0,
            ast_applied_rev: 0,
            git_diff_worker: crate::git_diff_worker::GitDiffWorker::spawn(),
            git_diff_requested_path: None,
            git_diff_was_dirty: false,
            split: None,
        }
    }
}

impl EditorApp {
    /// S5 (Etapa 2): Retorna la paleta de colores del tema activo.
    pub fn theme(&self) -> crate::theme::Theme {
        self.theme_mode.theme()
    }

    /// S6 (Etapa 2): Retorna el lenguaje detectado para el documento actual (autodetecta por extensión y contenido).
    pub fn current_language(&self) -> crate::syntax::Language {
        let path = self.doc.current_file.as_deref().or_else(|| {
            if self.doc.file_path_input.is_empty() {
                None
            } else {
                Some(std::path::Path::new(&self.doc.file_path_input))
            }
        });

        let sample = if self.doc.lines.len() <= 100 {
            self.doc.lines.join("\n")
        } else {
            self.doc.lines[..100].join("\n")
        };

        crate::syntax::Language::detect(path, &sample)
    }

    /// S8 (Etapa 2): Guarda el estado actual del editor en la pestaña activa.
    pub fn sync_active_to_tab(&mut self) {
        if let Some(tab) = self.doc.tabs.get_mut(self.doc.active_tab) {
            tab.lines = self.doc.lines.clone();
            tab.buffer = self.doc.buffer.clone();
            tab.editor_state = self.editor_state.clone();
            tab.last_snapshot = self.doc.last_snapshot.clone();
            tab.undo_stack = self.doc.undo_stack.clone();
            tab.file_path_input = self.doc.file_path_input.clone();
            tab.current_file = self.doc.current_file.clone();
            tab.dirty = self.doc.dirty;
            tab.line_ending = self.doc.line_ending;
            tab.last_disk_mtime = self.doc.last_disk_mtime;
            tab.external_changed = self.doc.external_changed;
            tab.last_mtime_check = self.doc.last_mtime_check;
            tab.view_bytes = self.doc.view_bytes;
        }
    }

    /// S2: Sincronización ligera de metadatos (dirty, view_bytes, cursor) sin clonar lines ni undo_stack.
    pub fn sync_active_metadata_to_tab(&mut self) {
        if let Some(tab) = self.doc.tabs.get_mut(self.doc.active_tab) {
            tab.dirty = self.doc.dirty;
            tab.view_bytes = self.doc.view_bytes;
            tab.current_file = self.doc.current_file.clone();
            tab.file_path_input = self.doc.file_path_input.clone();
            tab.external_changed = self.doc.external_changed;
            tab.editor_state.cursor_line = self.editor_state.cursor_line;
            tab.editor_state.cursor_col = self.editor_state.cursor_col;
            tab.editor_state.scroll = self.editor_state.scroll;
        }
    }

    /// S4: Avanza a la siguiente pestaña de manera cíclica.
    pub fn next_tab(&mut self) {
        if self.doc.tabs.len() > 1 {
            let next = (self.doc.active_tab + 1) % self.doc.tabs.len();
            self.switch_tab(next);
        }
    }

    /// S4: Retrocede a la pestaña anterior de manera cíclica.
    pub fn prev_tab(&mut self) {
        if self.doc.tabs.len() > 1 {
            let prev = if self.doc.active_tab == 0 {
                self.doc.tabs.len() - 1
            } else {
                self.doc.active_tab - 1
            };
            self.switch_tab(prev);
        }
    }

    /// S8 (Etapa 2): Carga los datos de la pestaña `idx` en el estado activo del editor.
    pub fn load_tab(&mut self, idx: usize) {
        if let Some(tab) = self.doc.tabs.get(idx) {
            self.doc.active_tab = idx;
            self.doc.lines = tab.lines.clone();
            self.doc.buffer = tab.buffer.clone();
            self.editor_state = tab.editor_state.clone();
            self.doc.last_snapshot = tab.last_snapshot.clone();
            self.doc.undo_stack = tab.undo_stack.clone();
            self.doc.file_path_input = tab.file_path_input.clone();
            self.doc.current_file = tab.current_file.clone();
            self.doc.dirty = tab.dirty;
            self.doc.line_ending = tab.line_ending;
            self.doc.last_disk_mtime = tab.last_disk_mtime;
            self.doc.external_changed = tab.external_changed;
            self.doc.last_mtime_check = tab.last_mtime_check;
            self.doc.view_bytes = tab.view_bytes;
            self.doc.chunk_tracker.reset();
            self.find_matches.clear();

            // S3 (Roadmap v0.2.9): el AST corre en un worker de fondo.
            // Al cambiar de pestaña SIEMPRE se re-pide el parseo del
            // contenido cargado (el "motor cacheado si no existía" del
            // diseño viejo dejó de existir: el worker no vive por tab).
            self.update_ast();

            // S4: Fast in-memory diagnostics lookup from existing singleton LSP client without process re-spawning
            if let (Some(ref client), Some(ref path)) = (&self.lsp.client, &self.doc.current_file) {
                let uri = karkinos_core::path_to_uri(path);
                self.editor_state.diagnostics = client.diagnostics_for_file(&uri);
            }

            self.sync_editor_breakpoints();
        }
    }

    /// S8 (Etapa 2): Cambia a la pestaña indicada, sincronizando la actual previamente.
    pub fn switch_tab(&mut self, new_idx: usize) {
        if new_idx == self.doc.active_tab || new_idx >= self.doc.tabs.len() {
            return;
        }
        self.sync_active_to_tab();
        self.load_tab(new_idx);
        self.doc.status = format!("Tab: {}", self.doc.tabs[self.doc.active_tab].display_title());
    }

    /// S8 (Etapa 2): Crea una nueva pestaña en blanco y la activa.
    pub fn new_tab(&mut self) {
        self.sync_active_to_tab();
        let new_tab = TabDocument::new(self.doc.next_tab_id);
        self.doc.next_tab_id += 1;
        self.doc.tabs.push(new_tab);
        self.load_tab(self.doc.tabs.len() - 1);
        self.doc.status = "New tab created".to_string();
    }

    /// S8 (Etapa 2): Cierra la pestaña en `idx`. Si es la única pestaña, la resetea en blanco.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.doc.tabs.len() {
            return;
        }

        if self.doc.tabs.len() == 1 {
            let reset_tab = TabDocument::new(self.doc.next_tab_id);
            self.doc.next_tab_id += 1;
            self.doc.tabs[0] = reset_tab;
            self.load_tab(0);
            return;
        }

        self.sync_active_to_tab();
        self.doc.tabs.remove(idx);

        if self.doc.active_tab >= self.doc.tabs.len() {
            self.doc.active_tab = self.doc.tabs.len() - 1;
        } else if idx < self.doc.active_tab {
            self.doc.active_tab -= 1;
        }

        self.load_tab(self.doc.active_tab);
    }


    /// S8 (Etapa 2): Intenta cerrar una pestaña. Si tiene cambios sin guardar
    /// y contenido real no vacío, pide confirmación estableciendo `tab_close_pending`.
    pub fn try_close_tab(&mut self, idx: usize) {
        if idx >= self.doc.tabs.len() {
            return;
        }

        let is_empty = if idx == self.doc.active_tab {
            self.doc.is_buffer_empty()
        } else {
            self.doc.tabs[idx].is_empty()
        };

        let is_dirty = if idx == self.doc.active_tab {
            self.doc.dirty
        } else {
            self.doc.tabs[idx].dirty
        };

        if is_dirty && !is_empty {
            self.doc.tab_close_pending = Some(idx);
        } else {
            self.close_tab(idx);
        }
    }


    /// Reemplaza el contenido completo del buffer a partir de un
    /// String (abrir archivo, deshacer, rehacer). También se usa
    /// solo en momentos puntuales, no por cada tecla.
    /// Invariante del buffer (S9): no hay pares '\r\n' — si el texto
    /// trae, se normalizan a '\n' (un '\r' suelto a mitad de línea se
    /// conserva: es contenido del texto, no un salto).
    pub fn set_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n");
        self.doc.buffer.set_text(&text);
        self.doc.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(|l| l.to_string()).collect()
        };
        self.editor_state.cursor_line = 0;
        self.editor_state.cursor_col = 0;
        self.editor_state.sel_anchor = None;
        self.editor_state.scroll = 0.0;
        self.doc.chunk_tracker.reset();
        self.doc.recalc_view_bytes();
        self.update_ast();
        self.update_lsp();
    }


    /// S3 (Roadmap v0.2.9): encarga el parseo del AST al worker de FONDO.
    /// Ya no corre tree-sitter en la UI: manda el snapshot del texto con
    /// una revisión nueva y el resultado llega por `poll_ast_results`.
    /// El nombre viejo (`update_ast`) se conserva porque todos los
    /// call-sites (set_text, apply_text_edits, load_tab, save) quieren
    /// exactamente esto: "el AST tiene que actualizarse con el texto
    /// actual, sin bloquear".
    pub fn update_ast(&mut self) {
        self.ast_request_rev = self.ast_request_rev.wrapping_add(1);
        if self.current_language() == crate::syntax::Language::Rust {
            let code = self.doc.current_text();
            self.ast_worker.request(self.ast_request_rev, &code);
        } else {
            // Lenguaje sin AST (genéricamente solo Rust): descarta los
            // spans semánticos que pudieran venir de un lenguaje anterior;
            // el worker no se despierta y el plegado se apaga.
            self.editor_state.ast_spans.clear();
            self.editor_state.folding_ranges.clear();
            self.ast_applied_rev = self.ast_request_rev;
            // S1 (Roadmap v0.2.9): la generación del AST cambió → la
            // cache de galleys se invalida al sincronizarse (sync_ast).
            self.editor_state.ast_rev = self.editor_state.ast_rev.wrapping_add(1);
        }
    }

    /// S3 (Roadmap v0.2.9): consume el resultado más fresco del worker,
    /// si llegó algo. Aplica spans + plegado al estado del editor y
    /// sube la generación del AST (invalida la cache de layouts de T1).
    /// Devuelve `true` cuando aplicó algo (la UI repinta suave).
    pub fn poll_ast_results(&mut self) -> bool {
        let Some(result) = self.ast_worker.try_recv_result() else {
            return false;
        };
        if result.revision < self.ast_applied_rev {
            return false; // resultado viejo: un pedido posterior ya corrió
        }
        self.ast_applied_rev = result.revision;
        self.editor_state.ast_spans = result.spans;
        self.editor_state.folding_ranges = result.folding;
        // F2 (Roadmap v0.2.28): encabezados pegajosos del mismo parseo.
        self.editor_state.sticky_scopes = result.scopes;
        // S1 (Roadmap v0.2.9): el AST cambió → nueva generación, la
        // cache de galleys se vacía en el próximo sync_ast del render.
        self.editor_state.ast_rev = self.editor_state.ast_rev.wrapping_add(1);
        true
    }

    /// F5 (Roadmap v0.2.31): abre, cambia de eje o cierra el split del área
    /// central. El panel nuevo arranca con el cursor del primario.
    pub fn toggle_split(&mut self, direction: SplitDirection) {
        match &self.split {
            Some(layout) if layout.direction == direction => self.split = None,
            _ => {
                let mut secondary = self.editor_state.clone();
                // Los cursores extra no se duplican: son del primario.
                secondary.extra_carets.clear();
                self.split = Some(SplitLayout {
                    direction,
                    ratio: 0.5,
                    secondary,
                });
            }
        }
    }

    /// F5: mueve el divisor del split dentro de límites razonables.
    pub fn set_split_ratio(&mut self, ratio: f32) {
        if let Some(layout) = &mut self.split {
            layout.ratio = ratio.clamp(0.15, 0.85);
        }
    }

    /// F1 (Roadmap v0.2.27): pide el diff contra HEAD cuando corresponde —
    /// al abrir un archivo (ruta distinta) o al terminar de guardar (pasó de
    /// sucio a limpio). Mientras se escribe no se pide nada, así que `git`
    /// nunca corre en el hilo de UI ni una vez por tecla.
    pub fn request_git_diff_if_needed(&mut self) {
        let Some(path) = self.doc.current_file.clone() else {
            return;
        };
        if self.doc.dirty {
            self.git_diff_was_dirty = true;
            return;
        }
        let path_changed = self.git_diff_requested_path.as_ref() != Some(&path);
        let just_saved = self.git_diff_was_dirty;
        if !path_changed && !just_saved {
            return;
        }
        self.git_diff_requested_path = Some(path.clone());
        self.git_diff_was_dirty = false;
        self.git_diff_worker.request(path, self.doc.current_text());
    }

    /// F1: consume los diffs que terminó el worker y los aplica al estado
    /// activo y a las pestañas abiertas de esa ruta. `true` si aplicó algo.
    pub fn poll_git_diff_results(&mut self) -> bool {
        let mut applied = false;
        while let Some(result) = self.git_diff_worker.try_recv_result() {
            if self.doc.current_file.as_ref() == Some(&result.path) {
                self.editor_state.git_diff = Some(std::sync::Arc::clone(&result.hunks));
                applied = true;
            }
            for tab in &mut self.doc.tabs {
                if tab.current_file.as_ref() == Some(&result.path) {
                    tab.editor_state.git_diff = Some(std::sync::Arc::clone(&result.hunks));
                    applied = true;
                }
            }
        }
        applied
    }

    /// S4 (Etapa 4): Actualiza diagnósticos y estado del cliente LSP.
    
    /// S4 (Etapa 5): Ejecuta un comando registrado por plugins sobre el documento activo.
    pub fn execute_plugin_command(&mut self, cmd_id: &str) -> bool {
        let mut doc = karkinos_core::Document::new(self.doc.active_tab);
        doc.file_path = self.doc.current_file.clone();
        doc.set_text(&self.doc.current_text());

        let mut ctx = karkinos_core::plugin::PluginContext::new();
        ctx.active_file = self.doc.current_file.clone();

        let handled = self.plugin_manager.execute_command(cmd_id, &mut doc, &mut ctx);
        if handled {
            let new_text = doc.buffer.to_string();
            if new_text != self.doc.current_text() {
                self.set_text(&new_text);
                self.doc.dirty = true;
            }
            if let Some(msg) = ctx.status_message {
                self.doc.status = msg;
            }
        }
        handled
    }

    /// Retorna el directorio de trabajo actual (directorio del archivo activo o browse_dir).
    pub fn current_working_dir(&self) -> std::path::PathBuf {
        self.doc
            .current_file
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.doc.browse_dir)
            .to_path_buf()
    }

    /// S1 (Rust IDE): Ejecuta un comando de Cargo en el directorio de trabajo del proyecto
    pub fn run_cargo(&mut self, action: karkinos_core::cargo::CargoAction) {
        let dir = self.current_working_dir();
        self.cargo.run(action, &dir);
    }

    /// S1 (Rust IDE): Salta a la ubicación de un diagnóstico de Cargo en el editor
    pub fn jump_to_cargo_diagnostic(&mut self, diag: &karkinos_core::cargo::CargoDiagnostic) {
        if let Some(ref path) = diag.file {
            let full_path = if path.is_absolute() {
                path.clone()
            } else {
                self.doc.browse_dir.join(path)
            };
            self.open_path(full_path);
            self.editor_state.cursor_line = diag.line;
            self.editor_state.cursor_col = diag.col;
            self.editor_state.needs_autoscroll = true;
            self.doc.status = format!("Jumped to {}: line {}", diag.level, diag.line + 1);
        }
    }

    /// S2 (Rust IDE): Solicita saltar a la definición del símbolo bajo el cursor (F12)
    
    /// S2 (Rust IDE): Solicita información de hover / documentación para el símbolo bajo el cursor
    
    /// S2 (Rust IDE): Chequea y procesa respuestas entrantes de Go to Definition, Hover, Code Actions, Inlay Hints y Rename
    pub fn update_lsp(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.update_lsp(&mut self.editor_state, &mut self.doc, is_rust);
    }

    pub fn goto_definition(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.goto_definition(&self.editor_state, &mut self.doc, is_rust);
    }

    pub fn request_hover(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.request_hover(&self.editor_state, &self.doc, is_rust);
    }

    pub fn request_code_actions(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.request_code_actions(&self.editor_state, &mut self.doc, is_rust);
    }

    /// Requests inlay hints for the current buffer.
    #[allow(dead_code)]
    pub fn request_inlay_hints(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.request_inlay_hints(&self.doc, is_rust);
    }

    pub fn request_rename(&mut self, new_name: &str) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.request_rename(&self.editor_state, &mut self.doc, is_rust, new_name);
    }

    /// S2 (Rust IDE): Chequea y procesa respuestas entrantes de Go to Definition, Hover, Code Actions, Inlay Hints y Rename
    pub fn poll_lsp_results(&mut self) {
        let (target, hover, actions, hints, rename) = if let Some(ref client) = self.lsp.client {
            (
                client.take_latest_definition(),
                client.take_latest_hover(),
                client.take_latest_code_actions(),
                client.take_latest_inlay_hints(),
                client.take_latest_rename(),
            )
        } else {
            (None, None, Vec::new(), Vec::new(), None)
        };

        if let Some(target) = target {
            let file_path = karkinos_core::uri_to_path(&target.uri);
            self.open_path(file_path);
            self.editor_state.cursor_line = target.range.start.line;
            self.editor_state.cursor_col = target.range.start.character;
            self.editor_state.needs_autoscroll = true;
            self.doc.status = format!("Jumped to definition at line {}", target.range.start.line + 1);
        }

        if let Some(hover) = hover {
            let first_line = hover.contents.lines().find(|l| !l.trim().is_empty()).unwrap_or(&hover.contents);
            self.doc.status = format!("Docs: {}", first_line.trim());
            self.editor_state.hover_text = Some(hover.contents);
        }

        if !actions.is_empty() {
            self.lsp.code_actions = actions;
            self.lsp.code_actions_menu_open = true;
            self.doc.status = format!("Found {} code actions (Ctrl+.)", self.lsp.code_actions.len());
        }

        if !hints.is_empty() {
            self.editor_state.inlay_hints = hints;
        }

        if let Some(rename_edit) = rename {
            self.apply_workspace_edit(&rename_edit);
        }
    }

    /// Requests code actions (quick-fixes, auto-imports) for current cursor position and diagnostics.
    
    /// Applies the code action at the specified index.
    pub fn apply_code_action(&mut self, action_idx: usize) {
        if action_idx >= self.lsp.code_actions.len() {
            return;
        }
        let action = self.lsp.code_actions[action_idx].clone();
        if let Some(ref edit) = action.edit {
            self.apply_workspace_edit(edit);
            self.doc.status = format!("Applied: {}", action.title);
        }
        self.lsp.code_actions_menu_open = false;
        self.lsp.code_actions.clear();
    }

    /// Requests inlay hints for the current buffer.
    #[allow(dead_code)]
    
    /// Requests workspace rename of symbol at cursor.
    
    /// Applies LSP text edits to the active editor buffer and updates lines and AST.
    pub fn apply_text_edits_to_active(&mut self, edits: &[karkinos_core::lsp::LspTextEdit]) {
        if edits.is_empty() {
            return;
        }
        self.doc.snapshot_if_changed();
        let mut sorted = edits.to_vec();
        sorted.sort_by(|a, b| {
            (b.range.start.line, b.range.start.character)
                .cmp(&(a.range.start.line, a.range.start.character))
        });

        for edit in sorted {
            let start_char = self
                .doc
                .buffer
                .line_col_to_char(edit.range.start.line, edit.range.start.character);
            let end_char = self
                .doc
                .buffer
                .line_col_to_char(edit.range.end.line, edit.range.end.character);

            if start_char < end_char {
                self.doc.buffer.remove(start_char..end_char);
            }
            if !edit.new_text.is_empty() {
                self.doc.buffer.insert(start_char, &edit.new_text);
            }
        }

        let full_text = self.doc.buffer.to_string();
        self.doc.lines = full_text.split('\n').map(|s| s.to_string()).collect();
        if self.doc.lines.is_empty() {
            self.doc.lines.push(String::new());
        }
        self.doc.recalc_view_bytes();
        self.doc.dirty = true;
        self.doc.snapshot_if_changed();
        self.update_ast();
        self.update_lsp();
    }

    /// Applies LSP text edits to a specific open tab.
    pub fn apply_text_edits_to_tab(&mut self, tab_idx: usize, edits: &[karkinos_core::lsp::LspTextEdit]) {
        if let Some(tab) = self.doc.tabs.get_mut(tab_idx) {
            let mut sorted = edits.to_vec();
            sorted.sort_by(|a, b| {
                (b.range.start.line, b.range.start.character)
                    .cmp(&(a.range.start.line, a.range.start.character))
            });

            for edit in sorted {
                let start_char = tab
                    .buffer
                    .line_col_to_char(edit.range.start.line, edit.range.start.character);
                let end_char = tab
                    .buffer
                    .line_col_to_char(edit.range.end.line, edit.range.end.character);

                if start_char < end_char {
                    tab.buffer.remove(start_char..end_char);
                }
                if !edit.new_text.is_empty() {
                    tab.buffer.insert(start_char, &edit.new_text);
                }
            }
            let full_text = tab.buffer.to_string();
            tab.lines = full_text.split('\n').map(|s| s.to_string()).collect();
            if tab.lines.is_empty() {
                tab.lines.push(String::new());
            }
            tab.dirty = true;
            tab.view_bytes = tab.buffer.len_bytes();
        }
    }

    /// Applies a workspace edit across open tabs and/or disk files.
    pub fn apply_workspace_edit(&mut self, edit: &karkinos_core::lsp::LspWorkspaceEdit) {
        let changes = edit.all_changes();
        let mut affected_files = 0;
        for (uri, edits) in changes {
            let file_path = karkinos_core::uri_to_path(&uri);

            // Check if it's the current file
            let is_current = self.doc.current_file.as_ref().map(|p| p == &file_path).unwrap_or(false);
            if is_current {
                self.apply_text_edits_to_active(&edits);
                affected_files += 1;
                continue;
            }

            // Check if it matches an open tab
            let mut found_tab = false;
            for (idx, tab) in self.doc.tabs.iter().enumerate() {
                if tab.current_file.as_ref().map(|p| p == &file_path).unwrap_or(false) {
                    self.apply_text_edits_to_tab(idx, &edits);
                    found_tab = true;
                    affected_files += 1;
                    break;
                }
            }

            // If not open, apply to file on disk
            if !found_tab && file_path.exists() {
                if let Ok(mut doc) = karkinos_core::Document::load_from_file(999, &file_path) {
                    doc.apply_text_edits(&edits);
                    let _ = std::fs::write(&file_path, doc.text_with_line_ending());
                    affected_files += 1;
                }
            }
        }
        self.doc.status = format!("Applied edits across {} file(s)", affected_files);
    }

    /// S4 (Rust IDE): Toggles a breakpoint on the current file and specified line.
    pub fn toggle_breakpoint(&mut self, line: usize) {
        let Some(path) = self.doc.current_file.clone() else {
            self.doc.status = "Cannot set breakpoint on unsaved file".to_string();
            return;
        };

        let key = (path.clone(), line);
        let added = if self.dap.breakpoints.contains(&key) {
            self.dap.breakpoints.remove(&key);
            false
        } else {
            self.dap.breakpoints.insert(key);
            true
        };

        self.sync_editor_breakpoints();

        // If debugger is actively running, sync to DAP server
        if let Some(ref client) = self.dap.client {
            let active_lines: Vec<usize> = self
                .dap
                .breakpoints
                .iter()
                .filter(|(p, _)| p == &path)
                .map(|(_, l)| *l)
                .collect();
            client.set_breakpoints(&path, &active_lines);
        }

        self.doc.status = format!(
            "Breakpoint {} at line {}",
            if added { "set" } else { "removed" },
            line + 1
        );
    }

    /// S4 (Rust IDE): Synchronizes breakpoints and stopped line state with active editor state.
    pub fn sync_editor_breakpoints(&mut self) {
        if let Some(ref path) = self.doc.current_file {
            self.editor_state.breakpoints = self
                .dap
                .breakpoints
                .iter()
                .filter(|(p, _)| p == path)
                .map(|(_, l)| *l)
                .collect();

            if let Some((ref stopped_path, stopped_line)) = self.dap.stopped_line {
                if stopped_path == path {
                    self.editor_state.stopped_line = Some(stopped_line);
                } else {
                    self.editor_state.stopped_line = None;
                }
            } else {
                self.editor_state.stopped_line = None;
            }
        } else {
            self.editor_state.breakpoints.clear();
            self.editor_state.stopped_line = None;
        }
    }

    /// S4 (Rust IDE): Starts or resumes a debug session via DAP (lldb-dap).
    pub fn start_debugging(&mut self) {
        self.dap.panel_open = true;

        if let Some(ref client) = self.dap.client {
            if client.state().is_stopped {
                client.continue_execution();
                self.doc.status = "Debugger: Resumed execution".to_string();
                return;
            }
        }

        // Locate executable target: target/debug/<name> or current dir
        let target_bin = self
            .doc
            .browse_dir
            .join("target")
            .join("debug")
            .join("karkinos");
        let bin_path = if target_bin.exists() {
            target_bin
        } else {
            self.doc.browse_dir.join("target").join("debug").join("app")
        };

        let client = karkinos_core::DapClient::spawn("lldb-dap", &[], &self.doc.browse_dir)
            .unwrap_or_else(|_| karkinos_core::DapClient::mock());

        // Sync all breakpoints
        let mut grouped: std::collections::HashMap<PathBuf, Vec<usize>> =
            std::collections::HashMap::new();
        for (path, line) in &self.dap.breakpoints {
            grouped.entry(path.clone()).or_insert_with(Vec::new).push(*line);
        }
        for (path, lines) in grouped {
            client.set_breakpoints(&path, &lines);
        }

        client.launch(&bin_path, &[]);
        self.dap.client = Some(client);
        self.doc.status = format!("Debugger started: {}", bin_path.display());
    }

    /// S4 (Rust IDE): Steps over current instruction (next).
    pub fn step_over(&mut self) {
        if let Some(ref client) = self.dap.client {
            client.step_over();
            self.doc.status = "Debugger: Step over".to_string();
        }
    }

    /// S4 (Rust IDE): Steps into current instruction (stepIn).
    pub fn step_in(&mut self) {
        if let Some(ref client) = self.dap.client {
            client.step_in();
            self.doc.status = "Debugger: Step in".to_string();
        }
    }

    /// S4 (Rust IDE): Steps out of current function (stepOut).
    pub fn step_out(&mut self) {
        if let Some(ref client) = self.dap.client {
            client.step_out();
            self.doc.status = "Debugger: Step out".to_string();
        }
    }

    /// S4 (Rust IDE): Pauses running debuggee.
    pub fn pause_debugger(&mut self) {
        if let Some(ref client) = self.dap.client {
            client.pause();
            self.doc.status = "Debugger: Pausing target…".to_string();
        }
    }

    /// S4 (Rust IDE): Terminates the active debug session.
    pub fn stop_debugging(&mut self) {
        if let Some(ref client) = self.dap.client {
            client.disconnect();
            self.doc.status = "Debugger: Stopped".to_string();
        }
        self.dap.client = None;
        self.dap.stopped_line = None;
        self.editor_state.stopped_line = None;
    }

    /// S4 (Rust IDE): Polls incoming DAP events (stopped, exited, output).
    pub fn poll_dap_results(&mut self) {
        if let Some(ref client) = self.dap.client {
            let st = client.state();
            if st.is_stopped {
                if let Some(ref frame) = st.current_frame {
                    if let Some(ref path) = frame.source_path {
                        if self.doc.current_file.as_ref() != Some(path) && path.exists() {
                            self.open_path(path.clone());
                        }
                        self.dap.stopped_line = Some((path.clone(), frame.line));
                        self.editor_state.stopped_line = Some(frame.line);
                        self.editor_state.cursor_line = frame.line;
                        self.editor_state.cursor_col = frame.column;
                        self.editor_state.needs_autoscroll = true;
                        self.doc.status = format!(
                            "Stopped at line {} ({})",
                            frame.line + 1,
                            frame.name
                        );
                    }
                }
            } else if !st.is_active {
                self.dap.stopped_line = None;
                self.editor_state.stopped_line = None;
            }
        }
    }

    /// S3 (Rust IDE): Triggers an asynchronous search on Open VSX
    pub fn search_open_vsx(&mut self, query: &str) {
        let q = query.trim().to_string();
        self.open_vsx_query_input = q.clone();
        self.open_vsx_searching = true;
        self.open_vsx_error = None;
        self.open_vsx_status = Some("Searching Open VSX...".to_string());

        let (tx, rx) = std::sync::mpsc::channel();
        self.open_vsx_rx = Some(rx);

        std::thread::spawn(move || {
            let res = karkinos_core::open_vsx::OpenVsxClient::search(&q, 25);
            let _ = tx.send(res);
        });
    }

    /// Returns the directory where extensions and downloaded .vsix files are stored.
    pub fn extensions_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(".karkinos")
            .join("extensions")
    }

    /// Lists installed .vsix packages on disk. Returns list of (filename, file_size_bytes).
    pub fn list_installed_vsix() -> Vec<(String, u64)> {
        let dir = Self::extensions_dir();
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("vsix") {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    list.push((name, size));
                }
            }
        }
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }

    /// Checks if a .vsix package for an extension exists on disk.
    pub fn is_vsix_installed(&self, ext: &karkinos_core::open_vsx::OpenVsxExtension) -> bool {
        let file_name = format!("{}-{}.vsix", ext.name, ext.version);
        Self::extensions_dir().join(&file_name).exists()
    }

    /// Uninstalls / deletes a downloaded .vsix package by filename.
    pub fn uninstall_vsix(&mut self, filename: &str) -> bool {
        let path = Self::extensions_dir().join(filename);
        if path.exists() {
            if std::fs::remove_file(&path).is_ok() {
                self.doc.status = format!("Uninstalled .vsix extension: {}", filename);
                true
            } else {
                self.doc.status = format!("Failed to delete: {}", filename);
                false
            }
        } else {
            false
        }
    }

    /// Uninstalls an in-memory plugin from the plugin manager.
    pub fn uninstall_plugin(&mut self, plugin_name: &str) -> bool {
        if self.plugin_manager.uninstall(plugin_name) {
            self.doc.status = format!("Uninstalled plugin: {}", plugin_name);
            true
        } else {
            false
        }
    }

    /// S3 (Rust IDE): Downloads an extension .vsix package asynchronously
    pub fn download_open_vsx_extension(&mut self, ext: &karkinos_core::open_vsx::OpenVsxExtension) {
        let url = ext.download_url.clone();
        let file_name = format!("{}-{}.vsix", ext.name, ext.version);
        let dest = Self::extensions_dir().join(&file_name);
        let id_name = ext.id();

        self.open_vsx_status = Some(format!("Downloading {}...", file_name));

        let (tx, rx) = std::sync::mpsc::channel();
        self.open_vsx_download_rx = Some(rx);

        std::thread::spawn(move || {
            let res = karkinos_core::open_vsx::OpenVsxClient::download_vsix(&url, &dest);
            let msg = match res {
                Ok(_) => Ok(format!("Successfully downloaded {} to {}", id_name, dest.display())),
                Err(e) => Err(format!("Download failed: {}", e)),
            };
            let _ = tx.send(msg);
        });
    }

    /// S3 (Rust IDE): Polls pending Open VSX search and download background tasks
    pub fn poll_open_vsx(&mut self) {
        if let Some(ref rx) = self.open_vsx_rx {
            if let Ok(result) = rx.try_recv() {
                self.open_vsx_searching = false;
                match result {
                    Ok(data) => {
                        self.open_vsx_total = data.total;
                        self.open_vsx_results = data.extensions;
                        self.open_vsx_status = Some(format!("Found {} extensions", self.open_vsx_total));
                    }
                    Err(err) => {
                        self.open_vsx_error = Some(err);
                        self.open_vsx_status = None;
                    }
                }
                self.open_vsx_rx = None;
            }
        }

        if let Some(ref rx) = self.open_vsx_download_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(msg) => {
                        self.open_vsx_status = Some(msg.clone());
                        self.doc.status = msg;
                    }
                    Err(err) => {
                        self.open_vsx_error = Some(err.clone());
                        self.doc.status = err;
                    }
                }
                self.open_vsx_download_rx = None;
            }
        }
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
        let last_line = self.doc.lines.len().saturating_sub(1);
        if line > last_line {
            // La línea vieja ya no existe: el contenido Ty se ajusta a
            // que el cursor aterrice al FIN de la última línea nueva.
            self.editor_state.cursor_line = last_line;
            self.editor_state.cursor_col =
                self.doc.lines[last_line].chars().count();
        } else {
            self.editor_state.cursor_line = line;
            let line_len = self.doc.lines[line].chars().count();
            self.editor_state.cursor_col = col.min(line_len);
        }
    }

    pub fn undo(&mut self) {
        self.doc.snapshot_if_changed();
        self.doc.chunk_tracker.reset();
        if let Some(edit) = self.doc.undo_stack.undo() {
            let snap = edit.before.clone();
            let text = snap.to_string();
            self.restore_text_keeping_view(&text);
            self.doc.last_snapshot = snap;
            self.doc.status = "Undone".to_string();
        } else {
            self.doc.status = "Nothing to undo".to_string();
        }
    }

    pub fn redo(&mut self) {
        self.doc.chunk_tracker.reset();
        if let Some(edit) = self.doc.undo_stack.redo() {
            let snap = edit.after.clone();
            let text = snap.to_string();
            self.restore_text_keeping_view(&text);
            self.doc.last_snapshot = snap;
            self.doc.status = "Redone".to_string();
        } else {
            self.doc.status = "Nothing to redo".to_string();
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
        for (li, line) in self.doc.lines.iter().enumerate() {
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
        self.doc.snapshot_if_changed();
        if let Some(&(line, col)) = self.find_matches.get(self.find_current) {
            let qlen = self.find_query.chars().count();
            let sb = crate::editor_widget::char_to_byte_pub(&self.doc.lines[line], col);
            let eb = crate::editor_widget::char_to_byte_pub(&self.doc.lines[line], col + qlen);
            self.doc.lines[line].replace_range(sb..eb, &self.find_replace);
            self.doc.dirty = true;
            self.doc.recalc_view_bytes();
            self.doc.snapshot_if_changed();
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
        self.doc.snapshot_if_changed();
        let qlen = self.find_query.chars().count();
        let count = self.find_matches.len();
        // Reemplazar de atrás hacia adelante evita recalcular offsets.
        for &(line, col) in self.find_matches.iter().rev() {
            let sb = crate::editor_widget::char_to_byte_pub(&self.doc.lines[line], col);
            let eb = crate::editor_widget::char_to_byte_pub(&self.doc.lines[line], col + qlen);
            self.doc.lines[line].replace_range(sb..eb, &self.find_replace);
        }
        self.doc.dirty = true;
        self.doc.recalc_view_bytes();
        self.doc.snapshot_if_changed();
        self.find_update_matches();
        self.doc.status = format!("Replaced {} occurrences", count);
    }

    /// Abre `path` directamente (usado por el explorador de
    /// carpetas). Después de abrir, el campo "Archivo" solo muestra
    /// el nombre — la ruta completa se ve en la barra de abajo.
    pub fn open_path(&mut self, path: PathBuf) {
        // S8 (Etapa 2): Si ya está abierto en alguna pestaña, activarla
        self.sync_active_to_tab();
        if let Some(pos) = self.doc.tabs.iter().position(|t| t.current_file.as_ref() == Some(&path)) {
            self.switch_tab(pos);
            self.doc.status = "File already open in tab".to_string();
            return;
        }

        // Si la pestaña actual está intacta y vacía, la reutilizamos; si no, creamos nueva pestaña
        let can_reuse_current = self.doc.current_file.is_none() && !self.doc.dirty && self.doc.lines == [""];
        if !can_reuse_current {
            let new_tab = TabDocument::new(self.doc.next_tab_id);
            self.doc.next_tab_id += 1;
            self.doc.tabs.push(new_tab);
            self.doc.active_tab = self.doc.tabs.len() - 1;
        }

        match DocumentService::read_file(&path) {
            Ok((content, ending, mtime)) => {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    self.doc.file_path_input = name.to_string();
                }
                self.doc.current_file = Some(path);
                self.set_text(&content);
                self.doc.last_snapshot = Rope::from_str(&content);
                self.doc.line_ending = ending;
                self.doc.last_disk_mtime = mtime;
                self.doc.external_changed = false;
                self.doc.last_mtime_check = None;
                self.doc.dirty = false;
                self.doc.status = "File opened".to_string();
                self.sync_active_to_tab();
            }
            Err(e) => {
                self.doc.status = format!("Error opening file: {}", e);
            }
        }
    }




    // ---- S8: nuevo documento ----

    /// S8: Limpia el editor para trabajar en un nuevo documento: buffer
    /// vacío, sin archivo asociado, sin cambios sin guardar, y resetea el
    /// historial de deshacer.
    pub fn new_document(&mut self) {
        self.set_text("");
        self.doc.current_file = None;
        self.doc.file_path_input.clear();
        self.doc.dirty = false;
        self.doc.last_snapshot = Rope::from_str("");
        self.doc.last_disk_mtime = None;
        self.doc.line_ending = LineEnding::Lf;
        self.doc.undo_stack = UndoStack::new();
        self.doc.chunk_tracker.reset();
        self.find_matches.clear();
        self.doc.status = "New document".to_string();
        self.sync_active_to_tab();
    }

    /// S8: Intenta crear un nuevo documento. Si el buffer actual está sucio y con contenido
    /// o guardándose, pide confirmación previa en lugar de descartar datos.
    pub fn try_new_document(&mut self) {
        if (self.doc.dirty && !self.doc.is_buffer_empty()) || self.doc.save_progress.is_some() {
            self.doc.new_doc_pending = true;
        } else {
            self.new_document();
        }
    }



    /// Reabre el archivo del disco actual (botón "Recargar" de la
    /// barra). NO hace nada si hay cambios sin guardar: pisar el
    /// buffer perdería trabajo del usuario.
    pub fn reload(&mut self) {
        let Some(path) = self.doc.current_file.clone() else {
            return;
        };
        if self.doc.dirty {
            self.doc.status = "Unsaved changes present: please save before reloading".to_string();
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
        self.doc.status = format!("Zoom: {} px", next.round());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Nombres que la T5a movió a document_service.rs / karkinos-core:
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime};
    use crate::document_service::{mtime_is_stale, SaveProgress};
    use karkinos_core::{default_new_file_path, detect_line_ending, serialize_line_ending};

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
        assert!(app.doc.status.contains("Zoom"));

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
        assert_eq!(app.doc.lines, vec!["uno", "dos", "", "tres"]);
        // Y el serializer trae de vuelta el archivo EXACTO original.
        assert_eq!(app.doc.current_text_with_ending(), "uno\r\ndos\r\n\r\ntres");
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
        assert_eq!(app.doc.current_text_with_ending(), "a\nb");
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
        app.doc.browse_dir = dir.clone();
        app.doc.dirty = true;
        // Sin ruta y sin archivo: save() ELIGE "untitled.txt" como
        // nombre default (bugfix 2026-10-29), así que el diálogo
        // ahora sí puede prometer el cierre: el guardado arranca
        // sobre un destino concreto en vez de abortar.
        app.doc.confirm_close_save();
        assert!(!app.doc.close_pending, "el diálogo sí puede cerrar: el save arrancó");
        assert!(app.doc.close_after_save);
        // El destino queda fijado a dir/untitled.txt antes del async.
        assert_eq!(app.doc.current_file, Some(dir.join("untitled.txt")));
        if let Some(progress) = &app.doc.save_progress {
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
        app.doc.current_file = Some(path.clone());
        // Memo viejo (como si el buffer hubiera venido de un archivo
        // distinto): tras un guardado ok tiene que quedar "no stale".
        app.doc.last_disk_mtime = None;
        let progress = Arc::new(Mutex::new(SaveProgress {
            written: 9,
            total: 9,
            done: true,
            error: None,
        }));
        app.doc.save_progress = Some(Arc::clone(&progress));
        app.doc.close_after_save = true;
        app.doc.note_save_done();
        assert!(app.doc.last_disk_mtime.is_some(), "post-guardado el memo tiene mtime nuevo");
        assert!(app.doc.close_after_save, "success: sigue el plan de cerrar");

        // Con error: cancelar el cierre automático y NO refrescar memo.
        progress.lock().unwrap().error = Some("disco lleno".to_string());
        app.doc.note_save_done();
        assert!(!app.doc.close_after_save);
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
        app.doc.snapshot_if_changed();

        // Simulo que el usuario tipeó más y la vista se movió abajo.
        app.set_text("uno\ndos\ntres\nCUATRO");
        app.doc.snapshot_if_changed();
        app.editor_state.cursor_line = 1;
        app.editor_state.cursor_col = 1;
        app.editor_state.scroll = 99.0;

        app.undo();
        // El texto vuelve al paso anterior...
        assert_eq!(app.doc.current_text(), "uno\ndos\ntres");
        // ...pero la vista NO se resetea a (0,0) / scroll 0.
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 1);
        assert!((app.editor_state.scroll - 99.0).abs() < f32::EPSILON);

        app.redo();
        assert_eq!(app.doc.current_text(), "uno\ndos\ntres\nCUATRO");
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 1);
        assert!((app.editor_state.scroll - 99.0).abs() < f32::EPSILON);
    }

    #[test]
    fn undo_restaura_cursor_clamp_fuera_del_buffer_reparado() {
        let mut app = EditorApp::default();
        app.set_text("sola una linea");
        app.doc.snapshot_if_changed();
        app.set_text("uno\ndos\ntres");
        app.doc.snapshot_if_changed();
        // Captura el estado del cursor fuera de rango (para ver como
        // aterrizaría sobre el buffer deshecho de UNA sola línea).
        app.editor_state.cursor_line = 2;
        app.editor_state.scroll = 40.0;

        app.undo();
        // El buffer deshecho tiene UNA línea: la 2 no existe y el
        // cursor aterriza al FIN de esa línea, no crashea ni va a 0.
        assert_eq!(app.doc.current_text(), "sola una linea");
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
        app.doc.browse_dir = dir.clone();
        app.set_text("contenido sin nombre");
        app.doc.dirty = true;
        // current_file None y file_path_input vacío: el caso reportado.
        app.doc.file_path_input.clear();

        assert!(app.doc.save());
        // Elige dir/untitled.txt y muestra en el input el nombre elegido.
        assert_eq!(app.doc.current_file, Some(dir.join("untitled.txt")));
        assert_eq!(app.doc.file_path_input, "untitled.txt");
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
        assert_eq!(app.doc.current_text(), "lobo y perro y gato");

        // Reemplazar todo lo que queda
        app.find_replace_all();
        assert_eq!(app.doc.current_text(), "lobo y perro y lobo");

        // Undo revierte el replace all
        app.undo();
        assert_eq!(app.doc.current_text(), "lobo y perro y gato");

        // Undo revierte el replace individual
        app.undo();
        assert_eq!(app.doc.current_text(), "gato y perro y gato");
    }

    // ---- S8: Botón Nuevo documento ----

    #[test]
    fn nuevo_documento_resetea_buffer_y_metadatos() {
        let mut app = EditorApp::default();
        app.set_text("texto previo");
        app.doc.current_file = Some(PathBuf::from("dummy.txt"));
        app.doc.file_path_input = "dummy.txt".to_string();
        app.doc.dirty = true;
        app.new_document();

        assert_eq!(app.doc.current_text(), "");
        assert_eq!(app.doc.lines, vec![String::new()]);
        assert_eq!(app.doc.current_file, None);
        assert_eq!(app.doc.file_path_input, "");
        assert!(!app.doc.dirty);
        assert_eq!(app.doc.status, "New document");
    }

    #[test]
    fn try_nuevo_documento_con_dirty_pide_confirmacion() {
        let mut app = EditorApp::default();
        app.set_text("Some unsaved text");
        app.doc.dirty = true;
        app.try_new_document();
        assert!(app.doc.new_doc_pending);

        // Sin cambios: crea de una sin pedir confirmación
        let mut app_clean = EditorApp::default();
        app_clean.try_new_document();
        assert!(!app_clean.doc.new_doc_pending);

        // Buffer vacío aunque esté dirty: no pide confirmación
        let mut app_empty_dirty = EditorApp::default();
        app_empty_dirty.doc.dirty = true;
        app_empty_dirty.try_new_document();
        assert!(!app_empty_dirty.doc.new_doc_pending);
    }

    #[test]
    fn test_empty_file_exit_does_not_prompt_close() {
        let mut app = EditorApp::default();
        // Even if dirty is marked true, empty buffer means all dirty tabs are empty
        app.doc.dirty = true;
        assert!(app.doc.all_dirty_tabs_are_empty());

        // Closing an empty dirty tab closes without pending confirmation
        app.try_close_tab(0);
        assert!(app.doc.tab_close_pending.is_none());

        // With text, all_dirty_tabs_are_empty is false
        app.set_text("Actual content");
        app.doc.dirty = true;
        assert!(!app.doc.all_dirty_tabs_are_empty());
        assert_eq!(app.doc.first_dirty_tab_index(), Some(0));
    }

    // ---- S8 (Etapa 2): Pestañas / Múltiples documentos ----

    #[test]
    fn tabs_creacion_cambio_y_cierre() {
        let mut app = EditorApp::default();
        assert_eq!(app.doc.tabs.len(), 1);
        assert_eq!(app.doc.active_tab, 0);

        app.set_text("Texto en pestaña 1");
        app.doc.dirty = true;

        app.new_tab();
        assert_eq!(app.doc.tabs.len(), 2);
        assert_eq!(app.doc.active_tab, 1);
        assert_eq!(app.doc.current_text(), "");

        app.set_text("Texto en pestaña 2");
        app.switch_tab(0);
        assert_eq!(app.doc.active_tab, 0);
        assert_eq!(app.doc.current_text(), "Texto en pestaña 1");

        app.switch_tab(1);
        assert_eq!(app.doc.active_tab, 1);
        assert_eq!(app.doc.current_text(), "Texto en pestaña 2");

        app.close_tab(0);
        assert_eq!(app.doc.tabs.len(), 1);
        assert_eq!(app.doc.active_tab, 0);
        assert_eq!(app.doc.current_text(), "Texto en pestaña 2");
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
        assert_eq!(app.doc.tabs.len(), 1);
        assert_eq!(app.doc.current_file, Some(path_a.clone()));

        // Abrir segundo archivo debe crear pestaña nueva
        app.open_path(path_b.clone());
        assert_eq!(app.doc.tabs.len(), 2);
        assert_eq!(app.doc.active_tab, 1);
        assert_eq!(app.doc.current_file, Some(path_b.clone()));

        // Abrir el archivo A nuevamente debe cambiar a la pestaña 0 sin duplicar
        app.open_path(path_a.clone());
        assert_eq!(app.doc.tabs.len(), 2);
        assert_eq!(app.doc.active_tab, 0);
        assert_eq!(app.doc.current_file, Some(path_a.clone()));

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
        app.doc.browse_dir = dir.clone();
        app.set_text("codigo de prueba");
        app.doc.dirty = true;

        // 1. try_save() en archivo sin nombre debe abrir el modal y no guardar aún
        assert!(!app.doc.try_save());
        assert!(app.doc.save_name_pending);

        // 2. Confirmar con nombre "mi_programa.rs"
        app.doc.save_name_input = "mi_programa.rs".to_string();
        assert!(app.doc.confirm_save_name());
        assert!(!app.doc.save_name_pending);
        assert_eq!(app.doc.current_file, Some(dir.join("mi_programa.rs")));
        assert_eq!(app.doc.file_path_input, "mi_programa.rs");

        // Esperar a que termine el guardado asíncrono
        if let Some(progress) = &app.doc.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        // 3. Otro documento sin nombre: dejar en blanco debe asignar "untitled.txt"
        let mut app2 = EditorApp::default();
        app2.doc.browse_dir = dir.clone();
        app2.set_text("otro texto");
        app2.doc.dirty = true;
        assert!(!app2.doc.try_save());
        assert!(app2.doc.save_name_pending);

        // Dejar vacío
        app2.doc.save_name_input = "   ".to_string();
        assert!(app2.doc.confirm_save_name());
        assert!(!app2.doc.save_name_pending);
        assert_eq!(app2.doc.current_file, Some(dir.join("untitled.txt")));
        assert_eq!(app2.doc.file_path_input, "untitled.txt");

        if let Some(progress) = &app2.doc.save_progress {
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
        assert_eq!(app.doc.tabs.len(), 3);
        assert_eq!(app.doc.active_tab, 2);

        // Ciclar adelante
        app.next_tab();
        assert_eq!(app.doc.active_tab, 0);
        app.next_tab();
        assert_eq!(app.doc.active_tab, 1);

        // Ciclar atrás
        app.prev_tab();
        assert_eq!(app.doc.active_tab, 0);
        app.prev_tab();
        assert_eq!(app.doc.active_tab, 2);

        // Detección de pestañas sucias
        assert_eq!(app.doc.dirty_tabs_count(), 0);
        assert_eq!(app.doc.first_dirty_tab_index(), None);

        // Marcar pestaña 1 como sucia
        app.doc.tabs[1].dirty = true;
        assert_eq!(app.doc.dirty_tabs_count(), 1);
        assert_eq!(app.doc.first_dirty_tab_index(), Some(1));

        // Si la activa (2) también está sucia, first_dirty_tab_index debe priorizar la activa
        app.doc.dirty = true;
        assert_eq!(app.doc.dirty_tabs_count(), 2);
        assert_eq!(app.doc.first_dirty_tab_index(), Some(2));
    }

    #[test]
    fn test_save_as_changes_target_file() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_save_as_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.set_text("codigo inicial");
        app.doc.current_file = Some(dir.join("archivo_original.txt"));
        app.doc.file_path_input = "archivo_original.txt".to_string();

        app.doc.save_as();
        assert!(app.doc.save_name_pending);
        assert_eq!(app.doc.save_name_input, "archivo_original.txt");

        app.doc.save_name_input = "archivo_copia.txt".to_string();
        assert!(app.doc.confirm_save_name());
        assert_eq!(app.doc.current_file, Some(dir.join("archivo_copia.txt")));

        if let Some(progress) = &app.doc.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let _ = std::fs::remove_file(dir.join("archivo_copia.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn test_editor_app_ast_worker_spans_and_folding_ranges() {
        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("main.rs"));
        app.set_text("fn calculate() -> i32 {\n    let a = 10;\n    a * 2\n}");

        // S3 (Roadmap v0.2.9): el parseo corre en FONDO: se espera la
        // respuesta del worker con presupuesto acotado (determinístico:
        // tree-sitter con 3 líneas parsea en <10ms).
        assert!(wait_for_ast(&mut app), "el worker debe terminar y la UI aplicar el resultado");
        assert!(
            app.editor_state.ast_spans.iter().any(|s| s.start_line == 0),
            "deben existir spans semánticos para la línea 0"
        );
        assert!(!app.editor_state.folding_ranges.is_empty());
        let has_func = app.editor_state.folding_ranges.iter().any(|r| r.start_line == 0 && r.end_line == 3);
        assert!(has_func, "should detect function folding range");
    }

    /// S3 (Roadmap v0.2.9): ayuda determinística de test — espera a que
    /// el worker de fondo responda (presupuesto ≤500ms).
    fn wait_for_ast(app: &mut EditorApp) -> bool {
        for _ in 0..100 {
            if app.poll_ast_results() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.poll_ast_results()
    }

    #[test]
    fn test_editor_app_lsp_diagnostics() {
        use karkinos_core::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut app = EditorApp::default();
        let path = PathBuf::from("main.rs");
        app.doc.current_file = Some(path);

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

        app.lsp.client = Some(client);
        app.update_lsp();

        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].severity, Some(DiagnosticSeverity::Error));
        assert!(app.doc.status.contains("failed to resolve"));
    }

    #[test]
    fn test_editor_app_plugin_execution() {
        let mut app = EditorApp::default();
        app.set_text("rust programming language");

        let handled = app.execute_plugin_command("uppercase_document");
        assert!(handled);
        assert_eq!(app.doc.current_text(), "RUST PROGRAMMING LANGUAGE");
        assert!(app.doc.status.contains("UPPERCASE"));

        let word_count_handled = app.execute_plugin_command("word_count");
        assert!(word_count_handled);
        assert!(app.doc.status.contains("3 words"));
    }

    #[test]
    fn test_tab_switching_preserves_lsp_singleton_and_ast() {
        use karkinos_core::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("tab1.rs"));
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
        app.lsp.client = Some(client);
        app.update_lsp();
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab1 warning");

        // Create tab 2 with Rust code
        app.new_tab();
        app.doc.current_file = Some(PathBuf::from("tab2.rs"));
        app.set_text("fn tab_two() {}");
        app.update_lsp();
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab2 error");

        // Switching back to tab 1 must NOT drop LSP client and must load tab1 diagnostics
        app.switch_tab(0);
        assert!(app.lsp.client.is_some(), "LSP client singleton must remain alive");
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab1 warning");
        // S3 (Roadmap v0.2.9): al cambiar de pestaña se re-pide el AST
        // del contenido del tab activo (worker de fondo).
        assert!(wait_for_ast(&mut app), "AST del tab activo debe aplicarse tras el switch");

        // Switching to tab 2 must also preserve LSP client and load tab2 diagnostics
        app.switch_tab(1);
        assert!(app.lsp.client.is_some(), "LSP client singleton must remain alive");
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab2 error");
        assert!(wait_for_ast(&mut app), "AST del tab activo debe aplicarse tras el switch");
    }

    #[test]
    fn test_editor_app_cargo_runner_and_navigation() {
        use karkinos_core::cargo::CargoDiagnostic;

        let mut app = EditorApp::default();
        assert!(!app.cargo.panel_open);

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
        assert!(app.doc.status.contains("Jumped to error: line 8"));
    }

    #[test]
    fn test_editor_app_goto_definition_and_hover() {
        use karkinos_core::lsp::{LspClient, LspHover, LspLocation, LspPosition, LspRange};

        let mut app = EditorApp::default();
        let client = LspClient::mock();
        let target_file = std::env::temp_dir().join("karkinos_def_test.rs");
        std::fs::write(&target_file, "fn target() {}\n").unwrap();

        // Simulate incoming definition result
        client.set_mock_definition(LspLocation {
            uri: format!("file://{}", target_file.display()),
            range: LspRange {
                start: LspPosition { line: 0, character: 3 },
                end: LspPosition { line: 0, character: 9 },
            },
        });
        client.set_mock_hover(LspHover {
            contents: "fn target() -> ()".to_string(),
            range: None,
        });
        app.lsp.client = Some(client);

        // Poll LSP results
        app.poll_lsp_results();

        assert_eq!(app.editor_state.cursor_line, 0);
        assert_eq!(app.editor_state.cursor_col, 3);
        assert!(app.editor_state.needs_autoscroll);
        assert!(app.doc.status.contains("Docs: fn target() -> ()"));
        assert_eq!(app.editor_state.hover_text.as_deref(), Some("fn target() -> ()"));

        let _ = std::fs::remove_file(target_file);
    }

    #[test]
    fn test_editor_app_open_vsx_polling() {
        use karkinos_core::open_vsx::{OpenVsxExtension, OpenVsxSearchResult};

        let mut app = EditorApp::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.open_vsx_rx = Some(rx);
        app.open_vsx_searching = true;

        tx.send(Ok(OpenVsxSearchResult {
            total: 1,
            extensions: vec![OpenVsxExtension {
                namespace: "rust-lang".to_string(),
                name: "rust-analyzer".to_string(),
                version: "0.4.3101".to_string(),
                display_name: "rust-analyzer".to_string(),
                description: "Rust language support".to_string(),
                download_url: "https://example.com/rust-analyzer.vsix".to_string(),
                download_count: 5000,
            }],
        })).unwrap();

        app.poll_open_vsx();
        assert!(!app.open_vsx_searching);
        assert_eq!(app.open_vsx_total, 1);
        assert_eq!(app.open_vsx_results.len(), 1);
        assert_eq!(app.open_vsx_results[0].id(), "rust-lang.rust-analyzer");
        assert!(app.open_vsx_status.as_ref().unwrap().contains("Found 1 extensions"));
    }

    #[test]
    fn test_editor_app_uninstall_extensions_and_vsix() {
        let mut app = EditorApp::default();
        assert_eq!(app.plugin_manager.len(), 3);

        // Test uninstall plugin
        let ok = app.uninstall_plugin("Case Transformer");
        assert!(ok);
        assert_eq!(app.plugin_manager.len(), 2);
        assert!(app.doc.status.contains("Uninstalled plugin: Case Transformer"));

        // Test uninstall vsix file
        let ext_dir = EditorApp::extensions_dir();
        let _ = std::fs::create_dir_all(&ext_dir);
        let test_vsix = ext_dir.join("test-ext-1.0.0.vsix");
        std::fs::write(&test_vsix, "fake vsix content").unwrap();

        let list = EditorApp::list_installed_vsix();
        assert!(list.iter().any(|(name, _)| name == "test-ext-1.0.0.vsix"));

        let uninstalled = app.uninstall_vsix("test-ext-1.0.0.vsix");
        assert!(uninstalled);
        assert!(!test_vsix.exists());
        assert!(app.doc.status.contains("Uninstalled .vsix extension: test-ext-1.0.0.vsix"));
    }

    #[test]
    fn test_current_language_autodetection_without_extension() {
        let mut app = EditorApp::default();
        app.doc.current_file = None;
        app.doc.file_path_input = String::new();

        // Initially empty buffer -> PlainText
        assert_eq!(app.current_language(), crate::syntax::Language::PlainText);

        // Buffer with Rust function -> Autodetects Rust
        app.set_text("pub fn main() {\n    println!(\"hello\");\n}");
        assert_eq!(app.current_language(), crate::syntax::Language::Rust);

        // Buffer with JSON -> Autodetects Json
        app.set_text("{\n  \"editor\": \"karkinos\"\n}");
        assert_eq!(app.current_language(), crate::syntax::Language::Json);

        // Buffer with HTML -> Autodetects Html
        app.set_text("<!DOCTYPE html>\n<html><body></body></html>");
        assert_eq!(app.current_language(), crate::syntax::Language::Html);
    }

    #[test]
    fn test_editor_app_code_actions_and_inlay_hints() {
        use karkinos_core::lsp::{
            InlayHintKind, LspClient, LspCodeAction, LspInlayHint, LspPosition, LspRange,
            LspTextEdit, LspWorkspaceEdit,
        };
        use std::collections::HashMap;

        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("main.rs"));
        app.set_text("fn main() {\n    let m = HashMap::new();\n}\n");

        let client = LspClient::mock();
        let mut changes = HashMap::new();
        changes.insert(
            "file://main.rs".to_string(),
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 0 },
                },
                new_text: "use std::collections::HashMap;\n".to_string(),
            }],
        );

        client.set_mock_code_actions(vec![LspCodeAction {
            title: "Import `HashMap`".to_string(),
            kind: Some("quickfix".to_string()),
            edit: Some(LspWorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
            }),
            is_preferred: Some(true),
        }]);

        client.set_mock_inlay_hints(vec![LspInlayHint {
            position: LspPosition { line: 1, character: 9 },
            label: ": HashMap<_, _>".to_string(),
            kind: Some(InlayHintKind::Type),
            padding_left: Some(true),
            padding_right: None,
        }]);

        app.lsp.client = Some(client);
        app.poll_lsp_results();

        assert!(app.lsp.code_actions_menu_open);
        assert_eq!(app.lsp.code_actions.len(), 1);
        assert_eq!(app.lsp.code_actions[0].title, "Import `HashMap`");
        assert_eq!(app.editor_state.inlay_hints.len(), 1);
        assert_eq!(app.editor_state.inlay_hints[0].label, ": HashMap<_, _>");

        // Apply code action
        app.apply_code_action(0);
        assert!(!app.lsp.code_actions_menu_open);
        let text = app.doc.current_text();
        assert!(text.starts_with("use std::collections::HashMap;\nfn main()"));
        assert!(app.doc.dirty);
        assert!(app.doc.status.contains("Applied: Import `HashMap`"));
    }

    #[test]
    fn test_editor_app_workspace_rename() {
        use karkinos_core::lsp::{
            LspClient, LspPosition, LspRange, LspTextEdit, LspWorkspaceEdit,
        };
        use std::collections::HashMap;

        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("lib.rs"));
        app.set_text("pub fn calculate() -> i32 { 42 }\n");

        let client = LspClient::mock();
        let mut changes = HashMap::new();
        changes.insert(
            "file://lib.rs".to_string(),
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: 0, character: 7 },
                    end: LspPosition { line: 0, character: 16 },
                },
                new_text: "compute_result".to_string(),
            }],
        );

        client.set_mock_rename(LspWorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
        });

        app.lsp.client = Some(client);
        app.poll_lsp_results();

        let text = app.doc.current_text();
        assert_eq!(text, "pub fn compute_result() -> i32 { 42 }\n");
        assert!(app.doc.dirty);
        assert!(app.doc.status.contains("Applied edits across 1 file(s)"));
    }

    #[test]
    fn test_editor_app_breakpoints_and_dap_debugging() {
        use karkinos_core::dap::{DapClient, DapStackFrame, DapStopReason, DapVariable};

        let mut app = EditorApp::default();
        let file_path = PathBuf::from("src/main.rs");
        app.doc.current_file = Some(file_path.clone());
        app.set_text("fn main() {\n    let val = 10;\n    println!(\"{}\", val);\n}\n");

        // Toggle breakpoint at line 1
        app.toggle_breakpoint(1);
        assert!(app.dap.breakpoints.contains(&(file_path.clone(), 1)));
        assert!(app.editor_state.breakpoints.contains(&1));
        assert!(app.doc.status.contains("Breakpoint set at line 2"));

        // Toggle breakpoint again -> removes it
        app.toggle_breakpoint(1);
        assert!(!app.dap.breakpoints.contains(&(file_path.clone(), 1)));
        assert!(!app.editor_state.breakpoints.contains(&1));
        assert!(app.doc.status.contains("Breakpoint removed at line 2"));

        // Set breakpoint again
        app.toggle_breakpoint(1);

        // Start debugging with mock DAP client
        let dap = DapClient::mock();
        dap.set_mock_stopped(
            DapStopReason::Breakpoint,
            DapStackFrame {
                id: 1,
                name: "main()".to_string(),
                source_path: Some(file_path.clone()),
                line: 1,
                column: 4,
            },
            vec![DapVariable {
                name: "val".to_string(),
                value: "10".to_string(),
                type_name: Some("i32".to_string()),
            }],
        );

        app.dap.client = Some(dap);
        app.poll_dap_results();

        assert_eq!(app.dap.stopped_line, Some((file_path.clone(), 1)));
        assert_eq!(app.editor_state.stopped_line, Some(1));
        assert_eq!(app.editor_state.cursor_line, 1);
        assert!(app.doc.status.contains("Stopped at line 2"));

        // Test step over
        app.step_over();
        assert!(app.doc.status.contains("Debugger: Step over"));

        // Test stop debugging
        app.stop_debugging();
        assert!(app.dap.client.is_none());
        assert!(app.dap.stopped_line.is_none());
        assert!(app.editor_state.stopped_line.is_none());
        assert!(app.doc.status.contains("Debugger: Stopped"));
    }

    // F5 (Roadmap v0.2.31): split panes.

    #[test]
    fn toggle_split_abre_cambia_eje_y_cierra() {
        let mut app = EditorApp::default();
        assert!(app.split.is_none(), "por defecto hay un solo panel");

        app.toggle_split(SplitDirection::Vertical);
        let split = app.split.as_ref().expect("split vertical");
        assert_eq!(split.direction, SplitDirection::Vertical);
        assert_eq!(split.ratio, 0.5);

        // Mismo eje: cierra.
        app.toggle_split(SplitDirection::Vertical);
        assert!(app.split.is_none());

        // Otro eje: cambia la dirección sin salir del split.
        app.toggle_split(SplitDirection::Vertical);
        app.toggle_split(SplitDirection::Horizontal);
        assert_eq!(
            app.split.as_ref().expect("split horizontal").direction,
            SplitDirection::Horizontal
        );
    }

    #[test]
    fn set_split_ratio_clampa_en_rango_util() {
        let mut app = EditorApp::default();
        app.toggle_split(SplitDirection::Vertical);
        app.set_split_ratio(0.01);
        assert_eq!(app.split.as_ref().unwrap().ratio, 0.15);
        app.set_split_ratio(0.99);
        assert_eq!(app.split.as_ref().unwrap().ratio, 0.85);
        app.set_split_ratio(0.4);
        assert_eq!(app.split.as_ref().unwrap().ratio, 0.4);
    }
}
