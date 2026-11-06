use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::editor_widget::EditorState;
use crate::document_service::{DocumentService, TabDocument};
use karkinos_core::recovery::{
    clear_recovery_dir, journal_filename, remove_recovery_file, save_recovery_file,
    scan_recovery_dir, RecoveryEntry,
};
use karkinos_core::LineEnding;
use ropey::Rope;
use crate::undo::UndoStack;



// S10: rango y paso del zoom de la fuente del editor (clamp 8..32 px).
pub const ZOOM_MIN: f32 = 8.0;
pub const ZOOM_MAX: f32 = 32.0;
pub const ZOOM_STEP: f32 = 1.0;

/// T2 (S1): ventana de calma tras la última tecla antes de recalcular el
/// texto completo para el AST y el LSP. Antes se recalculaban en cada
/// tecla y eso era lo que trababa el editor en documentos grandes.
pub const FULL_TEXT_SYNC_DEBOUNCE: Duration = Duration::from_millis(120);

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

/// v0.2.51: segundos UNIX actuales (0 si el reloj está antes de 1970).
pub fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// v0.2.51: edad legible de una entrada de recovery, con reloj inyectable
/// para tests. La UI del modal la usa con el reloj real.
pub fn format_recovery_age_at(timestamp_secs: u64, now_secs: u64) -> String {
    let age = now_secs.saturating_sub(timestamp_secs);
    match age {
        0..=59 => "hace unos segundos".to_string(),
        60..=3599 => format!("hace {} min", age / 60),
        3600..=86_399 => format!("hace {} h", age / 3600),
        _ => format!("hace {} días", age / 86_400),
    }
}

/// v0.2.51: edad legible usando el reloj del sistema.
pub fn format_recovery_age(timestamp_secs: u64) -> String {
    format_recovery_age_at(timestamp_secs, unix_now_secs())
}

/// v0.2.41: un nombre válido para el diálogo del explorador es un único
/// componente normal de ruta (sin separadores, `.`, `..` ni rutas absolutas).
pub(crate) fn is_valid_explorer_item_name(name: &str) -> bool {
    let mut components = std::path::Path::new(name).components();
    matches!(
        components.next(),
        Some(std::path::Component::Normal(_))
    ) && components.next().is_none()
}

/// T2 (S4): acciones del menú File de la barra de pestañas. Se extraen a
/// un enum para ejecutarlas después del closure de egui (que ya presta
/// `self`) y para poder testearlas sin un contexto gráfico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMenuAction {
    NewTab,
    OpenFile,
    OpenFolder,
    Save,
    SaveAs,
    SearchFiles,
    SearchInWorkspace,
    CloseTab,
    Exit,
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
    // v0.2.52: creación de archivos/carpetas desde el popup "Open…".
    // `picker_new_kind` distinto de `None` muestra el input inline.
    pub picker_new_kind: Option<PickerNewItemKind>,
    pub picker_new_name: String,
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
    pub extensions_tab: usize,
    // S9 (Roadmap v0.2.72): estado y operaciones de Open VSX / `.vsix`.
    pub extensions: crate::extensions_service::ExtensionsService,
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
    // T2 (S1): el AST y el LSP necesitan el texto completo; recalcularlos
    // en cada tecla era la causa del lag. Acá se guarda el instante del
    // último cambio y el bucle por frame los aplica tras 120 ms de calma.
    pub pending_full_text_sync: Option<Instant>,
    // T2 (S1): lenguaje detectado de la ruta activa. Solo se recalcula
    // cuando cambia la ruta (extensión/filename); sin ruta no se cachea
    // porque ahí la detección depende del contenido y cambia al escribir.
    current_language_cache: std::cell::RefCell<Option<(PathBuf, crate::syntax::Language)>>,
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
    // F6 (Roadmap v0.2.32): búsqueda global en el workspace.
    pub workspace_search: WorkspaceSearchState,
    pub workspace_search_engine: karkinos_core::WorkspaceSearch,
    // Barra lateral unificada (v0.2.39): explorador, búsqueda y símbolos en el mismo panel.
    pub sidebar_tab: SidebarTab,
    pub explorer: ExplorerState,
    // F7 (Roadmap v0.2.34): selector de símbolos (Ctrl+Shift+O).
    pub symbol_picker: SymbolPickerState,
    // v0.2.40: Command Palette (Ctrl+Shift+P / F1).
    pub command_palette: CommandPaletteState,
    // F8 (Roadmap v0.2.35): rama actual y blame inline.
    pub git_status: GitStatusState,
    // v0.2.42: pestaña Source Control (cambios, mensaje y feedback).
    pub git_ui: GitUiState,
    // v0.2.44: visor de diff abierto sobre un archivo del Source Control.
    pub git_diff_modal: Option<GitDiffViewState>,
    // v0.2.45 (Tarea C): input de ruta del modal "Open Folder" (None = cerrado).
    pub open_folder_modal: Option<String>,
    // v0.2.44: watcher periódico de disco (pestañas + metadatos de Git).
    pub last_disk_poll: Option<std::time::Instant>,
    pub last_git_mtime: Option<std::time::SystemTime>,
    pub blame_worker: crate::git_status_worker::BlameWorker,
    // v0.2.48 (Tarea B): servidor IPC de instancia única. `None` cuando el
    // proceso arrancó como secundario (no aplica) o el bind falló; en la
    // instancia primaria canaliza los `karkinos <path>` entrantes.
    pub ipc_server: Option<karkinos_core::ipc::IpcServer>,
    pub ipc_waker_installed: bool,
    // v0.2.51: journaling & crash recovery. `recovery_pending` holds the
    // journals found at startup until the user restores or discards them.
    pub recovery_pending: Vec<(PathBuf, RecoveryEntry)>,
    pub show_recovery_modal: bool,
    pub recovery_dir: PathBuf,
    // Contenido observado en el último tick por pestaña (para detectar
    // ediciones sin comparar contra el último journal emitido).
    recovery_seen: std::collections::HashMap<usize, String>,
    // Último contenido efectivamente escrito al journal.
    pub(crate) recovery_journaled: std::collections::HashMap<usize, String>,
    // Momento del último cambio de contenido observado por pestaña.
    pub(crate) recovery_changed_at: std::collections::HashMap<usize, Instant>,
    recovery_last_scan: Option<Instant>,
}

/// v0.2.44: contenido del visor de diff de un archivo del Source Control.
#[derive(Debug, Clone, Default)]
pub struct GitDiffViewState {
    pub file_path: std::path::PathBuf,
    pub rel_name: String,
    pub diff_text: String,
    pub parsed_lines: Vec<karkinos_core::git_status::ParsedDiffLine>,
    pub is_staged: bool,
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

/// F6 (Roadmap v0.2.32): estado del panel de búsqueda global.
#[derive(Debug, Default)]
pub struct WorkspaceSearchState {
    pub open: bool,
    pub input: String,
    pub case_sensitive: bool,
    pub match_word: bool,
    pub use_regex: bool,
    pub results: Vec<karkinos_core::FileMatches>,
    pub collapsed: std::collections::HashSet<std::path::PathBuf>,
    pub searching: bool,
    pub error: Option<String>,
    pub total_matches: usize,
    pub files_scanned: usize,
    /// Generación del pedido vigente; los eventos viejos se descartan.
    pub generation: u64,
    /// Pide foco en el campo al abrir el panel (Ctrl+Shift+F).
    pub focus_requested: bool,
    /// Momento del último cambio de texto, para el debounce.
    pub last_edit_at: Option<std::time::Instant>,
    // v0.2.43: reemplazo global en el workspace.
    /// El campo de reemplazo está desplegado.
    pub show_replace: bool,
    /// Texto de reemplazo aplicado a todas las coincidencias.
    pub replace_input: String,
    /// Filtro de archivos (`*.rs, *.toml`, `src/`, …); vacío = sin filtro.
    pub file_filter: String,
    /// Feedback del último reemplazo (éxito o error).
    pub replace_summary: Option<String>,
    /// Pide foco en el campo de reemplazo (Ctrl+Shift+H).
    pub replace_focus_requested: bool,
}

/// Barra lateral unificada (v0.2.39): herramienta activa en el panel lateral compartido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarTab {
    #[default]
    None,
    Explorer,
    Search,
    SourceControl,
    Symbols,
    Extensions,
}

/// Barra lateral (v0.2.33): estado del explorador de archivos del proyecto.
#[derive(Debug, Default)]
pub struct ExplorerState {
    pub open: bool,
    pub files: Vec<std::path::PathBuf>,
    /// v0.2.42: todas las carpetas del recorrido, incluidas las vacías.
    pub directories: Vec<std::path::PathBuf>,
    /// Raíz para la que se cargó la lista; si cambia, se refresca.
    pub loaded_for: Option<std::path::PathBuf>,
    /// Carpetas plegadas en el árbol (UI-6).
    pub collapsed_dirs: std::collections::HashSet<std::path::PathBuf>,
    /// v0.2.41: diálogo inline de creación activo (`None` = cerrado).
    pub creating_item: Option<ExplorerNewItemKind>,
    pub new_item_name: String,
    /// Carpeta destino del nuevo ítem; `None` usa `doc.browse_dir`.
    pub new_item_target_dir: Option<std::path::PathBuf>,
    /// v0.2.41: archivo pendiente de confirmación de borrado.
    pub item_to_delete: Option<std::path::PathBuf>,
    /// Pide foco en el input de creación al abrir el diálogo inline.
    pub new_item_focus: bool,
    /// v0.2.49: archivo/carpeta pendiente de renombrado inline.
    /// La tupla es `(old_path, new_name)`: el `String` es el nombre
    /// editable que muestra el input y que se confirma con Enter/✓.
    pub renaming_item: Option<(std::path::PathBuf, String)>,
    /// Pide foco en el input de renombrado al abrir el diálogo inline.
    pub rename_focus: bool,
}

