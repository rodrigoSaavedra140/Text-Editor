//! S5 (Roadmap v0.2.9): servicio LSP (rust-analyzer) del editor.
//!
//! T5b: `LspService` posee la conexión cliente y el protocolo de
//! pedidos (definitions, hover, code actions, inlay hints, rename).
//! Los métodos piden la lectura del documento activo y del estado de
//! presentación por parámetros; el consumo de respuestas que muta el
//! documento varias pestañas (`poll_lsp_results`, `apply_workspace_edit`,
//! `apply_text_edits_*`, `apply_code_action`) queda como orquestación
//! en `app.rs`, porque cruza documento, AST y presentación a la vez.

use crate::document_service::DocumentService;
use crate::editor_widget::EditorState;

/// S4 (Etapa 4): servicio del Language Server Protocol.
pub struct LspService {
    pub client: Option<karkinos_core::lsp::LspClient>,
    // S4 (Rust IDE): Code Actions & Rename
    pub code_actions: Vec<karkinos_core::lsp::LspCodeAction>,
    pub code_actions_menu_open: bool,
    pub rename_dialog_open: bool,
    pub rename_input: String,
}

impl Default for LspService {
    fn default() -> Self {
        LspService {
            client: None,
            code_actions: Vec::new(),
            code_actions_menu_open: false,
            rename_dialog_open: false,
            rename_input: String::new(),
        }
    }
}

impl LspService {
    /// S4 (Etapa 4): Actualiza diagnósticos y estado del cliente LSP.
    /// `is_rust` lo resuelve el orquestador (usa el vocabulary de la app).
    pub fn update_lsp(
        &mut self,
        editor_state: &mut EditorState,
        doc: &mut DocumentService,
        is_rust: bool,
    ) {
        if !is_rust {
            editor_state.diagnostics.clear();
            return;
        }

        if self.client.is_none() {
            let client = karkinos_core::lsp::LspClient::spawn("rust-analyzer", &[], &doc.browse_dir)
                .unwrap_or_else(|_| karkinos_core::lsp::LspClient::mock());
            self.client = Some(client);
        }

        if let (Some(ref client), Some(ref path)) = (&self.client, &doc.current_file) {
            let uri = karkinos_core::path_to_uri(path);
            let text = doc.current_text();
            client.did_change(&uri, 2, &text);
            editor_state.diagnostics = client.diagnostics_for_file(&uri);

            if let Some(diag) = editor_state.diagnostics.iter().find(|d| d.range.start.line == editor_state.cursor_line) {
                editor_state.hover_text = Some(diag.message.clone());
                doc.status = format!("LSP: {}", diag.message);
            }

            let line_count = doc.lines.len();
            client.request_inlay_hints(
                &uri,
                karkinos_core::lsp::LspRange {
                    start: karkinos_core::lsp::LspPosition { line: 0, character: 0 },
                    end: karkinos_core::lsp::LspPosition { line: line_count, character: 0 },
                },
            );
        }
    }

    /// S2 (Rust IDE): Solicita saltar a la definición del símbolo bajo el cursor (F12).
    pub fn goto_definition(
        &mut self,
        editor_state: &EditorState,
        doc: &mut DocumentService,
        is_rust: bool,
    ) {
        if !is_rust {
            return;
        }
        if let (Some(ref client), Some(ref path)) = (&self.client, &doc.current_file) {
            let uri = karkinos_core::path_to_uri(path);
            client.request_definition(&uri, editor_state.cursor_line, editor_state.cursor_col);
            doc.status = "Resolving definition (F12)…".to_string();
        }
    }

    /// S2 (Rust IDE): Solicita información de hover / documentación para el símbolo bajo el cursor.
    pub fn request_hover(
        &mut self,
        editor_state: &EditorState,
        doc: &DocumentService,
        is_rust: bool,
    ) {
        if !is_rust {
            return;
        }
        if let (Some(ref client), Some(ref path)) = (&self.client, &doc.current_file) {
            let uri = karkinos_core::path_to_uri(path);
            client.request_hover(&uri, editor_state.cursor_line, editor_state.cursor_col);
        }
    }

    /// Requests code actions (quick-fixes, auto-imports) for current cursor position and diagnostics.
    pub fn request_code_actions(
        &mut self,
        editor_state: &EditorState,
        doc: &mut DocumentService,
        is_rust: bool,
    ) {
        if !is_rust {
            return;
        }
        if let (Some(ref client), Some(ref path)) = (&self.client, &doc.current_file) {
            let uri = karkinos_core::path_to_uri(path);
            let range = karkinos_core::lsp::LspRange {
                start: karkinos_core::lsp::LspPosition {
                    line: editor_state.cursor_line,
                    character: editor_state.cursor_col,
                },
                end: karkinos_core::lsp::LspPosition {
                    line: editor_state.cursor_line,
                    character: editor_state.cursor_col,
                },
            };
            let diags = editor_state.diagnostics.clone();
            client.request_code_actions(&uri, range, &diags);
            doc.status = "Querying code actions (Ctrl+.)…".to_string();
        }
    }

    /// Requests inlay hints for the current buffer.
    #[allow(dead_code)]
    pub fn request_inlay_hints(&mut self, doc: &DocumentService, is_rust: bool) {
        if !is_rust {
            return;
        }
        if let (Some(ref client), Some(ref path)) = (&self.client, &doc.current_file) {
            let uri = karkinos_core::path_to_uri(path);
            let line_count = doc.lines.len();
            let range = karkinos_core::lsp::LspRange {
                start: karkinos_core::lsp::LspPosition { line: 0, character: 0 },
                end: karkinos_core::lsp::LspPosition { line: line_count, character: 0 },
            };
            client.request_inlay_hints(&uri, range);
        }
    }

    /// Requests workspace rename of symbol at cursor.
    pub fn request_rename(
        &mut self,
        editor_state: &EditorState,
        doc: &mut DocumentService,
        is_rust: bool,
        new_name: &str,
    ) {
        if !is_rust || new_name.trim().is_empty() {
            return;
        }
        if let (Some(ref client), Some(ref path)) = (&self.client, &doc.current_file) {
            let uri = karkinos_core::path_to_uri(path);
            let pos = karkinos_core::lsp::LspPosition {
                line: editor_state.cursor_line,
                character: editor_state.cursor_col,
            };
            client.request_rename(&uri, pos, new_name.trim());
            doc.status = format!("Renaming symbol to '{}'...", new_name.trim());
        }
    }
}
