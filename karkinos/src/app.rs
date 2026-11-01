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

/// v0.2.41: un nombre válido para el diálogo del explorador es un único
/// componente normal de ruta (sin separadores, `.`, `..` ni rutas absolutas).
fn is_valid_explorer_item_name(name: &str) -> bool {
    let mut components = std::path::Path::new(name).components();
    matches!(
        components.next(),
        Some(std::path::Component::Normal(_))
    ) && components.next().is_none()
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
    // v0.2.44: watcher periódico de disco (pestañas + metadatos de Git).
    pub last_disk_poll: Option<std::time::Instant>,
    pub last_git_mtime: Option<std::time::SystemTime>,
    pub blame_worker: crate::git_status_worker::BlameWorker,
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
}

/// v0.2.41: tipo de ítem que crea el diálogo inline del explorador.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerNewItemKind {
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
            workspace_search: WorkspaceSearchState::default(),
            workspace_search_engine: karkinos_core::WorkspaceSearch::spawn(),
            sidebar_tab: SidebarTab::None,
            explorer: ExplorerState::default(),
            symbol_picker: SymbolPickerState::default(),
            command_palette: CommandPaletteState::default(),
            git_status: GitStatusState::default(),
            git_ui: GitUiState::default(),
            git_diff_modal: None,
            last_disk_poll: None,
            last_git_mtime: None,
            blame_worker: crate::git_status_worker::BlameWorker::spawn(),
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

    /// v0.2.41: crea el ítem pendiente del diálogo inline del explorador.
    /// El nombre debe ser un único componente de ruta, así nunca se escribe
    /// fuera de `target_dir`. Un archivo creado se abre de inmediato.
    pub fn create_explorer_item(&mut self) -> Result<std::path::PathBuf, String> {
        let Some(kind) = self.explorer.creating_item else {
            return Err("No item is being created".to_string());
        };

        let name = self.explorer.new_item_name.trim().to_string();
        if !is_valid_explorer_item_name(&name) {
            let msg = "Invalid name: use a single file or folder name".to_string();
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let target_dir = self
            .explorer
            .new_item_target_dir
            .clone()
            .unwrap_or_else(|| self.doc.browse_dir.clone());
        let path = target_dir.join(&name);
        if path.exists() {
            let msg = format!("Already exists: {}", path.display());
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let created = match kind {
            ExplorerNewItemKind::File => std::fs::File::create(&path).map(|_| ()),
            ExplorerNewItemKind::Folder => std::fs::create_dir(&path),
        };
        if let Err(e) = created {
            let msg = format!("Could not create {}: {}", name, e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        self.explorer.cancel_new_item();
        self.explorer.loaded_for = None;
        self.refresh_explorer();

        if kind == ExplorerNewItemKind::File {
            self.open_path(path.clone());
        }
        self.doc.status = format!("Created {}", name);
        Ok(path)
    }

    /// v0.2.41: elimina un archivo del disco y cierra su pestaña si estaba
    /// abierto. Las carpetas se rechazan: el explorador solo ofrece el botón
    /// de borrado en filas de archivo.
    pub fn delete_explorer_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        if path.is_dir() {
            let msg = format!("Not a file: {}", path.display());
            self.doc.status = msg.clone();
            return Err(msg);
        }

        if let Err(e) = std::fs::remove_file(path) {
            let msg = format!("Could not delete {}: {}", path.display(), e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        self.sync_active_to_tab();
        if let Some(idx) = self
            .doc
            .tabs
            .iter()
            .position(|t| t.current_file.as_deref() == Some(path))
        {
            self.close_tab(idx);
        }

        self.explorer.item_to_delete = None;
        self.explorer.loaded_for = None;
        self.refresh_explorer();
        self.doc.status = format!("Deleted {}", name);
        Ok(())
    }

    /// F8 (Roadmap v0.2.35): lee la rama del repo una sola vez (desde
    /// `.git/HEAD`, sin subprocesos).
    pub fn refresh_git_branch(&mut self) {
        if self.git_status.branch.is_none() {
            self.git_status.branch = karkinos_core::read_branch(&self.doc.browse_dir);
        }
    }

    /// v0.2.42: raíz del repo que contiene el workspace actual.
    fn git_repo_root(&self) -> std::path::PathBuf {
        crate::git_diff_worker::repo_root_for(&self.doc.browse_dir)
            .unwrap_or_else(|| self.doc.browse_dir.clone())
    }

    /// v0.2.42: recarga los cambios pendientes de `git status --porcelain`.
    pub fn refresh_git_status(&mut self) {
        let root = self.git_repo_root();
        self.git_ui.is_loading = true;
        self.git_ui.last_error = None;

        match karkinos_core::git_status::query_git_status(&root) {
            Ok(entries) => {
                self.git_ui.entries = entries;
            }
            Err(error) => {
                self.git_ui.entries.clear();
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git status failed: {error}");
            }
        }

        self.git_ui.is_loading = false;
        self.git_status.branch = karkinos_core::read_branch(&root);
    }

    /// v0.2.44: abre el visor de diff de `path` (working tree o staged).
    /// La ruta relativa se calcula contra `doc.browse_dir`, que es la raíz
    /// contra la que también consulta el Source Control.
    pub fn open_git_diff(&mut self, path: &std::path::Path, staged: bool) {
        let root = self.doc.browse_dir.clone();
        let rel_path = path
            .strip_prefix(&root)
            .map(|relative| relative.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf());
        let diff_text =
            match karkinos_core::git_status::query_git_diff(&root, Some(&rel_path), staged) {
                Ok(diff) => diff,
                Err(error) => {
                    self.doc.status = format!("Git diff failed: {error}");
                    return;
                }
            };

        let rel_name = rel_path.to_string_lossy().replace('\\', "/");
        let parsed_lines = karkinos_core::git_status::parse_unified_diff(&diff_text);
        self.git_diff_modal = Some(GitDiffViewState {
            file_path: path.to_path_buf(),
            rel_name,
            diff_text,
            parsed_lines,
            is_staged: staged,
        });
    }

    /// v0.2.44: cierra el visor de diff.
    pub fn close_git_diff(&mut self) {
        self.git_diff_modal = None;
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
            tab.lines = if content.is_empty() {
                vec![String::new()]
            } else {
                content.split('\n').map(|line| line.to_string()).collect()
            };
            tab.buffer = karkinos_core::TextBuffer::from_str(&content);
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

    /// v0.2.42: crea un commit con el mensaje de la caja de Source Control.
    pub fn git_commit(&mut self) -> Result<String, String> {
        let root = self.git_repo_root();
        let message = self.git_ui.commit_message.clone();

        match karkinos_core::git_status::execute_git_commit(&root, &message) {
            Ok(output) => {
                self.git_ui.commit_message.clear();
                self.git_ui.last_error = None;
                self.refresh_git_status();
                self.doc.status = "Commit created".to_string();
                Ok(output)
            }
            Err(error) => {
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git commit failed: {error}");
                Err(error)
            }
        }
    }

    /// v0.2.42: agrega todos los cambios al índice (`git add -A`).
    pub fn git_stage_all(&mut self) -> Result<(), String> {
        let root = self.git_repo_root();

        match karkinos_core::git_status::execute_git_stage_all(&root) {
            Ok(()) => {
                self.git_ui.last_error = None;
                self.refresh_git_status();
                self.doc.status = "Staged all changes".to_string();
                Ok(())
            }
            Err(error) => {
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git stage failed: {error}");
                Err(error)
            }
        }
    }

    /// v0.2.42: agrega un único archivo al índice.
    pub fn git_stage_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        let root = self.git_repo_root();

        match karkinos_core::git_status::execute_git_stage_path(&root, path) {
            Ok(()) => {
                self.git_ui.last_error = None;
                self.refresh_git_status();
                Ok(())
            }
            Err(error) => {
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git stage failed: {error}");
                Err(error)
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
        let line = symbol.line.min(self.doc.lines.len().saturating_sub(1));
        self.editor_state.cursor_line = line;
        self.editor_state.cursor_col = symbol.character;
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
            "sidebar:explorer" => self.toggle_explorer(),
            "sidebar:search" => self.open_workspace_search(),
            "search:replace_in_workspace" => self.open_workspace_replace(),
            "search:toggle_replace" => {
                self.workspace_search.show_replace = !self.workspace_search.show_replace;
            }
            "sidebar:symbols" => self.open_symbol_picker(),
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
            "editor:format" => self.format_document(),
            "file:save" => {
                let _ = self.doc.try_save();
            }
            "file:new" => self.try_new_document(),
            _ => {}
        }
    }

    /// v0.2.40: formatea el buffer activo con `rustfmt` (stdin → stdout).
    /// Solo aplica a Rust; el resto reporta el motivo en la barra de estado.
    pub fn format_document(&mut self) {
        use std::io::Write;

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
            tab.lines = if content.is_empty() {
                vec![String::new()]
            } else {
                content.split('\n').map(|line| line.to_string()).collect()
            };
            tab.buffer = karkinos_core::TextBuffer::from_str(&content);
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
            .min(self.doc.lines.len().saturating_sub(1));
        self.editor_state.cursor_line = line;
        self.editor_state.cursor_col = m.col_start;
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

    // F6 (Roadmap v0.2.32): búsqueda global.

    #[test]
    fn trigger_workspace_search_con_regex_invalida_reporta_error() {
        let mut app = EditorApp::default();
        app.workspace_search.input = "([a-z".to_string();
        app.workspace_search.use_regex = true;
        app.trigger_workspace_search();

        assert!(app.workspace_search.error.is_some(), "la regex inválida se informa");
        assert!(!app.workspace_search.searching, "no se lanza un recorrido inútil");
        assert!(app.workspace_search.results.is_empty());
    }

    #[test]
    fn trigger_workspace_search_vacia_no_busca() {
        let mut app = EditorApp::default();
        app.workspace_search.input = "   ".to_string();
        app.trigger_workspace_search();
        assert!(!app.workspace_search.searching);
        assert!(app.workspace_search.error.is_none());
    }

    // v0.2.43: reemplazo global en el workspace.

    fn replace_test_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("karkinos_replace_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn execute_workspace_replace_escribe_disco_y_sincroniza_pestanas() {
        let dir = replace_test_dir("sync");
        let a = dir.join("a.rs");
        let b = dir.join("b.txt");
        std::fs::write(&a, "let foo = foo;\n").expect("a");
        std::fs::write(&b, "foo bar\n").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.open_path(a.clone());
        app.workspace_search.input = "foo".to_string();
        app.workspace_search.replace_input = "baz".to_string();

        let summary = app.execute_workspace_replace().expect("replace");

        assert_eq!(summary.files_modified, 2);
        assert_eq!(summary.replacements_count, 3);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "let baz = baz;\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "baz bar\n");

        // El buffer activo y la pestaña abierta reflejan el disco, sin dirty.
        assert_eq!(app.doc.current_text(), "let baz = baz;\n");
        assert_eq!(app.doc.buffer.to_string(), "let baz = baz;\n");
        assert!(!app.doc.dirty);
        let tab = &app.doc.tabs[app.doc.active_tab];
        assert_eq!(tab.buffer.to_string(), "let baz = baz;\n");
        assert!(!tab.dirty);
        assert_eq!(
            tab.last_disk_mtime,
            std::fs::metadata(&a).ok().and_then(|m| m.modified().ok())
        );

        let feedback = app
            .workspace_search
            .replace_summary
            .as_deref()
            .unwrap_or_default();
        assert!(
            feedback.contains("3 occurrences across 2 files"),
            "summary: {feedback}"
        );
        assert!(app.doc.status.contains("3 occurrences across 2 files"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn execute_workspace_replace_respeta_el_filtro_de_archivos() {
        let dir = replace_test_dir("filter");
        let rust = dir.join("only.rs");
        let text = dir.join("skip.txt");
        std::fs::write(&rust, "foo\n").expect("rust");
        std::fs::write(&text, "foo\n").expect("text");

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.workspace_search.input = "foo".to_string();
        app.workspace_search.replace_input = "bar".to_string();
        app.workspace_search.file_filter = " *.rs ".to_string();

        let summary = app.execute_workspace_replace().expect("replace");

        assert_eq!(summary.files_modified, 1);
        assert_eq!(summary.replacements_count, 1);
        assert_eq!(std::fs::read_to_string(&rust).unwrap(), "bar\n");
        assert_eq!(std::fs::read_to_string(&text).unwrap(), "foo\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn execute_workspace_replace_preserva_ediciones_sin_guardar_en_pestanas_intactas() {
        let dir = replace_test_dir("dirty_tabs");
        let a = dir.join("a.rs");
        let b = dir.join("b.rs");
        std::fs::write(&a, "alpha\n").expect("a");
        std::fs::write(&b, "foo\n").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.open_path(a.clone());
        app.set_text("alpha edited\n");
        app.doc.dirty = true;
        app.open_path(b.clone());
        app.workspace_search.input = "foo".to_string();
        app.workspace_search.replace_input = "bar".to_string();

        app.execute_workspace_replace().expect("replace");

        // a.rs no tenía coincidencias: su pestaña conserva edits y dirty.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha\n");
        let tab_a = app
            .doc
            .tabs
            .iter()
            .find(|tab| tab.current_file.as_deref() == Some(a.as_path()))
            .expect("tab a");
        assert_eq!(tab_a.buffer.to_string(), "alpha edited\n");
        assert!(tab_a.dirty, "una pestaña intacta no pierde ediciones");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn palette_comandos_de_reemplazo_abren_y_alternan_el_campo() {
        let mut app = EditorApp::default();

        let replace = PALETTE_COMMANDS
            .iter()
            .find(|command| command.id == "search:replace_in_workspace")
            .expect("comando replace_in_workspace");
        assert_eq!(replace.title, "Search: Replace in Files (Ctrl+Shift+H)");
        assert_eq!(replace.category, "Search");
        assert_eq!(replace.shortcut, Some("Ctrl+Shift+H"));
        assert!(
            PALETTE_COMMANDS.iter().any(|command| {
                command.id == "search:toggle_replace" && command.category == "Search"
            })
        );

        app.execute_palette_command("search:replace_in_workspace");
        assert_eq!(app.sidebar_tab, SidebarTab::Search);
        assert!(app.workspace_search.open);
        assert!(app.workspace_search.show_replace);
        assert!(app.workspace_search.replace_focus_requested);

        app.execute_palette_command("search:toggle_replace");
        assert!(!app.workspace_search.show_replace);
        app.execute_palette_command("search:toggle_replace");
        assert!(app.workspace_search.show_replace);
    }

    #[test]
    fn jump_to_search_match_posiciona_cursor_y_seleccion() {        let path = std::env::temp_dir().join(format!(
            "karkinos_jump_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn uno() {}\nfn dos() {}\nfn tres() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.open_path(path.clone());
        app.jump_to_search_match(
            &path,
            &karkinos_core::SearchMatch {
                line_number: 2,
                col_start: 3,
                col_end: 6,
                line_content: "fn dos() {}".to_string(),
            },
        );

        assert_eq!(app.editor_state.cursor_line, 1, "línea 2 es índice 1");
        assert_eq!(app.editor_state.cursor_col, 3);
        assert_eq!(app.editor_state.sel_anchor, Some((1, 6)));

        std::fs::remove_file(path).ok();
    }

    // Barra lateral (v0.2.33): explorador.

    #[test]
    fn explorer_toggle_y_refresh_cargan_archivos_del_proyecto() {
        let root = std::env::temp_dir().join(format!(
            "karkinos_explorer_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(root.join("src")).expect("crear src");
        std::fs::create_dir_all(root.join("target")).expect("crear target");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("archivo");
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        assert!(!app.explorer.open);

        app.toggle_explorer();
        assert!(app.explorer.open, "el explorador queda abierto");
        let names: Vec<String> = app
            .explorer
            .files
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(names.contains(&"src/main.rs".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("target/")), "{names:?}");

        app.toggle_explorer();
        assert!(!app.explorer.open, "el segundo toggle lo cierra");

        std::fs::remove_dir_all(&root).ok();
    }

    // v0.2.41: operaciones de archivo desde el explorador.

    fn explorer_temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "karkinos_explorer_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("crear raíz temporal");
        root
    }

    #[test]
    fn explorer_crear_archivo_lo_escribe_en_disco_y_lo_abre() {
        let root = explorer_temp_root("create_file");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.explorer.open = true;

        app.explorer.begin_new_item(ExplorerNewItemKind::File, None);
        app.explorer.new_item_name = "nuevo.rs".to_string();

        let path = app.create_explorer_item().expect("crear archivo");

        assert_eq!(path, root.join("nuevo.rs"));
        assert!(path.is_file(), "el archivo existe en disco");
        assert_eq!(
            app.doc.current_file,
            Some(path.clone()),
            "el archivo se abre de inmediato"
        );
        assert!(
            app.explorer.files.contains(&path),
            "el explorador se refresca con el archivo nuevo"
        );
        assert_eq!(app.explorer.creating_item, None);
        assert!(app.explorer.new_item_name.is_empty());
        assert_eq!(app.doc.status, "Created nuevo.rs");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_crear_carpeta_en_disco_y_rechaza_nombres_invalidos() {
        let root = explorer_temp_root("create_folder");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.explorer
            .begin_new_item(ExplorerNewItemKind::Folder, Some(root.clone()));
        app.explorer.new_item_name = "nueva_carpeta".to_string();

        let path = app.create_explorer_item().expect("crear carpeta");

        assert_eq!(path, root.join("nueva_carpeta"));
        assert!(path.is_dir());
        assert_eq!(
            app.doc.current_file, None,
            "una carpeta no se abre en el editor"
        );
        assert_eq!(app.explorer.creating_item, None);

        app.explorer.begin_new_item(ExplorerNewItemKind::File, None);
        app.explorer.new_item_name = "   ".to_string();
        assert!(app.create_explorer_item().is_err(), "nombre vacío");
        assert!(
            app.explorer.creating_item.is_some(),
            "el diálogo sigue abierto para corregir"
        );

        app.explorer.new_item_name = "../escapa".to_string();
        assert!(app.create_explorer_item().is_err(), "no sale de la raíz");
        assert!(!root.parent().unwrap().join("escapa").exists());

        app.explorer.new_item_name = "nuevo.rs".to_string();
        std::fs::write(root.join("nuevo.rs"), "ya existe").expect("archivo existente");
        assert!(
            app.create_explorer_item().is_err(),
            "no pisa archivos existentes"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_borrar_archivo_cierra_su_pestana_y_refresca() {
        let root = explorer_temp_root("delete_file");
        let a = root.join("a.rs");
        let b = root.join("b.rs");
        std::fs::write(&a, "fn a() {}").expect("a");
        std::fs::write(&b, "fn b() {}").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();
        app.open_path(a.clone());
        app.open_path(b.clone());
        assert_eq!(app.doc.current_file, Some(b.clone()));
        assert_eq!(app.doc.tabs.len(), 2);

        app.delete_explorer_file(&a).expect("borrar a.rs");

        assert!(!a.exists(), "a.rs desaparece del disco");
        assert!(b.exists(), "b.rs queda intacto");
        assert_eq!(
            app.doc.current_file,
            Some(b.clone()),
            "la pestaña activa no cambia"
        );
        assert!(app
            .doc
            .tabs
            .iter()
            .all(|t| t.current_file.as_deref() != Some(a.as_path())));
        assert!(!app.explorer.files.contains(&a));
        assert_eq!(app.doc.status, "Deleted a.rs");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_borrar_archivo_activo_resetea_la_pestana() {
        let root = explorer_temp_root("delete_active");
        let a = root.join("unico.rs");
        std::fs::write(&a, "fn unico() {}").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(a.clone());
        assert_eq!(app.doc.tabs.len(), 1);

        app.delete_explorer_file(&a).expect("borrar archivo activo");

        assert!(!a.exists());
        assert_eq!(app.doc.current_file, None);
        assert_eq!(app.doc.tabs.len(), 1, "la única pestaña se resetea en blanco");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_borrar_archivo_inexistente_y_carpeta_devuelven_error() {
        let root = explorer_temp_root("delete_missing");
        let missing = root.join("no_existe.rs");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        assert!(app.delete_explorer_file(&missing).is_err());
        assert!(
            app.delete_explorer_file(&root).is_err(),
            "rechaza carpetas, solo borra archivos"
        );

        std::fs::remove_dir_all(&root).ok();
    }


    fn symbol(name: &str, kind: karkinos_core::syntax_ast::SymbolKind, line: usize) -> karkinos_core::syntax_ast::DocumentSymbol {
        karkinos_core::syntax_ast::DocumentSymbol {
            name: name.to_string(),
            kind,
            line,
            character: 3,
        }
    }

    #[test]
    fn symbol_picker_filtra_por_nombre_sin_mayusculas() {
        let mut app = EditorApp::default();
        app.editor_state.symbols = vec![
            symbol("main", karkinos_core::syntax_ast::SymbolKind::Function, 0),
            symbol("process", karkinos_core::syntax_ast::SymbolKind::Method, 5),
        ];
        app.open_symbol_picker();
        assert!(app.symbol_picker.open);
        assert_eq!(app.filtered_symbols().len(), 2, "sin filtro muestra todo");

        app.symbol_picker.query = "PROC".to_string();
        let filtered = app.filtered_symbols();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "process");
    }

    #[test]
    fn jump_to_symbol_posiciona_cursor_y_cierra_selector() {
        let path = std::env::temp_dir().join(format!(
            "karkinos_symbol_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn uno() {}\nfn dos() {}\nfn tres() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.open_path(path.clone());
        app.open_symbol_picker();
        app.jump_to_symbol(&symbol("dos", karkinos_core::syntax_ast::SymbolKind::Function, 1));

        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 3);
        assert!(!app.symbol_picker.open, "el salto cierra el selector");

        std::fs::remove_file(path).ok();
    }

    // F8 (Roadmap v0.2.35): blame inline.

    #[test]
    fn inline_blame_se_limpia_cuando_el_cursor_se_mueve() {
        let path = std::env::temp_dir().join(format!(
            "karkinos_blame_move_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn main() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        app.editor_state.blame_text = Some("Ada, 2 days ago".to_string());
        app.git_status.last_cursor = (0, 0);
        app.editor_state.cursor_line = 3;

        app.update_inline_blame();
        assert!(app.editor_state.blame_text.is_none(), "al moverse se limpia");
        assert!(app.git_status.cursor_settled_at.is_some());

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn inline_blame_aparece_tras_el_dwell_con_cache() {
        let path = std::env::temp_dir().join(format!(
            "karkinos_blame_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn main() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        app.editor_state.cursor_line = 0;
        app.editor_state.cursor_col = 0;
        app.git_status.last_cursor = (0, 0);
        app.git_status.cursor_settled_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(400));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut lines = std::collections::HashMap::new();
        lines.insert(
            1usize,
            karkinos_core::git_status::BlameLine {
                author: "Ada".to_string(),
                timestamp: now - 86_400,
                summary: "feat: x".to_string(),
            },
        );
        app.git_status
            .blame_cache
            .insert(path.clone(), std::sync::Arc::new(lines));

        app.update_inline_blame();
        let text = app
            .editor_state
            .blame_text
            .clone()
            .expect("con la cache y el dwell cumplido hay ghost text");
        assert!(text.starts_with("Ada, "), "{text}");
        assert!(text.contains("feat: x"), "{text}");

        std::fs::remove_file(path).ok();
    }

    // UI-6 (v0.2.38): árbol del explorador.

    #[test]
    fn file_tree_groups_directories_first_and_sorted() {
        let root = std::path::Path::new("/proj");
        let paths = vec![
            std::path::PathBuf::from("/proj/src/main.rs"),
            std::path::PathBuf::from("/proj/src/app.rs"),
            std::path::PathBuf::from("/proj/Cargo.toml"),
            std::path::PathBuf::from("/proj/crates/core/lib.rs"),
        ];
        let tree = FileTreeNode::build_from_paths(root, &paths);
        assert_eq!(tree.len(), 3);
        assert!(matches!(&tree[0], FileTreeNode::Directory { name, .. } if name == "crates"));
        assert!(matches!(&tree[1], FileTreeNode::Directory { name, .. } if name == "src"));
        assert!(matches!(&tree[2], FileTreeNode::File { name, .. } if name == "Cargo.toml"));

        match &tree[1] {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 2);
                assert!(
                    matches!(&children[0], FileTreeNode::File { name, .. } if name == "app.rs")
                );
                assert!(
                    matches!(&children[1], FileTreeNode::File { name, .. } if name == "main.rs")
                );
            }
            _ => panic!("src tiene que ser una carpeta"),
        }
        if let FileTreeNode::File { extension, .. } = &tree[2] {
            assert_eq!(extension, "toml");
        }
    }

    #[test]
    fn file_tree_collects_all_directories_for_collapse_all() {
        let root = std::path::Path::new("/p");
        let paths = vec![
            std::path::PathBuf::from("/p/a/b/c.rs"),
            std::path::PathBuf::from("/p/d/e.rs"),
        ];
        let tree = FileTreeNode::build_from_paths(root, &paths);
        let mut dirs = Vec::new();
        FileTreeNode::collect_directories(&tree, &mut dirs);
        assert!(dirs.contains(&std::path::PathBuf::from("/p/a")));
        assert!(dirs.contains(&std::path::PathBuf::from("/p/a/b")));
        assert!(dirs.contains(&std::path::PathBuf::from("/p/d")));
        assert_eq!(dirs.len(), 3);
    }

    #[test]
    fn file_tree_includes_empty_directories() {
        let root = std::path::Path::new("/proj");
        let files = vec![std::path::PathBuf::from("/proj/src/main.rs")];
        let dirs = vec![
            std::path::PathBuf::from("/proj/src"),
            std::path::PathBuf::from("/proj/assets/images"),
            std::path::PathBuf::from("/proj/empty"),
        ];

        let tree = FileTreeNode::build_from_paths_and_dirs(root, &files, &dirs);
        let names: Vec<&str> = tree.iter().map(|n| n.name()).collect();
        assert_eq!(names, vec!["assets", "empty", "src"], "{names:?}");

        let empty = tree
            .iter()
            .find(|n| n.name() == "empty")
            .expect("la carpeta vacía aparece en el árbol");
        match empty {
            FileTreeNode::Directory { children, path, .. } => {
                assert!(children.is_empty(), "sin hijos");
                assert_eq!(path, &std::path::PathBuf::from("/proj/empty"));
            }
            _ => panic!("empty tiene que ser una carpeta"),
        }

        let assets = tree
            .iter()
            .find(|n| n.name() == "assets")
            .expect("assets aparece");
        match assets {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].name(), "images");
                match &children[0] {
                    FileTreeNode::Directory { children, .. } => {
                        assert!(children.is_empty(), "images queda vacía")
                    }
                    _ => panic!("images tiene que ser una carpeta"),
                }
            }
            _ => panic!("assets tiene que ser una carpeta"),
        }

        let src = tree.iter().find(|n| n.name() == "src").expect("src aparece");
        match src {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 1);
                assert!(
                    matches!(&children[0], FileTreeNode::File { name, .. } if name == "main.rs")
                );
            }
            _ => panic!("src tiene que ser una carpeta"),
        }
    }

    #[test]
    fn refresh_explorer_incluye_carpetas_vacias_en_el_arbol() {
        let root = explorer_temp_root("empty_folders");
        std::fs::create_dir_all(root.join("vacia/nested")).expect("crear vacía");
        std::fs::create_dir_all(root.join("src")).expect("crear src");
        std::fs::create_dir_all(root.join("target")).expect("crear target");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("archivo");
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();

        let rel_dirs: Vec<String> = app
            .explorer
            .directories
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(rel_dirs.contains(&"vacia".to_string()), "{rel_dirs:?}");
        assert!(
            rel_dirs.contains(&"vacia/nested".to_string()),
            "{rel_dirs:?}"
        );
        assert!(
            !rel_dirs.iter().any(|d| d.starts_with("target")),
            "{rel_dirs:?}"
        );

        let tree = FileTreeNode::build_from_paths_and_dirs(
            &root,
            &app.explorer.files,
            &app.explorer.directories,
        );
        let vacia = tree
            .iter()
            .find(|n| n.name() == "vacia")
            .expect("la carpeta vacía aparece en el árbol");
        match vacia {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 1, "nested conserva la jerarquía");
                assert_eq!(children[0].name(), "nested");
                match &children[0] {
                    FileTreeNode::Directory { children, .. } => {
                        assert!(children.is_empty(), "la carpeta hoja no tiene hijos")
                    }
                    _ => panic!("nested tiene que ser una carpeta"),
                }
            }
            _ => panic!("vacia tiene que ser una carpeta"),
        }

        std::fs::remove_dir_all(&root).ok();
    }

    // UI-7 (v0.2.38): minimap toggleable.

    #[test]
    fn toggle_minimap_flips_primary_and_split() {
        let mut app = EditorApp::default();
        assert!(!app.editor_state.show_minimap, "arranca apagado");

        app.toggle_split(SplitDirection::Vertical);
        assert!(!app.split.as_ref().unwrap().secondary.show_minimap);

        app.toggle_minimap();
        assert!(app.editor_state.show_minimap);
        assert!(
            app.split.as_ref().unwrap().secondary.show_minimap,
            "el panel secundario acompaña"
        );

        app.toggle_minimap();
        assert!(!app.editor_state.show_minimap);
        assert!(!app.split.as_ref().unwrap().secondary.show_minimap);
    }

    // Barra lateral unificada (v0.2.39): pestañas compartidas y alternancia limpia.

    #[test]
    fn sidebar_tab_switching_unifies_panel() {
        let mut app = EditorApp::default();
        assert_eq!(app.sidebar_tab, SidebarTab::None);
        assert!(!app.explorer.open);
        assert!(!app.workspace_search.open);

        // Abrir Explorer
        app.toggle_explorer();
        assert_eq!(app.sidebar_tab, SidebarTab::Explorer);
        assert!(app.explorer.open);
        assert!(!app.workspace_search.open);

        // Cambiar a Search (se abre en la misma ventana, cierra Explorer)
        app.open_workspace_search();
        assert_eq!(app.sidebar_tab, SidebarTab::Search);
        assert!(app.workspace_search.open);
        assert!(!app.explorer.open);

        // Cambiar a Symbols
        app.open_symbol_picker();
        assert_eq!(app.sidebar_tab, SidebarTab::Symbols);
        assert!(!app.workspace_search.open);

        // Volver a clickear Symbols lo cierra
        app.open_symbol_picker();
        assert_eq!(app.sidebar_tab, SidebarTab::None);

        // v0.2.42: Source Control entra en la misma alternancia.
        app.open_source_control();
        assert_eq!(app.sidebar_tab, SidebarTab::SourceControl);
        assert!(!app.explorer.open);
        assert!(!app.workspace_search.open);
        assert!(!app.symbol_picker.open);

        app.open_source_control();
        assert_eq!(app.sidebar_tab, SidebarTab::None);
    }

    // v0.2.40: Command Palette (Ctrl+Shift+P / F1).

    #[test]
    fn palette_catalogo_tiene_ids_unicos_y_los_comandos_esperados() {
        let mut ids: Vec<&str> = PALETTE_COMMANDS.iter().map(|command| command.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "cada id del catálogo es único");
        assert_eq!(total, 30, "el catálogo de v0.2.44 tiene 30 comandos");

        for expected in [
            "layout:split_vertical",
            "layout:split_horizontal",
            "layout:close_split",
            "editor:toggle_minimap",
            "sidebar:explorer",
            "sidebar:search",
            "search:replace_in_workspace",
            "search:toggle_replace",
            "sidebar:symbols",
            "terminal:toggle",
            "cargo:check",
            "cargo:test",
            "cargo:clippy",
            "cargo:run",
            "debug:toggle_breakpoint",
            "debug:start",
            "debug:step_over",
            "debug:step_into",
            "debug:stop",
            "theme:dark",
            "theme:monokai",
            "theme:dracula",
            "editor:format",
            "file:save",
            "file:new",
            "git:source_control",
            "git:refresh",
            "git:stage_all",
            "git:view_diff",
            "git:close_diff",
        ] {
            assert!(
                PALETTE_COMMANDS.iter().any(|command| command.id == expected),
                "falta el comando {expected}"
            );
        }
    }

    #[test]
    fn palette_filtra_por_categoria_y_titulo_sin_mayusculas() {
        let mut app = EditorApp::default();
        assert_eq!(app.filtered_palette_commands().len(), PALETTE_COMMANDS.len());

        app.command_palette.query = "THEME".to_string();
        let themes: Vec<&str> = app
            .filtered_palette_commands()
            .iter()
            .map(|command| command.id)
            .collect();
        assert_eq!(themes, ["theme:dark", "theme:monokai", "theme:dracula"]);

        app.command_palette.query = "minimap".to_string();
        let minimap: Vec<&str> = app
            .filtered_palette_commands()
            .iter()
            .map(|command| command.id)
            .collect();
        assert_eq!(minimap, ["editor:toggle_minimap"]);

        app.command_palette.query = "  file:  ".to_string();
        let files: Vec<&str> = app
            .filtered_palette_commands()
            .iter()
            .map(|command| command.id)
            .collect();
        assert_eq!(files, ["file:save", "file:new"], "el trim también aplica");

        app.command_palette.query = "sin-coincidencias".to_string();
        assert!(app.filtered_palette_commands().is_empty());
    }

    #[test]
    fn palette_abre_limpia_query_y_no_borra_lo_escrito_si_ya_esta_abierto() {
        let mut app = EditorApp::default();
        assert!(!app.command_palette.open);

        app.open_command_palette();
        assert!(app.command_palette.open);
        assert!(app.command_palette.query.is_empty());
        assert_eq!(app.command_palette.selected, 0);

        app.command_palette.query = "cargo".to_string();
        app.command_palette.selected = 2;
        app.open_command_palette();
        assert_eq!(app.command_palette.query, "cargo", "F1 de nuevo no borra el query");
        assert_eq!(app.command_palette.selected, 2);
    }

    #[test]
    fn palette_seleccion_clampa_y_enter_ejecuta_el_seleccionado() {
        let mut app = EditorApp::default();
        app.open_command_palette();
        app.command_palette.query = "theme".to_string();

        app.move_palette_selection(-1);
        assert_eq!(app.command_palette.selected, 0, "no baja de 0");

        app.move_palette_selection(1);
        app.move_palette_selection(100);
        assert_eq!(app.command_palette.selected, 2, "no pasa del último");

        app.execute_selected_palette_command();
        assert_eq!(
            app.theme_mode,
            crate::theme::ThemeMode::Dracula,
            "Enter ejecuta el comando seleccionado"
        );
        assert!(!app.command_palette.open, "ejecutar cierra el modal");
        assert!(app.command_palette.query.is_empty());
        assert_eq!(app.command_palette.selected, 0);
    }

    #[test]
    fn palette_ejecuta_layout_y_cierra_o_ignora_ids_desconocidos() {
        let mut app = EditorApp::default();
        app.open_command_palette();

        app.execute_palette_command("layout:split_vertical");
        assert_eq!(
            app.split.as_ref().expect("split abierto").direction,
            SplitDirection::Vertical
        );
        assert!(!app.command_palette.open);

        app.open_command_palette();
        app.execute_palette_command("layout:close_split");
        assert!(app.split.is_none());

        app.open_command_palette();
        app.execute_palette_command("no:existe");
        assert!(!app.command_palette.open, "un id desconocido no deja la UI colgada");
        assert!(app.split.is_none());
    }

    #[test]
    fn format_document_rechaza_lenguajes_no_rust_sin_tocar_el_buffer() {
        let mut app = EditorApp::default();
        app.doc.file_path_input = "notas.txt".to_string();
        app.set_text("texto plano\n");

        app.format_document();

        assert_eq!(app.doc.current_text(), "texto plano\n");
        assert!(
            app.doc.status.starts_with("Format: rustfmt"),
            "status: {}",
            app.doc.status
        );
        assert!(!app.doc.dirty);
    }

    #[test]
    fn format_aplica_texto_como_edicion_undoable_y_preserva_cursor() {
        let mut app = EditorApp::default();
        app.set_text("fn  main( ){}\n");
        app.editor_state.cursor_line = 0;
        app.editor_state.cursor_col = 8;

        app.apply_formatted_text("fn main() {}\n");

        assert_eq!(app.doc.current_text(), "fn main() {}\n");
        assert!(app.doc.dirty, "el formateo marca el buffer como sucio");
        assert_eq!(app.editor_state.cursor_line, 0);
        assert_eq!(app.editor_state.cursor_col, 8, "el cursor no salta al inicio");

        app.undo();
        assert_eq!(
            app.doc.current_text(),
            "fn  main( ){}\n",
            "el formateo se puede deshacer"
        );
    }

    // v0.2.42: Source Control (git status, stage y commit).

    fn git_temp_repo(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "karkinos_git_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("crear repo temporal");

        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .expect("ejecutar git");
            assert!(
                output.status.success(),
                "git {args:?} falló: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "karkinos@example.com"]);
        git(&["config", "user.name", "Karkinos Test"]);
        root
    }

    #[test]
    fn git_ui_refresh_stage_all_and_commit_roundtrip() {
        use karkinos_core::git_status::GitStatusKind;

        let root = git_temp_repo("roundtrip");
        std::fs::write(root.join("nuevo.txt"), "hola\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.refresh_git_status();
        assert!(app.git_ui.last_error.is_none(), "{:?}", app.git_ui.last_error);
        assert!(app.git_status.branch.is_some(), "la rama se lee del repo");
        assert_eq!(app.git_ui.entries.len(), 1);
        assert_eq!(app.git_ui.entries[0].rel_path, "nuevo.txt");
        assert_eq!(app.git_ui.entries[0].kind, GitStatusKind::Untracked);
        assert!(!app.git_ui.entries[0].staged);

        app.git_stage_all().expect("stage all");
        assert_eq!(app.git_ui.entries.len(), 1);
        assert_eq!(app.git_ui.entries[0].kind, GitStatusKind::Added);
        assert!(app.git_ui.entries[0].staged);

        app.git_ui.commit_message = "add nuevo.txt".to_string();
        let output = app.git_commit().expect("commit");
        assert!(output.contains("add nuevo.txt"), "{output}");
        assert!(app.git_ui.commit_message.is_empty(), "el mensaje se limpia");
        assert!(app.git_ui.entries.is_empty(), "no quedan cambios pendientes");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_ui_commit_rejects_blank_message_and_keeps_it() {
        let root = git_temp_repo("blank_message");
        std::fs::write(root.join("a.txt"), "a\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.git_ui.commit_message = "   ".to_string();

        let error = app.git_commit().expect_err("mensaje vacío");
        assert_eq!(error, "Commit message cannot be empty");
        assert_eq!(app.git_ui.commit_message, "   ", "no se pierde lo escrito");
        assert!(app.git_ui.last_error.is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_ui_stage_single_file_leaves_the_rest_untracked() {
        use karkinos_core::git_status::GitStatusKind;

        let root = git_temp_repo("stage_one");
        std::fs::write(root.join("uno.txt"), "uno\n").expect("archivo");
        std::fs::write(root.join("dos.txt"), "dos\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_git_status();
        assert_eq!(app.git_ui.entries.len(), 2);

        let path = app
            .git_ui
            .entries
            .iter()
            .find(|entry| entry.rel_path == "uno.txt")
            .expect("uno.txt en la lista")
            .path
            .clone();
        app.git_stage_path(&path).expect("stage de un archivo");

        assert_eq!(app.git_ui.entries.len(), 2);
        let uno = app
            .git_ui
            .entries
            .iter()
            .find(|entry| entry.rel_path == "uno.txt")
            .expect("uno.txt sigue listado");
        assert_eq!(uno.kind, GitStatusKind::Added);
        assert!(uno.staged);
        let dos = app
            .git_ui
            .entries
            .iter()
            .find(|entry| entry.rel_path == "dos.txt")
            .expect("dos.txt sigue listado");
        assert_eq!(dos.kind, GitStatusKind::Untracked);
        assert!(!dos.staged);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn palette_git_commands_open_refresh_and_stage() {
        let root = git_temp_repo("palette");
        std::fs::write(root.join("palette.txt"), "contenido\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.execute_palette_command("git:source_control");
        assert_eq!(app.sidebar_tab, SidebarTab::SourceControl);
        assert_eq!(app.git_ui.entries.len(), 1);

        app.execute_palette_command("git:stage_all");
        assert!(app.git_ui.entries[0].staged);

        app.execute_palette_command("git:refresh");
        assert_eq!(app.sidebar_tab, SidebarTab::SourceControl, "refresh no cierra");
        assert!(app.git_ui.entries[0].staged);

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: watcher de disco, pestaña limpia.
    #[test]
    fn poll_external_changes_reloads_clean_tab_from_disk() {
        let root = std::env::temp_dir().join(format!(
            "karkinos_watch_clean_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("carpeta temporal");
        let path = root.join("watched.txt");
        std::fs::write(&path, "primera\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(path.clone());
        assert_eq!(app.doc.lines, ["primera", ""]);
        assert!(!app.doc.dirty);

        std::fs::write(&path, "segunda linea\n").expect("reescritura");
        app.doc.tabs[app.doc.active_tab].last_disk_mtime = Some(std::time::SystemTime::UNIX_EPOCH);
        app.last_disk_poll = None;
        app.poll_external_changes();

        assert_eq!(app.doc.lines, ["segunda linea", ""]);
        assert_eq!(app.doc.buffer.to_string(), "segunda linea\n");
        assert!(!app.doc.dirty, "una recarga limpia no marca dirty");
        assert!(app.doc.tabs[app.doc.active_tab].last_disk_mtime.is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: watcher de disco, pestaña con ediciones sin guardar.
    #[test]
    fn poll_external_changes_preserves_dirty_tab() {
        let root = std::env::temp_dir().join(format!(
            "karkinos_watch_dirty_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("carpeta temporal");
        let path = root.join("edited.txt");
        std::fs::write(&path, "en disco\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(path.clone());
        app.set_text("edicion local\n");
        app.doc.dirty = true;

        std::fs::write(&path, "cambio externo\n").expect("reescritura");
        app.doc.tabs[app.doc.active_tab].last_disk_mtime = Some(std::time::SystemTime::UNIX_EPOCH);
        app.last_disk_poll = None;
        app.poll_external_changes();

        assert_eq!(app.doc.lines, ["edicion local", ""], "no pisa ediciones");
        assert!(app.doc.dirty, "sigue dirty");
        assert!(
            app.doc.status.contains("File modified on disk: edited.txt"),
            "status = {}",
            app.doc.status
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: apertura y cierre del visor de diff.
    #[test]
    fn open_and_close_git_diff_modal() {
        use karkinos_core::git_status::DiffLineKind;

        let root = git_temp_repo("diff_view");
        let path = root.join("view.txt");
        std::fs::write(&path, "uno\n").expect("archivo inicial");
        for args in [vec!["add", "view.txt"], vec!["commit", "-m", "inicial"]] {
            let output = std::process::Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .expect("ejecutar git");
            assert!(
                output.status.success(),
                "git {args:?} falló: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::write(&path, "uno\ndos\n").expect("archivo modificado");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.open_git_diff(&path, false);
        let view = app.git_diff_modal.as_ref().expect("modal abierto");
        assert_eq!(view.rel_name, "view.txt");
        assert!(!view.is_staged);
        assert!(view.diff_text.contains("+dos"));
        assert!(view
            .parsed_lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Added && line.text == "+dos"));

        app.close_git_diff();
        assert!(app.git_diff_modal.is_none(), "close limpia el modal");

        let stage = std::process::Command::new("git")
            .args(["add", "view.txt"])
            .current_dir(&root)
            .output()
            .expect("ejecutar git");
        assert!(stage.status.success());

        app.open_git_diff(&path, true);
        let view = app.git_diff_modal.as_ref().expect("modal staged");
        assert!(view.is_staged);
        assert!(view.diff_text.contains("+dos"));

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: comandos del visor en el catálogo del Command Palette.
    #[test]
    fn palette_git_diff_commands_open_and_close_modal() {
        let root = git_temp_repo("palette_diff");
        let path = root.join("active.txt");
        std::fs::write(&path, "linea\n").expect("archivo");
        for args in [vec!["add", "active.txt"], vec!["commit", "-m", "inicial"]] {
            let output = std::process::Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .expect("ejecutar git");
            assert!(output.status.success(), "git falló");
        }
        std::fs::write(&path, "linea\nnueva\n").expect("modificado");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(path);

        app.execute_palette_command("git:view_diff");
        assert!(app.git_diff_modal.is_some(), "view_diff abre el modal");

        app.execute_palette_command("git:close_diff");
        assert!(app.git_diff_modal.is_none(), "close_diff lo cierra");

        std::fs::remove_dir_all(&root).ok();
    }
}