/// v0.2.41: tipo de ítem que crea el diálogo inline del explorador.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerNewItemKind {
    File,
    Folder,
}

/// v0.2.52: tipo de ítem que crea el popup "Open…".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerNewItemKind {
    File,
    Folder,
}

impl ExplorerState {
    /// v0.2.41: abre el diálogo de creación inline para `kind`.
    pub fn begin_new_item(
        &mut self,
        kind: ExplorerNewItemKind,
        target_dir: Option<std::path::PathBuf>,
    ) {
        self.creating_item = Some(kind);
        self.new_item_name.clear();
        self.new_item_target_dir = target_dir;
        self.item_to_delete = None;
        self.new_item_focus = true;
    }

    /// v0.2.41: cierra el diálogo de creación sin tocar el disco.
    pub fn cancel_new_item(&mut self) {
        self.creating_item = None;
        self.new_item_name.clear();
        self.new_item_target_dir = None;
        self.new_item_focus = false;
    }
}

/// UI-6 (v0.2.38): nodo del árbol de archivos del explorador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeNode {
    Directory {
        name: String,
        path: std::path::PathBuf,
        children: Vec<FileTreeNode>,
    },
    File {
        name: String,
        path: std::path::PathBuf,
        extension: String,
    },
}

impl FileTreeNode {
    /// Construye el árbol jerárquico a partir de la lista plana de archivos.
    /// Primero las carpetas (alfabéticas), después los archivos.
    ///
    /// Se conserva por retrocompatibilidad; la UI usa
    /// [`FileTreeNode::build_from_paths_and_dirs`] para incluir carpetas vacías.
    #[allow(dead_code)]
    pub fn build_from_paths(
        root: &std::path::Path,
        paths: &[std::path::PathBuf],
    ) -> Vec<FileTreeNode> {
        Self::build_from_paths_and_dirs(root, paths, &[])
    }

    /// v0.2.42: construye el árbol combinando archivos y carpetas explícitas,
    /// así las carpetas vacías también aparecen. Las rutas se interpretan
    /// relativas a `root` y se descartan las que caen fuera.
    pub fn build_from_paths_and_dirs(
        root: &std::path::Path,
        files: &[std::path::PathBuf],
        dirs: &[std::path::PathBuf],
    ) -> Vec<FileTreeNode> {
        use std::collections::{BTreeMap, BTreeSet};

        let mut dir_set: BTreeSet<std::path::PathBuf> = BTreeSet::new();
        for dir in dirs {
            if let Ok(relative) = dir.strip_prefix(root) {
                insert_with_ancestors(&mut dir_set, relative);
            }
        }

        let mut files_by_dir: BTreeMap<std::path::PathBuf, Vec<FileTreeNode>> = BTreeMap::new();
        for path in files {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Some(file_name) = relative.file_name() else {
                continue;
            };
            let parent = relative.parent().unwrap_or(std::path::Path::new(""));
            insert_with_ancestors(&mut dir_set, parent);
            files_by_dir
                .entry(parent.to_path_buf())
                .or_default()
                .push(FileTreeNode::File {
                    name: file_name.to_string_lossy().to_string(),
                    path: path.clone(),
                    extension: path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_string(),
                });
        }

        Self::build_level(root, std::path::Path::new(""), &dir_set, &mut files_by_dir)
    }

    fn build_level(
        root: &std::path::Path,
        rel_dir: &std::path::Path,
        dir_set: &std::collections::BTreeSet<std::path::PathBuf>,
        files_by_dir: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<FileTreeNode>>,
    ) -> Vec<FileTreeNode> {
        let mut out = Vec::new();
        for dir_rel in dir_set {
            if dir_rel.parent() != Some(rel_dir) {
                continue;
            }
            let name = dir_rel
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let path = root.join(dir_rel);
            let children = Self::build_level(root, dir_rel, dir_set, files_by_dir);
            out.push(FileTreeNode::Directory {
                name,
                path,
                children,
            });
        }
        if let Some(mut file_nodes) = files_by_dir.remove(rel_dir) {
            file_nodes.sort_by(|a, b| a.name().cmp(b.name()));
            out.extend(file_nodes);
        }
        out
    }

    pub fn name(&self) -> &str {
        match self {
            FileTreeNode::Directory { name, .. } => name,
            FileTreeNode::File { name, .. } => name,
        }
    }

    /// Todas las rutas de carpeta del árbol (para "Collapse All").
    pub fn collect_directories(nodes: &[FileTreeNode], out: &mut Vec<std::path::PathBuf>) {
        for node in nodes {
            if let FileTreeNode::Directory { path, children, .. } = node {
                out.push(path.clone());
                FileTreeNode::collect_directories(children, out);
            }
        }
    }
}

/// v0.2.42: registra `relative` y todos sus ancestros en el conjunto de
/// carpetas del árbol, para que una carpeta vacía no pierda su jerarquía.
fn insert_with_ancestors(
    set: &mut std::collections::BTreeSet<std::path::PathBuf>,
    relative: &std::path::Path,
) {
    let mut current = std::path::PathBuf::new();
    for component in relative.components() {
        current.push(component.as_os_str());
        set.insert(current.clone());
    }
}

/// F7 (Roadmap v0.2.34): estado del selector de símbolos del documento.
#[derive(Debug, Default)]
pub struct SymbolPickerState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

/// v0.2.40: acción registrable en el Command Palette (`Ctrl+Shift+P` / `F1`).
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str, // "Editor", "View", "Cargo", "Debug", "Layout", "Theme", "File"
    pub shortcut: Option<&'static str>,
}

/// v0.2.40: estado del modal del Command Palette.
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

/// v0.2.40: catálogo estático de comandos disponibles en el Command Palette.
/// El orden define el orden sin filtro y es estable para los tests.
pub const PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        id: "layout:split_vertical",
        title: "View: Split Editor Vertically",
        category: "Layout",
        shortcut: Some("Ctrl+\\"),
    },
    PaletteCommand {
        id: "layout:split_horizontal",
        title: "View: Split Editor Horizontally",
        category: "Layout",
        shortcut: Some("Ctrl+Shift+\\"),
    },
    PaletteCommand {
        id: "layout:close_split",
        title: "View: Close Split Editor",
        category: "Layout",
        shortcut: None,
    },
    PaletteCommand {
        id: "editor:toggle_minimap",
        title: "View: Toggle Minimap",
        category: "View",
        shortcut: Some("Ctrl+Shift+M"),
    },
    PaletteCommand {
        id: "sidebar:explorer",
        title: "View: Show Explorer",
        category: "View",
        shortcut: Some("Ctrl+Shift+E"),
    },
    PaletteCommand {
        id: "sidebar:search",
        title: "View: Search in Workspace",
        category: "View",
        shortcut: Some("Ctrl+Shift+F"),
    },
    PaletteCommand {
        id: "search:replace_in_workspace",
        title: "Search: Replace in Files (Ctrl+Shift+H)",
        category: "Search",
        shortcut: Some("Ctrl+Shift+H"),
    },
    PaletteCommand {
        id: "search:toggle_replace",
        title: "Search: Toggle Replace Field",
        category: "Search",
        shortcut: None,
    },
    PaletteCommand {
        id: "sidebar:symbols",
        title: "View: Show Document Symbols",
        category: "View",
        shortcut: Some("Ctrl+Shift+O"),
    },
    PaletteCommand {
        id: "sidebar:extensions",
        title: "View: Show Extensions",
        category: "View",
        shortcut: Some("Ctrl+Shift+X"),
    },
    PaletteCommand {
        id: "terminal:toggle",
        title: "Terminal: Toggle Terminal Dock",
        category: "View",
        shortcut: Some("Ctrl+`"),
    },
    PaletteCommand {
        id: "cargo:check",
        title: "Cargo: Run Check",
        category: "Cargo",
        shortcut: Some("F7"),
    },
    PaletteCommand {
        id: "cargo:test",
        title: "Cargo: Run Tests",
        category: "Cargo",
        shortcut: None,
    },
    PaletteCommand {
        id: "cargo:clippy",
        title: "Cargo: Run Clippy",
        category: "Cargo",
        shortcut: None,
    },
    PaletteCommand {
        id: "cargo:run",
        title: "Cargo: Run Project",
        category: "Cargo",
        shortcut: Some("F5"),
    },
    PaletteCommand {
        id: "debug:toggle_breakpoint",
        title: "Debug: Toggle Breakpoint",
        category: "Debug",
        shortcut: Some("F9"),
    },
    PaletteCommand {
        id: "debug:start",
        title: "Debug: Start Debugging",
        category: "Debug",
        shortcut: Some("F5"),
    },
    PaletteCommand {
        id: "debug:step_over",
        title: "Debug: Step Over",
        category: "Debug",
        shortcut: Some("F10"),
    },
    PaletteCommand {
        id: "debug:step_into",
        title: "Debug: Step Into",
        category: "Debug",
        shortcut: Some("F11"),
    },
    PaletteCommand {
        id: "debug:stop",
        title: "Debug: Stop Debugging",
        category: "Debug",
        shortcut: None,
    },
    PaletteCommand {
        id: "theme:dark",
        title: "Preferences: Theme: Dark (Graphite)",
        category: "Theme",
        shortcut: None,
    },
    PaletteCommand {
        id: "theme:monokai",
        title: "Preferences: Theme: Monokai",
        category: "Theme",
        shortcut: None,
    },
    PaletteCommand {
        id: "theme:dracula",
        title: "Preferences: Theme: Dracula",
        category: "Theme",
        shortcut: None,
    },
    PaletteCommand {
        id: "theme:onedark",
        title: "Preferences: Theme: One Dark",
        category: "Theme",
        shortcut: None,
    },
    PaletteCommand {
        id: "editor:format",
        title: "Editor: Format Document with rustfmt",
        category: "Editor",
        shortcut: None,
    },
    PaletteCommand {
        id: "file:save",
        title: "File: Save Current File",
        category: "File",
        shortcut: Some("Ctrl+S"),
    },
    PaletteCommand {
        id: "file:new",
        title: "File: New Untitled File",
        category: "File",
        shortcut: Some("Ctrl+N"),
    },
    PaletteCommand {
        id: "file:open_folder",
        title: "File: Open Folder (Change Workspace)",
        category: "File",
        shortcut: Some("Ctrl+K Ctrl+O"),
    },
    PaletteCommand {
        id: "git:source_control",
        title: "Git: Open Source Control",
        category: "Git",
        shortcut: Some("Ctrl+Shift+G"),
    },
    PaletteCommand {
        id: "git:refresh",
        title: "Git: Refresh Status",
        category: "Git",
        shortcut: None,
    },
    PaletteCommand {
        id: "git:stage_all",
        title: "Git: Stage All Changes",
        category: "Git",
        shortcut: None,
    },
    PaletteCommand {
        id: "git:view_diff",
        title: "Git: View Diff of Active File",
        category: "Git",
        shortcut: None,
    },
    PaletteCommand {
        id: "git:close_diff",
        title: "Git: Close Diff Viewer",
        category: "Git",
        shortcut: None,
    },
    PaletteCommand {
        id: "file:rename_active",
        title: "File: Rename Active File",
        category: "File",
        shortcut: Some("F2"),
    },
    PaletteCommand {
        id: "file:duplicate_active",
        title: "File: Duplicate Active File",
        category: "File",
        shortcut: None,
    },
    PaletteCommand {
        id: "view:zoom_in",
        title: "View: Zoom In",
        category: "View",
        shortcut: Some("Ctrl++"),
    },
    PaletteCommand {
        id: "view:zoom_out",
        title: "View: Zoom Out",
        category: "View",
        shortcut: Some("Ctrl+-"),
    },
    PaletteCommand {
        id: "view:reset_zoom",
        title: "View: Reset Zoom",
        category: "View",
        shortcut: Some("Ctrl+0"),
    },
    PaletteCommand {
        id: "editor:toggle_smear_cursor",
        title: "Editor: Toggle Smear Cursor Animation",
        category: "Editor",
        shortcut: None,
    },
];

/// v0.2.42: estado de la pestaña Source Control.
#[derive(Debug, Default)]
pub struct GitUiState {
    pub entries: Vec<karkinos_core::git_status::GitStatusEntry>,
    pub commit_message: String,
    pub is_loading: bool,
    pub last_error: Option<String>,
}

/// F8 (Roadmap v0.2.35): rama actual y cache de blame por archivo.
#[derive(Debug, Default)]
pub struct GitStatusState {
    pub branch: Option<String>,
    pub blame_cache: std::collections::HashMap<
        std::path::PathBuf,
        std::sync::Arc<std::collections::HashMap<usize, karkinos_core::git_status::BlameLine>>,
    >,
    pub blame_requested_for: Option<std::path::PathBuf>,
    pub cursor_settled_at: Option<std::time::Instant>,
    pub last_cursor: (usize, usize),
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
            picker_new_kind: None,
            picker_new_name: String::new(),
            font_size: 14.0,
            theme_mode: crate::theme::ThemeMode::Dark,
            quick_open: crate::quick_open::QuickOpenState::default(),
            quick_open_services: crate::quick_open::QuickOpenServices::default(),
            plugin_manager: karkinos_core::plugin::PluginManager::with_default_plugins(),
            extensions_tab: 0,
            extensions: crate::extensions_service::ExtensionsService::default(),
            ast_worker: crate::ast_worker::AstWorker::spawn(),
            ast_request_rev: 0,
            ast_applied_rev: 0,
            pending_full_text_sync: None,
            current_language_cache: std::cell::RefCell::new(None),
            git_diff_worker: crate::git_diff_worker::GitDiffWorker::spawn(),
            git_diff_requested_path: None,
            git_diff_was_dirty: false,
            split: None,
            workspace_search: WorkspaceSearchState::default(),
            workspace_search_engine: karkinos_core::WorkspaceSearch::spawn(),
            sidebar_tab: SidebarTab::None,
            explorer: ExplorerState::default(),
            symbol_picker: SymbolPickerState::default(),
            command_palette: CommandPaletteState::default(),
            git_status: GitStatusState::default(),
            git_ui: GitUiState::default(),
            git_diff_modal: None,
            open_folder_modal: None,
            last_disk_poll: None,
            last_git_mtime: None,
            blame_worker: crate::git_status_worker::BlameWorker::spawn(),
            ipc_server: None,
            ipc_waker_installed: false,
            recovery_pending: Vec::new(),
            show_recovery_modal: false,
            recovery_dir: karkinos_core::recovery::recovery_dir(),
            recovery_seen: std::collections::HashMap::new(),
            recovery_journaled: std::collections::HashMap::new(),
            recovery_changed_at: std::collections::HashMap::new(),
            recovery_last_scan: None,
        }
    }
}

impl EditorApp {
    /// v0.2.51: construye la app y detecta journals dejados por una
    /// sesión anterior. `Default` queda libre de I/O para tests; el
    /// arranque real pasa por acá.
    pub fn new() -> Self {
        let mut app = Self::default();
        app.init_recovery_from_dir(karkinos_core::recovery::recovery_dir());
        app
    }

    /// v0.2.51: escanea `dir` y arma el estado del modal de recuperación.
    /// Separado de `new()` para poder testear con un directorio temporal.
    pub fn init_recovery_from_dir(&mut self, dir: PathBuf) {
        self.recovery_dir = dir;
        self.recovery_pending = scan_recovery_dir(&self.recovery_dir);
        self.show_recovery_modal = !self.recovery_pending.is_empty();
    }

    /// v0.2.51: escribe journals de las pestañas dirty con debounce de
    /// 1.5 s desde la última modificación. Se llama una vez por frame;
    /// el escaneo pesado se limita a ~4 Hz.
    pub fn poll_recovery_autosave(&mut self) {
        self.poll_recovery_autosave_at(Instant::now());
    }

    /// Variante con reloj inyectable para tests deterministas.
    pub fn poll_recovery_autosave_at(&mut self, now: Instant) {
        if self.show_recovery_modal {
            return;
        }
        if let Some(last) = self.recovery_last_scan {
            if now.saturating_duration_since(last) < Duration::from_millis(250) {
                return;
            }
        }
        self.recovery_last_scan = Some(now);

        let dir = self.recovery_dir.clone();
        let now_secs = unix_now_secs();
        for idx in 0..self.doc.tabs.len() {
            let (id, dirty, path, title, content, cursor_line, cursor_col) =
                self.recovery_tab_snapshot(idx);
            if !dirty {
                continue;
            }

            let changed = self
                .recovery_seen
                .get(&id)
                .map(|previous| previous != &content)
                .unwrap_or(true);
            if changed {
                self.recovery_seen.insert(id, content.clone());
                self.recovery_changed_at.insert(id, now);
            }

            let idle_for = self
                .recovery_changed_at
                .get(&id)
                .map(|changed_at| now.saturating_duration_since(*changed_at))
                .unwrap_or_default();
            if idle_for < Duration::from_millis(1500) {
                continue;
            }
            if self.recovery_journaled.get(&id) == Some(&content) {
                continue;
            }

            let entry = RecoveryEntry {
                original_path: path.clone(),
                title,
                content: content.clone(),
                cursor_line,
                cursor_col,
                timestamp_secs: now_secs,
            };
            let filename = journal_filename(path.as_deref(), id);
            if save_recovery_file(&dir, &filename, &entry).is_ok() {
                self.recovery_journaled.insert(id, content);
            }
        }
    }

    /// v0.2.51: reabre cada entrada pendiente en una pestaña nueva, con
    /// el texto recuperado marcado como dirty y el cursor restaurado.
    pub fn restore_all_recovery(&mut self) {
        let entries: Vec<RecoveryEntry> = self
            .recovery_pending
            .drain(..)
            .map(|(_, entry)| entry)
            .collect();
        self.show_recovery_modal = false;
        let restored = entries.len();
        for entry in &entries {
            self.restore_recovery_entry(entry);
        }
        let _ = clear_recovery_dir(&self.recovery_dir);
        self.reset_recovery_tracking();
        self.doc.status = if restored == 1 {
            "Recovered 1 unsaved buffer".to_string()
        } else {
            format!("Recovered {restored} unsaved buffers")
        };
    }

    /// v0.2.51: descarta los journals sin tocar los archivos en disco.
    pub fn discard_all_recovery(&mut self) {
        let _ = clear_recovery_dir(&self.recovery_dir);
        self.recovery_pending.clear();
        self.show_recovery_modal = false;
        self.reset_recovery_tracking();
        self.doc.status = "Crash recovery discarded".to_string();
    }

    /// v0.2.51: elimina el journal de la pestaña `idx` (guardado
    /// exitoso, cierre o descarte). Borra tanto la clave por ruta como
    /// la de untitled, por si el guardado le asignó nombre a la pestaña.
    pub fn cleanup_recovery_for_tab(&mut self, idx: usize) {
        let Some(tab) = self.doc.tabs.get(idx) else {
            return;
        };
        let id = tab.id;
        let path = if idx == self.doc.active_tab {
            self.doc.current_file.clone()
        } else {
            tab.current_file.clone()
        };

        let dir = self.recovery_dir.clone();
        let _ = remove_recovery_file(&dir, &journal_filename(None, id));
        if let Some(path) = path.as_deref() {
            let _ = remove_recovery_file(&dir, &journal_filename(Some(path), id));
        }

        self.recovery_seen.remove(&id);
        self.recovery_journaled.remove(&id);
        self.recovery_changed_at.remove(&id);
    }

    /// v0.2.51: variante para el guardado de la pestaña activa.
    pub fn cleanup_recovery_for_active_tab(&mut self) {
        let idx = self.doc.active_tab;
        self.cleanup_recovery_for_tab(idx);
    }

    /// v0.2.51: limpia journals de todas las pestañas dirty (descartar
    /// el cierre de la ventana completa).
    pub fn cleanup_recovery_for_dirty_tabs(&mut self) {
        for idx in 0..self.doc.tabs.len() {
            let dirty = if idx == self.doc.active_tab {
                self.doc.dirty
            } else {
                self.doc.tabs[idx].dirty
            };
            if dirty {
                self.cleanup_recovery_for_tab(idx);
            }
        }
    }

    fn reset_recovery_tracking(&mut self) {
        self.recovery_seen.clear();
        self.recovery_journaled.clear();
        self.recovery_changed_at.clear();
        self.recovery_last_scan = None;
    }

    /// Datos de journaling de la pestaña `idx`: (id, dirty, ruta, título,
    /// contenido, línea, columna). La pestaña activa vive en `doc`.
    fn recovery_tab_snapshot(
        &self,
        idx: usize,
    ) -> (usize, bool, Option<PathBuf>, String, String, usize, usize) {
        let tab = &self.doc.tabs[idx];
        let active = idx == self.doc.active_tab;
        let dirty = if active { self.doc.dirty } else { tab.dirty };
        let path = if active {
            self.doc.current_file.clone()
        } else {
            tab.current_file.clone()
        };
        let content = if active {
            self.doc.text()
        } else {
            tab.text()
        };
        let title = if let Some(name) = path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
        {
            name.to_string()
        } else if !tab.file_path_input.is_empty() {
            tab.file_path_input.clone()
        } else {
            format!("Untitled {}", tab.id)
        };
        let (cursor_line, cursor_col) = if active {
            (self.editor_state.cursor_line, self.editor_state.cursor_col)
        } else {
            (tab.editor_state.cursor_line, tab.editor_state.cursor_col)
        };
        (tab.id, dirty, path, title, content, cursor_line, cursor_col)
    }

    fn restore_recovery_entry(&mut self, entry: &RecoveryEntry) {
        self.new_tab();
        self.doc.current_file = entry.original_path.clone();
        if let Some(path) = &entry.original_path {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                self.doc.file_path_input = name.to_string();
            }
        } else if !entry.title.is_empty() {
            self.doc.file_path_input = entry.title.clone();
        }

        self.set_text(&entry.content);
        self.doc.last_snapshot = Rope::from_str(&entry.content);
        self.doc.last_disk_mtime = entry
            .original_path
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok());
        self.doc.dirty = true;

        let line = entry
            .cursor_line
            .min(self.doc.line_count().saturating_sub(1));
        let col = entry
            .cursor_col
            .min(self.doc.line(line).chars().count());
        self.editor_state.cursor_line = line;
        self.editor_state.cursor_col = col;
        self.editor_state.sel_anchor = None;
        self.editor_state.scroll = 0.0;
        self.sync_active_to_tab();
    }

    /// S5 (Etapa 2): Retorna la paleta de colores del tema activo.
    pub fn theme(&self) -> crate::theme::Theme {
        self.theme_mode.theme()
    }

    /// S6 (Etapa 2): Retorna el lenguaje detectado para el documento actual (autodetecta por extensión y contenido).
    ///
    /// T2 (S1): cuando hay ruta, el resultado se cachea por ruta — la
    /// extensión/filename no cambia mientras se escribe, así que no hace
    /// falta recalcularlo en cada tecla. Sin ruta la detección depende
    /// del contenido, así que no se cachea (y la muestra se arma con las
    /// primeras 100 líneas, sin materializar el documento entero).
    pub fn current_language(&self) -> crate::syntax::Language {
        let path = self.doc.current_file.as_deref().or_else(|| {
            if self.doc.file_path_input.is_empty() {
                None
            } else {
                Some(std::path::Path::new(&self.doc.file_path_input))
            }
        });

        let Some(path) = path else {
            return crate::syntax::Language::detect(None, &self.language_sample());
        };

        if let Some((cached_path, language)) = self.current_language_cache.borrow().as_ref() {
            if cached_path == path {
                return *language;
            }
        }

        let language = crate::syntax::Language::detect(Some(path), &self.language_sample());
        *self.current_language_cache.borrow_mut() = Some((path.to_path_buf(), language));
        language
    }

    /// T2 (S1): muestra de como mucho 100 líneas para la detección por
    /// contenido, armada con `line()` para no reconstruir el `Rope`
    /// completo en un `String`.
    fn language_sample(&self) -> String {
        let line_count = self.doc.line_count().min(100);
        let mut sample = String::new();
        for i in 0..line_count {
            if i > 0 {
                sample.push('\n');
            }
            sample.push_str(&self.doc.line(i));
        }
        sample
    }

    /// S8 (Etapa 2): Guarda el estado actual del editor en la pestaña activa.
    pub fn sync_active_to_tab(&mut self) {
        // T1: se comparte el `Rope` (clon O(1)) en lugar de materializar
        // el texto completo y reconstruir el buffer de la pestaña en
        // cada tecla.
        let buffer = self.doc.buffer().clone();
        if let Some(tab) = self.doc.tabs.get_mut(self.doc.active_tab) {
            tab.set_buffer(buffer);
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
            tab.editor_state.preferred_col = self.editor_state.preferred_col;
            tab.editor_state.scroll = self.editor_state.scroll;
        }
    }

    /// T2 (S1): marca que el AST y el LSP quedaron desactualizados por una
    /// edición. El bucle por frame los aplica cuando pasan 120 ms sin
    /// teclas nuevas, en una sola pasada, en vez de en cada tecla.
    pub fn mark_full_text_sync_pending(&mut self) {
        self.pending_full_text_sync = Some(Instant::now());
    }

    /// T2 (S1): aplica ya la sincronización diferida. Guardar, cambiar de
    /// pestaña, ir a definición, pedir hover o formatear no pueden esperar
    /// a que el usuario deje de escribir.
    pub fn flush_full_text_sync(&mut self) {
        if self.pending_full_text_sync.take().is_some() {
            self.update_ast();
            self.update_lsp();
        }
    }

    /// T2 (S1): avanza el debounce del texto completo. Devuelve el tiempo
    /// que falta para aplicarlo, o `None` si ya se aplicó (o no hay nada
    /// pendiente); el frame usa ese `Some` para pedir el repintado.
    pub fn poll_full_text_sync(&mut self, now: Instant) -> Option<Duration> {
        let since = self.pending_full_text_sync?;
        let elapsed = now.saturating_duration_since(since);
        if elapsed >= FULL_TEXT_SYNC_DEBOUNCE {
            self.pending_full_text_sync = None;
            self.update_ast();
            self.update_lsp();
            None
        } else {
            Some(FULL_TEXT_SYNC_DEBOUNCE - elapsed)
        }
    }

    /// T2 (S1): guarda el documento activo, aplicando primero el AST/LSP
    /// diferido para que el guardado no deje atrás un estado viejo.
    pub fn save_document(&mut self) {
        self.flush_full_text_sync();
        self.doc.try_save();
    }

    /// T2 (S1): "Guardar como…" con la misma sincronización previa.
    pub fn save_document_as(&mut self) {
        self.flush_full_text_sync();
        self.doc.save_as();
    }

    /// T2 (S4): ejecuta una acción del menú File. El `ctx` solo hace falta
    /// para `Exit`, que delega en `try_close` (diálogo de cambios sin
    /// guardar); el resto son acciones de estado sin UI.
    pub fn apply_file_menu_action(&mut self, action: FileMenuAction, ctx: Option<&egui::Context>) {
        match action {
            FileMenuAction::NewTab => self.new_tab(),
            FileMenuAction::OpenFile => self.picker_open = true,
            FileMenuAction::OpenFolder => self.prompt_open_folder(),
            FileMenuAction::Save => self.save_document(),
            FileMenuAction::SaveAs => self.save_document_as(),
            FileMenuAction::SearchFiles => self
                .quick_open
                .request_open(&self.doc.browse_dir, &mut self.quick_open_services),
            FileMenuAction::SearchInWorkspace => self.open_workspace_search(),
            FileMenuAction::CloseTab => {
                let idx = self.doc.active_tab;
                self.try_close_tab(idx);
            }
            FileMenuAction::Exit => {
                if let Some(ctx) = ctx {
                    self.try_close(ctx);
                }
            }
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
        // T2 (S1): el AST/LSP diferido es del documento que se está
        // dejando atrás; se aplica antes de cambiar el buffer activo.
        self.flush_full_text_sync();
        if idx < self.doc.tabs.len() {
            // T1: camino inverso, también compartiendo el `Rope`
            // (clon O(1)) en lugar de pasar por un `String`.
            let buffer = self.doc.tabs[idx].buffer().clone();
            self.doc.active_tab = idx;
            self.doc.set_buffer(buffer);
            self.editor_state = self.doc.tabs[idx].editor_state.clone();
            self.doc.last_snapshot = self.doc.tabs[idx].last_snapshot.clone();
            self.doc.undo_stack = self.doc.tabs[idx].undo_stack.clone();
            self.doc.file_path_input = self.doc.tabs[idx].file_path_input.clone();
            self.doc.current_file = self.doc.tabs[idx].current_file.clone();
            self.doc.dirty = self.doc.tabs[idx].dirty;
            self.doc.line_ending = self.doc.tabs[idx].line_ending;
            self.doc.last_disk_mtime = self.doc.tabs[idx].last_disk_mtime;
            self.doc.external_changed = self.doc.tabs[idx].external_changed;
            self.doc.last_mtime_check = self.doc.tabs[idx].last_mtime_check;
            self.doc.view_bytes = self.doc.tabs[idx].view_bytes;
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

        // v0.2.51: una pestaña que se cierra ya no tiene trabajo que
        // recuperar; su journal se elimina antes de sacarla del vector.
        self.cleanup_recovery_for_tab(idx);

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
        self.doc.set_text(&text);
        self.editor_state.cursor_line = 0;
        self.editor_state.cursor_col = 0;
        self.editor_state.preferred_col = None;
        self.editor_state.sel_anchor = None;
        self.editor_state.scroll = 0.0;
        self.doc.chunk_tracker.reset();
        self.doc.recalc_view_bytes();
        self.update_ast();
        self.update_lsp();
        // T2 (S1): acá ya se aplicó el texto completo, así que no queda
        // nada diferido que hacer.
        self.pending_full_text_sync = None;
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
            self.editor_state.sticky_scopes.clear();
            self.editor_state.symbols.clear();
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
        // F7 (Roadmap v0.2.34): símbolos del documento para Ctrl+Shift+O.
        self.editor_state.symbols = result.symbols;
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

    /// Barra lateral unificada (v0.2.39): abre/cierra el explorador y refresca su lista.
    pub fn toggle_explorer(&mut self) {
        if self.sidebar_tab == SidebarTab::Explorer {
            self.sidebar_tab = SidebarTab::None;
            self.explorer.open = false;
        } else {
            self.sidebar_tab = SidebarTab::Explorer;
            self.explorer.open = true;
            self.workspace_search.open = false;
            self.refresh_explorer();
        }
    }

    /// Barra lateral: recarga la lista de archivos y carpetas del proyecto.
    /// Reusa el recorrido del buscador global (mismas exclusiones y sin
    /// binarios); las carpetas vacías también entran al árbol (v0.2.42).
    pub fn refresh_explorer(&mut self) {
        let root = self.doc.browse_dir.clone();
        if self.explorer.loaded_for.as_ref() == Some(&root)
            && (!self.explorer.files.is_empty() || !self.explorer.directories.is_empty())
        {
            return;
        }
        let (mut files, mut directories) = karkinos_core::workspace_search::walk_entries(&root);
        files.sort();
        directories.sort();
        let tree = FileTreeNode::build_from_paths_and_dirs(&root, &files, &directories);
        if self.explorer.collapsed_dirs.is_empty() {
            let mut dirs = Vec::new();
            FileTreeNode::collect_directories(&tree, &mut dirs);
            self.explorer.collapsed_dirs.extend(dirs);
        }
        self.explorer.files = files;
        self.explorer.directories = directories;
        self.explorer.loaded_for = Some(root);
    }

    /// v0.2.52: crea el ítem pendiente del popup "Open…". Un archivo se
    /// abre en una pestaña nueva y cierra el popup; una carpeta queda en
    /// disco y el selector sigue abierto para seguir navegando.
    pub fn picker_create_item(&mut self) -> Result<std::path::PathBuf, String> {
        let Some(kind) = self.picker_new_kind else {
            return Err("No item is being created".to_string());
        };

        let name = self.picker_new_name.trim().to_string();
        if !is_valid_explorer_item_name(&name) {
            let msg = "Invalid name: use a single file or folder name".to_string();
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let path = self.doc.browse_dir.join(&name);
        if path.exists() {
            let msg = format!("Already exists: {}", path.display());
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let created = match kind {
            PickerNewItemKind::File => std::fs::File::create(&path).map(|_| ()),
            PickerNewItemKind::Folder => std::fs::create_dir(&path),
        };
        if let Err(e) = created {
            let msg = format!("Could not create {}: {}", name, e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        // El listado cacheado del popup tiene que reflejar el ítem nuevo.
        self.doc.invalidate_dir_cache();
        self.picker_new_name.clear();
        self.picker_new_kind = None;

        if kind == PickerNewItemKind::File {
            self.picker_open = false;
            self.picker_button_rect = None;
            self.open_path(path.clone());
        }
        self.doc.status = format!("Created {}", name);
        Ok(path)
    }

    /// Roadmap v0.2.45 (Tarea C): cambia la carpeta raíz del workspace.
    /// Refresca explorador y Git, descarta el estado de búsqueda global
    /// y el visor de diff (ambos quedan inválidos para la carpeta nueva)
    /// y notifica el resultado en la barra de estado.
    pub fn open_workspace_folder(&mut self, folder: &std::path::Path) {
        self.doc.browse_dir = folder.to_path_buf();
        self.refresh_explorer();
        self.refresh_git_status();
        self.refresh_git_branch();

        // La búsqueda global apuntaba al workspace anterior: sus resultados
        // ya no corresponden. Subir la generación descarta los eventos
        // pendientes del worker.
        self.workspace_search.generation = self.workspace_search.generation.wrapping_add(1);
        self.workspace_search.results.clear();
        self.workspace_search.total_matches = 0;
        self.workspace_search.files_scanned = 0;
        self.workspace_search.replace_summary = None;
        self.workspace_search.error = None;
        self.workspace_search.searching = false;

        // El diff del workspace anterior tampoco aplica.
        self.git_diff_modal = None;

        self.doc.status = format!("Opened folder: {}", folder.display());
    }

    /// Roadmap v0.2.45 (Tarea C): abre el modal "Open Folder" con la ruta
    /// actual prellenada. Cierra el selector de carpetas y el Command
    /// Palette, para que solo un modal tenga el foco.
    pub fn prompt_open_folder(&mut self) {
        self.picker_open = false;
        self.picker_button_rect = None;
        self.command_palette.open = false;
        self.command_palette.query.clear();
        self.open_folder_modal = Some(self.doc.browse_dir.display().to_string());
    }

    /// Roadmap v0.2.45 (Tarea C): confirma la ruta del modal. Si es un
    /// directorio existente, cambia el workspace y cierra el modal; si no,
    /// avisa en la barra de estado y deja el modal abierto para corregir.
    pub fn confirm_open_folder(&mut self) {
        let Some(raw) = self.open_folder_modal.clone() else {
            return;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        // La tilde (`~`) no la expande `Path` por su cuenta: la resolvemos
        // contra el home (y, si sigue relativa, contra la raíz actual)
        // antes de comprobar si el destino es un directorio.
        let path = crate::util::expand_user_path(trimmed, &self.doc.browse_dir);
        if path.is_dir() {
            self.open_folder_modal = None;
            self.open_workspace_folder(&path);
        } else {
            self.doc.status = format!("Not a folder: {}", trimmed);
        }
    }

    /// v0.2.44: watcher periódico (cadencia 1.5 s) de estado de Git y de
    /// archivos abiertos modificados por fuera del editor.
    pub fn poll_external_changes(&mut self) {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_disk_poll {
            if now.duration_since(last) < std::time::Duration::from_millis(1500) {
                return;
            }
        }
        self.last_disk_poll = Some(now);

        self.refresh_git_branch();
        self.poll_git_metadata_changes();
        self.poll_open_tab_disk_changes();
    }

    /// v0.2.44: refresca `git status` y la rama cuando cambia `.git/HEAD`
    /// o `.git/index`. La primera lectura solo memoriza el mtime.
    fn poll_git_metadata_changes(&mut self) {
        let git_dir = self.git_repo_root().join(".git");
        let mut git_mtime: Option<std::time::SystemTime> = None;
        for entry in ["HEAD", "index"] {
            let Ok(metadata) = std::fs::metadata(git_dir.join(entry)) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            git_mtime = Some(match git_mtime {
                Some(current) => current.max(modified),
                None => modified,
            });
        }

        let changed = match (git_mtime, self.last_git_mtime) {
            (Some(current), Some(previous)) => current != previous,
            _ => false,
        };
        self.last_git_mtime = git_mtime;

        if changed {
            self.refresh_git_status();
        }
    }

    /// v0.2.44: recarga las pestañas limpias cuyo archivo cambió en disco y
    /// solo notifica en la barra de estado cuando hay ediciones sin guardar.
    fn poll_open_tab_disk_changes(&mut self) {
        // La pestaña activa guarda su `dirty` y su mtime en `doc`; sin esta
        // sincronización el watcher compararía metadatos viejos.
        self.sync_active_metadata_to_tab();

        let active = self.doc.active_tab;
        for idx in 0..self.doc.tabs.len() {
            let Some(path) = self.doc.tabs[idx].current_file.clone() else {
                continue;
            };
            let disk = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok());
            if !crate::document_service::mtime_is_stale(disk, self.doc.tabs[idx].last_disk_mtime) {
                continue;
            }

            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());

            if self.doc.tabs[idx].dirty {
                // Nunca pisar ediciones locales: solo avisar.
                self.doc.status = format!("File modified on disk: {name}");
                continue;
            }

            let Ok((content, line_ending, mtime)) = DocumentService::read_file(&path) else {
                continue;
            };
            let view_bytes = content.len();
            let tab = &mut self.doc.tabs[idx];
            tab.set_text(&content);
            tab.last_snapshot = Rope::from_str(&content);
            tab.undo_stack = UndoStack::new();
            tab.dirty = false;
            tab.line_ending = line_ending;
            tab.last_disk_mtime = mtime;
            tab.external_changed = false;
            tab.last_mtime_check = None;
            tab.view_bytes = view_bytes;

            if idx == active {
                self.load_tab(idx);
            }
        }
    }

    /// F8: consume los blame que terminó el worker (cacheados por archivo).
    pub fn poll_blame_results(&mut self) -> bool {
        let mut applied = false;
        while let Some((path, lines)) = self.blame_worker.try_recv() {
            self.git_status.blame_cache.insert(path, lines);
            applied = true;
        }
        applied
    }

    /// F8: actualiza el ghost text del blame. Se consulta recién cuando el
    /// cursor quedó quieto 300 ms; al moverse se limpia y arranca de nuevo.
    pub fn update_inline_blame(&mut self) {
        let Some(path) = self.doc.current_file.clone() else {
            self.editor_state.blame_text = None;
            return;
        };

        let cursor = (self.editor_state.cursor_line, self.editor_state.cursor_col);
        if cursor != self.git_status.last_cursor {
            self.git_status.last_cursor = cursor;
            self.git_status.cursor_settled_at = Some(std::time::Instant::now());
            self.editor_state.blame_text = None;
            return;
        }

        match self.git_status.cursor_settled_at {
            Some(at) if at.elapsed() >= std::time::Duration::from_millis(300) => {}
            _ => return,
        }

        if !self.git_status.blame_cache.contains_key(&path) {
            if self.git_status.blame_requested_for.as_ref() != Some(&path) {
                self.git_status.blame_requested_for = Some(path.clone());
                self.blame_worker.request(path);
            }
            return;
        }

        let line_number = self.editor_state.cursor_line + 1;
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.editor_state.blame_text = self
            .git_status
            .blame_cache
            .get(&path)
            .and_then(|lines| lines.get(&line_number))
            .map(|blame| karkinos_core::git_status::format_blame_line(blame, now_unix));
    }

    /// F8: tiempo restante hasta cumplir el dwell de 300 ms del blame inline.
    /// Devuelve `Some` solo si el cursor se movió hace menos de 300 ms (todavía
    /// hay que despertar para consultar el blame); `None` significa reposo: sin
    /// repaint adicional. Así un archivo sin commit no fuerza un loop continuo.
    pub fn blame_repaint_remaining(&self) -> Option<std::time::Duration> {
        let at = self.git_status.cursor_settled_at?;
        let elapsed = at.elapsed();
        if elapsed < std::time::Duration::from_millis(300) {
            Some(std::time::Duration::from_millis(300) - elapsed)
        } else {
            None
        }
    }

    /// F7 (Roadmap v0.2.34 / v0.2.39): abre el selector de símbolos en la barra lateral unificada.
    pub fn open_symbol_picker(&mut self) {
        if self.sidebar_tab == SidebarTab::Symbols {
            self.sidebar_tab = SidebarTab::None;
            self.symbol_picker.open = false;
        } else {
            self.sidebar_tab = SidebarTab::Symbols;
            self.symbol_picker.open = true;
            self.symbol_picker.query.clear();
            self.symbol_picker.selected = 0;
            self.explorer.open = false;
            self.workspace_search.open = false;
        }
    }

    /// v0.2.42: abre (o cierra) la pestaña Source Control y refresca el estado.
    pub fn open_source_control(&mut self) {
        if self.sidebar_tab == SidebarTab::SourceControl {
            self.sidebar_tab = SidebarTab::None;
        } else {
            self.sidebar_tab = SidebarTab::SourceControl;
            self.explorer.open = false;
            self.workspace_search.open = false;
            self.symbol_picker.open = false;
        }
        self.refresh_git_status();
    }

    /// Abre (o cierra) la pestaña Extensions en la barra lateral unificada.
    pub fn open_extensions(&mut self) {
        if self.sidebar_tab == SidebarTab::Extensions {
            self.sidebar_tab = SidebarTab::None;
        } else {
            self.sidebar_tab = SidebarTab::Extensions;
            self.explorer.open = false;
            self.workspace_search.open = false;
            self.symbol_picker.open = false;
            if self.extensions.open_vsx_results.is_empty()
                && !self.extensions.open_vsx_searching
                && self.extensions_tab == 2
            {
                self.extensions.search_open_vsx("rust");
            }
        }
    }

    /// F7: símbolos filtrados por el texto del selector (sin distinguir
    /// mayúsculas). Sale del AST del worker, no de LSP.
    pub fn filtered_symbols(&self) -> Vec<&karkinos_core::syntax_ast::DocumentSymbol> {
        let needle = self.symbol_picker.query.trim().to_lowercase();
        self.editor_state
            .symbols
            .iter()
            .filter(|symbol| needle.is_empty() || symbol.name.to_lowercase().contains(&needle))
            .collect()
    }

    /// F7: lleva el cursor al símbolo y cierra el selector.
    pub fn jump_to_symbol(&mut self, symbol: &karkinos_core::syntax_ast::DocumentSymbol) {
        let line = symbol.line.min(self.doc.line_count().saturating_sub(1));
        self.editor_state.cursor_line = line;
        self.editor_state.cursor_col = symbol.character;
        self.editor_state.preferred_col = None;
        self.editor_state.sel_anchor = Some((line, symbol.character));
        self.editor_state.request_autoscroll();
        self.symbol_picker.open = false;
    }

    /// UI-7 (v0.2.38): alterna el minimap en el editor primario y en el
    /// panel secundario del split, para que ambos mantengan el mismo layout.
    pub fn toggle_minimap(&mut self) {
        let visible = !self.editor_state.show_minimap;
        self.editor_state.show_minimap = visible;
        if let Some(split) = self.split.as_mut() {
            split.secondary.show_minimap = visible;
        }
    }

    /// S11: alterna la animación fluida tipo smear del cursor.
    pub fn toggle_smear_cursor(&mut self) {
        let enabled = !self.editor_state.smear_enabled;
        self.editor_state.smear_enabled = enabled;
        for tab in &mut self.doc.tabs {
            tab.editor_state.smear_enabled = enabled;
        }
        if let Some(split) = self.split.as_mut() {
            split.secondary.smear_enabled = enabled;
        }
        self.doc.status = format!("Smear cursor: {}", if enabled { "Enabled" } else { "Disabled" });
    }

    /// v0.2.40: abre el Command Palette con la búsqueda limpia. Si ya está
    /// abierto no reinicia el query: F1 de nuevo no debe borrar lo escrito.
    pub fn open_command_palette(&mut self) {
        if self.command_palette.open {
            return;
        }
        // Un solo modal con foco: si Enter ejecutara dos handlers a la vez
        // (p. ej. el selector de símbolos y el palette) sería ambiguo.
        self.quick_open.close();
        self.symbol_picker.open = false;
        self.lsp.code_actions_menu_open = false;
        self.lsp.rename_dialog_open = false;
        self.goto_pending = false;
        self.picker_open = false;
        self.command_palette.open = true;
        self.command_palette.query.clear();
        self.command_palette.selected = 0;
    }

    /// v0.2.40: comandos del catálogo filtrados por coincidencia sin
    /// distinguir mayúsculas sobre `category` y `title`.
    pub fn filtered_palette_commands(&self) -> Vec<&'static PaletteCommand> {
        let needle = self.command_palette.query.trim().to_lowercase();
        PALETTE_COMMANDS
            .iter()
            .filter(|command| {
                needle.is_empty()
                    || command.category.to_lowercase().contains(&needle)
                    || command.title.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// v0.2.40: mueve la selección del palette y la mantiene dentro de la
    /// lista filtrada (las flechas en los extremos no se escapan).
    pub fn move_palette_selection(&mut self, delta: isize) {
        let count = self.filtered_palette_commands().len();
        if count == 0 {
            self.command_palette.selected = 0;
            return;
        }
        let next = self.command_palette.selected as isize + delta;
        self.command_palette.selected = next.clamp(0, count as isize - 1) as usize;
    }

    /// v0.2.40: ejecuta el comando seleccionado (Enter en el modal).
    pub fn execute_selected_palette_command(&mut self) {
        let id = self
            .filtered_palette_commands()
            .get(self.command_palette.selected)
            .map(|command| command.id);
        if let Some(id) = id {
            self.execute_palette_command(id);
        }
    }

    /// v0.2.40: ejecuta un comando del palette por id. El modal siempre se
    /// cierra, incluso si el id no existe (no deja la UI colgada).
    pub fn execute_palette_command(&mut self, id: &str) {
        self.command_palette.open = false;
        self.command_palette.query.clear();
        self.command_palette.selected = 0;

        match id {
            "layout:split_vertical" => {
                self.toggle_split(SplitDirection::Vertical);
            }
            "layout:split_horizontal" => {
                self.toggle_split(SplitDirection::Horizontal);
            }
            "layout:close_split" => {
                if self.split.is_some() {
                    self.split = None;
                    self.doc.status = "Split closed".to_string();
                }
            }
            "editor:toggle_minimap" => self.toggle_minimap(),
            "editor:toggle_smear_cursor" => self.toggle_smear_cursor(),
            "sidebar:explorer" => self.toggle_explorer(),
            "sidebar:search" => self.open_workspace_search(),
            "search:replace_in_workspace" => self.open_workspace_replace(),
            "search:toggle_replace" => {
                self.workspace_search.show_replace = !self.workspace_search.show_replace;
            }
            "sidebar:symbols" => self.open_symbol_picker(),
            "sidebar:extensions" => self.open_extensions(),
            "git:source_control" => self.open_source_control(),
            "git:refresh" => self.refresh_git_status(),
            "git:stage_all" => {
                let _ = self.git_stage_all();
            }
            "git:view_diff" => {
                if let Some(path) = self.doc.current_file.clone() {
                    self.open_git_diff(&path, false);
                }
            }
            "git:close_diff" => self.close_git_diff(),
            "terminal:toggle" => {
                if self.cargo.panel_open && self.cargo.active_tab == 0 {
                    self.cargo.panel_open = false;
                } else {
                    self.cargo.panel_open = true;
                    self.cargo.active_tab = 0;
                }
            }
            "cargo:check" => self.run_cargo(karkinos_core::cargo::CargoAction::Check),
            "cargo:test" => self.run_cargo(karkinos_core::cargo::CargoAction::Test),
            "cargo:clippy" => self.run_cargo(karkinos_core::cargo::CargoAction::Clippy),
            "cargo:run" => self.run_cargo(karkinos_core::cargo::CargoAction::Run),
            "debug:toggle_breakpoint" => {
                let line = self.editor_state.cursor_line;
                self.toggle_breakpoint(line);
            }
            "debug:start" => {
                self.cargo.panel_open = true;
                self.cargo.active_tab = 2;
                self.start_debugging();
            }
            "debug:step_over" => self.step_over(),
            "debug:step_into" => self.step_in(),
            "debug:stop" => self.stop_debugging(),
            "theme:dark" => {
                self.theme_mode = crate::theme::ThemeMode::Dark;
                self.doc.status = "Theme: Dark".to_string();
            }
            "theme:monokai" => {
                self.theme_mode = crate::theme::ThemeMode::Monokai;
                self.doc.status = "Theme: Monokai".to_string();
            }
            "theme:dracula" => {
                self.theme_mode = crate::theme::ThemeMode::Dracula;
                self.doc.status = "Theme: Dracula".to_string();
            }
            "theme:onedark" => {
                self.theme_mode = crate::theme::ThemeMode::OneDark;
                self.doc.status = "Theme: One Dark".to_string();
            }
            "editor:format" => self.format_document(),
            "file:save" => {
                let _ = self.doc.try_save();
            }
            "file:new" => self.try_new_document(),
            "file:open_folder" => self.prompt_open_folder(),
            "file:rename_active" => self.begin_rename_active_file(),
            "file:duplicate_active" => {
                if let Some(path) = self.doc.current_file.clone() {
                    let _ = self.explorer_duplicate_item(&path);
                }
            }
            "view:zoom_in" => self.zoom(true),
            "view:zoom_out" => self.zoom(false),
            "view:reset_zoom" => self.reset_zoom(),
            _ => {}
        }
    }

    /// v0.2.40: formatea el buffer activo con `rustfmt` (stdin → stdout).
    /// Solo aplica a Rust; el resto reporta el motivo en la barra de estado.
    pub fn format_document(&mut self) {
        use std::io::Write;

        // T2 (S1): rustfmt trabaja sobre el texto actual; el AST/LSP
        // diferido se aplica antes de reemplazar el buffer.
        self.flush_full_text_sync();
        if self.current_language() != crate::syntax::Language::Rust {
            self.doc.status = "Format: rustfmt only supports Rust files".to_string();
            return;
        }

        let original = self.doc.current_text();
        let mut child = match std::process::Command::new("rustfmt")
            .args(["--edition", "2021", "--emit", "stdout"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                self.doc.status = format!("Format failed: {error}");
                return;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(error) = stdin.write_all(original.as_bytes()) {
                self.doc.status = format!("Format failed: {error}");
                return;
            }
        }

        match child.wait_with_output() {
            Ok(output) if output.status.success() => {
                let formatted = String::from_utf8_lossy(&output.stdout).to_string();
                if formatted == original {
                    self.doc.status = "Already formatted".to_string();
                } else {
                    self.apply_formatted_text(&formatted);
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.doc.status = format!("Format failed: {}", stderr.trim());
            }
            Err(error) => {
                self.doc.status = format!("Format failed: {error}");
            }
        }
    }

    /// v0.2.40: aplica el texto ya formateado como una edición undoable,
    /// preservando cursor y scroll. Mismo contrato que `apply_text_edits`.
    fn apply_formatted_text(&mut self, formatted: &str) {
        self.doc.snapshot_if_changed();
        self.restore_text_keeping_view(formatted);
        self.doc.snapshot_if_changed();
        self.doc.dirty = true;
        self.doc.status = "Formatted with rustfmt".to_string();
    }

    /// F6 (Roadmap v0.2.32 / v0.2.39): abre la búsqueda global en la barra lateral unificada y pide foco.
    pub fn open_workspace_search(&mut self) {
        if self.sidebar_tab == SidebarTab::Search {
            self.sidebar_tab = SidebarTab::None;
            self.workspace_search.open = false;
        } else {
            self.sidebar_tab = SidebarTab::Search;
            self.workspace_search.open = true;
            self.explorer.open = false;
            self.symbol_picker.open = false;
            self.workspace_search.focus_requested = true;
        }
    }

    /// F6: dispara una búsqueda nueva en el worker. Sube la generación para
    /// que los eventos de búsquedas anteriores se descarten al llegar.
    pub fn trigger_workspace_search(&mut self) {
        let query = karkinos_core::SearchQuery {
            query: self.workspace_search.input.clone(),
            case_sensitive: self.workspace_search.case_sensitive,
            match_word: self.workspace_search.match_word,
            use_regex: self.workspace_search.use_regex,
            max_results: 500,
            file_pattern: self.workspace_file_pattern(),
        };
        self.workspace_search.generation = self.workspace_search.generation.wrapping_add(1);
        self.workspace_search.results.clear();
        self.workspace_search.total_matches = 0;
        self.workspace_search.files_scanned = 0;
        self.workspace_search.replace_summary = None;
        self.workspace_search.error = query.validate().err();
        self.workspace_search.searching =
            self.workspace_search.error.is_none() && !query.is_empty();
        if self.workspace_search.searching {
            let root = self.doc.browse_dir.clone();
            self.workspace_search_engine
                .request(self.workspace_search.generation, &root, query);
        }
    }

    /// v0.2.43: filtro de archivos activo, o `None` si el campo está vacío.
    fn workspace_file_pattern(&self) -> Option<String> {
        let filter = self.workspace_search.file_filter.trim();
        if filter.is_empty() {
            None
        } else {
            Some(filter.to_string())
        }
    }

    /// v0.2.43: abre la búsqueda global con el reemplazo desplegado y el
    /// foco puesto en el campo de reemplazo (Ctrl+Shift+H).
    pub fn open_workspace_replace(&mut self) {
        self.sidebar_tab = SidebarTab::Search;
        self.workspace_search.open = true;
        self.explorer.open = false;
        self.symbol_picker.open = false;
        self.workspace_search.show_replace = true;
        self.workspace_search.replace_focus_requested = true;
    }

    /// v0.2.43: reemplaza todas las coincidencias en el workspace y
    /// sincroniza las pestañas abiertas con el contenido que quedó en disco.
    pub fn execute_workspace_replace(
        &mut self,
    ) -> Result<karkinos_core::workspace_search::ReplaceSummary, String> {
        use karkinos_core::workspace_search::{ReplaceSummary, replace_in_workspace};

        let query = karkinos_core::SearchQuery {
            query: self.workspace_search.input.clone(),
            case_sensitive: self.workspace_search.case_sensitive,
            match_word: self.workspace_search.match_word,
            use_regex: self.workspace_search.use_regex,
            max_results: 500,
            file_pattern: self.workspace_file_pattern(),
        };

        // Contenido en disco de las pestañas abiertas ANTES de reemplazar:
        // solo esas pestañas pueden necesitar recarga después.
        let open_files = self.capture_open_tab_disk_contents();

        let result = replace_in_workspace(
            &self.doc.browse_dir,
            &query,
            &self.workspace_search.replace_input,
        );
        // Aun si el lote falla a mitad de camino, las pestañas abiertas
        // deben reflejar lo que efectivamente quedó en disco.
        self.reload_tabs_after_replace(open_files);

        let summary: ReplaceSummary = match result {
            Ok(summary) => summary,
            Err(error) => {
                self.workspace_search.replace_summary = None;
                self.doc.status = format!("Replace failed: {error}");
                return Err(error);
            }
        };

        // Las coincidencias en memoria ya no reflejan el disco.
        self.trigger_workspace_search();

        let message = format!(
            "Replaced {} occurrences across {} files",
            summary.replacements_count, summary.files_modified
        );
        self.workspace_search.replace_summary = Some(message.clone());
        self.doc.status = message;
        Ok(summary)
    }

    /// v0.2.43: lee el contenido en disco actual de cada pestaña abierta.
    fn capture_open_tab_disk_contents(&self) -> Vec<(usize, std::path::PathBuf, String)> {
        let mut open_files = Vec::new();
        for (idx, tab) in self.doc.tabs.iter().enumerate() {
            let Some(path) = tab.current_file.clone() else {
                continue;
            };
            if let Ok((content, _, _)) = DocumentService::read_file(&path) {
                open_files.push((idx, path, content));
            }
        }
        open_files
    }

    /// v0.2.43: recarga desde disco solo las pestañas cuyo archivo cambió
    /// con el reemplazo y vuelca la pestaña activa al buffer del documento.
    fn reload_tabs_after_replace(&mut self, open_files: Vec<(usize, std::path::PathBuf, String)>) {
        // La pestaña activa puede tener ediciones todavía no volcadas.
        self.sync_active_to_tab();

        let active = self.doc.active_tab;
        let mut active_changed = false;
        for (idx, path, before) in open_files {
            let Ok((content, line_ending, mtime)) = DocumentService::read_file(&path) else {
                continue;
            };
            if content == before {
                // El reemplazo no tocó este archivo: una pestaña con
                // ediciones sin guardar conserva su buffer y su `dirty`.
                continue;
            }
            let view_bytes = content.len();
            let tab = &mut self.doc.tabs[idx];
            tab.set_text(&content);
            tab.last_snapshot = Rope::from_str(&content);
            tab.undo_stack = UndoStack::new();
            tab.dirty = false;
            tab.line_ending = line_ending;
            tab.last_disk_mtime = mtime;
            tab.external_changed = false;
            tab.last_mtime_check = None;
            tab.view_bytes = view_bytes;
            if idx == active {
                active_changed = true;
            }
        }

        // Solo la pestaña activa recargada necesita volver al buffer vivo.
        if active_changed {
            self.load_tab(active);
        }
    }

    /// F6: consume los eventos del worker. `true` si hubo novedades (repaint).
    pub fn poll_workspace_search(&mut self) -> bool {
        let mut applied = false;
        while let Some((generation, event)) = self.workspace_search_engine.try_recv() {
            if generation != self.workspace_search.generation {
                continue; // resultado de una búsqueda que ya quedó vieja
            }
            applied = true;
            match event {
                karkinos_core::SearchEvent::File(file) => {
                    self.workspace_search.total_matches += file.matches.len();
                    self.workspace_search.results.push(file);
                }
                karkinos_core::SearchEvent::Done { files_scanned, .. } => {
                    self.workspace_search.files_scanned = files_scanned;
                    self.workspace_search.searching = false;
                }
            }
        }
        applied
    }

    /// F6: abre el archivo del resultado y posiciona el cursor exactamente en
    /// la coincidencia (línea y columna), con la palabra seleccionada.
    pub fn jump_to_search_match(
        &mut self,
        path: &std::path::Path,
        m: &karkinos_core::SearchMatch,
    ) {
        self.open_path(path.to_path_buf());
        let line = m
            .line_number
            .saturating_sub(1)
            .min(self.doc.line_count().saturating_sub(1));
        self.editor_state.cursor_line = line;
        self.editor_state.cursor_col = m.col_start;
        self.editor_state.preferred_col = None;
        self.editor_state.sel_anchor = Some((line, m.col_end));
        self.editor_state.request_autoscroll();
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
            self.editor_state.preferred_col = None;
            self.editor_state.needs_autoscroll = true;
            self.doc.status = format!("Jumped to {}: line {}", diag.level, diag.line + 1);
        }
    }

    /// S2 (Rust IDE): Chequea y procesa respuestas entrantes de Go to Definition, Hover, Code Actions, Inlay Hints y Rename
    pub fn update_lsp(&mut self) {
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.update_lsp(&mut self.editor_state, &mut self.doc, is_rust);
    }

    /// S2 (Rust IDE): Solicita saltar a la definición del símbolo bajo el cursor (F12)
    pub fn goto_definition(&mut self) {
        self.flush_full_text_sync();
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.goto_definition(&self.editor_state, &mut self.doc, is_rust);
    }

    /// S2 (Rust IDE): Solicita información de hover / documentación para el símbolo bajo el cursor
    pub fn request_hover(&mut self) {
        self.flush_full_text_sync();
        let is_rust = self.current_language() == crate::syntax::Language::Rust;
        self.lsp.request_hover(&self.editor_state, &self.doc, is_rust);
    }

    /// Requests code actions (quick-fixes, auto-imports) for current cursor position and diagnostics.
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

    /// Requests workspace rename of symbol at cursor.
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
                .line_col_to_char(edit.range.start.line, edit.range.start.character);
            let end_char = self
                .doc
                .line_col_to_char(edit.range.end.line, edit.range.end.character);

            if start_char < end_char {
                self.doc.remove(start_char..end_char);
            }
            if !edit.new_text.is_empty() {
                self.doc.insert(start_char, &edit.new_text);
            }
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
                    .line_col_to_char(edit.range.start.line, edit.range.start.character);
                let end_char = tab
                    .line_col_to_char(edit.range.end.line, edit.range.end.character);

                if start_char < end_char {
                    tab.remove(start_char..end_char);
                }
                if !edit.new_text.is_empty() {
                    tab.insert(start_char, &edit.new_text);
                }
            }
            tab.dirty = true;
            tab.recalc_view_bytes();
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
            grouped.entry(path.clone()).or_default().push(*line);
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

    /// Uninstalls an in-memory plugin from the plugin manager.
    pub fn uninstall_plugin(&mut self, plugin_name: &str) -> bool {
        if self.plugin_manager.uninstall(plugin_name) {
            self.doc.status = format!("Uninstalled plugin: {}", plugin_name);
            true
        } else {
            false
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
        let last_line = self.doc.line_count().saturating_sub(1);
        if line > last_line {
            // La línea vieja ya no existe: el contenido Ty se ajusta a
            // que el cursor aterrice al FIN de la última línea nueva.
            self.editor_state.cursor_line = last_line;
            self.editor_state.cursor_col =
                self.doc.line(last_line).chars().count();
        } else {
            self.editor_state.cursor_line = line;
            let line_len = self.doc.line(line).chars().count();
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
        for li in 0..self.doc.line_count() {
            let line = self.doc.line(li).into_owned();
            let hay = if self.find_case_sensitive {
                line
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
            let mut text = self.doc.line(line).into_owned();
            let sb = crate::editor_widget::char_to_byte_pub(&text, col);
            let eb = crate::editor_widget::char_to_byte_pub(&text, col + qlen);
            text.replace_range(sb..eb, &self.find_replace);
            self.doc.set_line(line, &text);
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
            let mut text = self.doc.line(line).into_owned();
            let sb = crate::editor_widget::char_to_byte_pub(&text, col);
            let eb = crate::editor_widget::char_to_byte_pub(&text, col + qlen);
            text.replace_range(sb..eb, &self.find_replace);
            self.doc.set_line(line, &text);
        }
        self.doc.dirty = true;
        self.doc.recalc_view_bytes();
        self.doc.snapshot_if_changed();
        self.find_update_matches();
        self.doc.status = format!("Replaced {} occurrences", count);
    }

    /// Roadmap v0.2.45 (Tarea B): abre la carpeta o el archivo pasado
    /// por línea de comandos en la pestaña inicial. Según lo que exista
    /// en disco:
    /// - directorio → pasa a ser la raíz del explorador;
    /// - archivo → su carpeta contenedora es la nueva raíz y el archivo se abre;
    /// - archivo nuevo → igual, dejando el buffer listo para guardar.
    pub fn init_from_cli_path(&mut self, path: &std::path::Path) {
        // La tilde (`~`) no la expande `Path`: la resolvemos contra el home
        // y, si el resultado sigue siendo relativo, contra la raíz actual.
        let resolved =
            crate::util::expand_user_path(&path.to_string_lossy(), &self.doc.browse_dir);

        if resolved.is_dir() {
            self.doc.browse_dir = resolved.clone();
            self.refresh_explorer();
            self.refresh_git_status();
            self.refresh_git_branch();
        } else if resolved.is_file() {
            if let Some(parent) = resolved.parent() {
                self.doc.browse_dir = parent.to_path_buf();
                self.refresh_explorer();
                self.refresh_git_status();
                self.refresh_git_branch();
            }
            self.open_path(resolved);
        } else {
            // Creación de archivo nuevo: su carpeta contenedora (si ya
            // existe) pasa a ser la raíz del explorador y se abre/prepara
            // la pestaña con esa ruta y buffer vacío, lista para guardar.
            if let Some(parent) = resolved.parent() {
                if parent.exists() {
                    self.doc.browse_dir = parent.to_path_buf();
                    self.refresh_explorer();
                    self.refresh_git_status();
                    self.refresh_git_branch();
                }
            }
            self.open_path(resolved);
        }
    }

    /// v0.2.48 (Tarea B): drena los comandos IPC pendientes del canal del
    /// servidor. Por cada `Open(path)` abre la pestaña/workspace con
    /// `init_from_cli_path` y pide foco + atención al gestor de ventanas.
    pub fn poll_ipc_events(&mut self, ctx: &egui::Context) {
        // Drenar primero en un buffer local: el préstamo inmutable del canal
        // del servidor no puede coexistir con la mutación de `self` abajo.
        let mut commands = Vec::new();
        if let Some(server) = &self.ipc_server {
            while let Ok(command) = server.receiver.try_recv() {
                commands.push(command);
            }
        }
        for command in commands {
            match command {
                karkinos_core::ipc::IpcCommand::Open(path) => {
                    self.init_from_cli_path(&path);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Critical,
                    ));
                    ctx.request_repaint();
                }
                karkinos_core::ipc::IpcCommand::Focus => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Critical,
                    ));
                    ctx.request_repaint();
                }
            }
        }
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
        let can_reuse_current = self.doc.current_file.is_none() && !self.doc.dirty && self.doc.text().is_empty();
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

    /// S10: fija el tamaño de fuente acotado al rango válido (8..32 px),
    /// sincronizando el EditorState primario, todas las pestañas y el panel secundario si existe.
    pub fn set_font_size(&mut self, size: f32) {
        let clamped = crate::editor_widget::clamp_font_size(size);
        self.font_size = clamped;
        self.editor_state.set_font_size(clamped);
        for tab in &mut self.doc.tabs {
            tab.editor_state.set_font_size(clamped);
        }
        if let Some(split) = self.split.as_mut() {
            split.secondary.set_font_size(clamped);
        }
        self.doc.status = format!("Zoom: {} px", clamped.round());
    }

    /// S10: restablece el tamaño de fuente al valor por defecto (14 px).
    pub fn reset_zoom(&mut self) {
        self.set_font_size(crate::editor_widget::DEFAULT_FONT_SIZE);
    }

    /// S10: cambia el tamaño de la fuente del editor un paso (zoom in
    /// / out con clamp 8..32) y lo informa en la barra de estado.
    pub fn zoom(&mut self, zoom_in: bool) {
        let next = apply_zoom(self.font_size, zoom_in);
        if (next - self.font_size).abs() < f32::EPSILON {
            return; // ya en el tope: no spamear el status
        }
        self.set_font_size(next);
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
