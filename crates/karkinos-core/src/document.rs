//! Headless Document abstraction decoupled from GUI and rendering logic.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ropey::Rope;

use crate::buffer::TextBuffer;
use crate::ending::{detect_line_ending, serialize_line_ending, LineEnding};
use crate::syntax_ast::{AstEngine, AstHighlightSpan, AstLanguage, FoldingRange};
use crate::undo::{Edit, UndoStack};

/// Represents an active document managed by the core editing engine.
#[derive(Clone)]
pub struct Document {
    pub id: usize,
    pub buffer: TextBuffer,
    pub undo_stack: UndoStack,
    pub last_snapshot: Rope,
    pub file_path: Option<PathBuf>,
    pub file_path_input: String,
    pub dirty: bool,
    pub line_ending: LineEnding,
    pub last_disk_mtime: Option<SystemTime>,
    pub view_bytes: usize,
    pub ast_engine: Option<AstEngine>,
    pub lsp_client: Option<crate::lsp::LspClient>,
}

impl Document {
    /// Creates a new empty untitled document.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            buffer: TextBuffer::new(),
            undo_stack: UndoStack::new(),
            last_snapshot: Rope::from_str(""),
            file_path: None,
            file_path_input: String::new(),
            dirty: false,
            line_ending: LineEnding::Lf,
            last_disk_mtime: None,
            view_bytes: 0,
            ast_engine: None,
            lsp_client: None,
        }
    }

    /// Attaches an LSP client and notifies it with `didOpen` if a file path is configured.
    pub fn attach_lsp(&mut self, client: crate::lsp::LspClient, language_id: &str) {
        if let Some(ref path) = self.file_path {
            let uri = format!("file://{}", path.display());
            client.did_open(&uri, language_id, &self.buffer.to_string());
        }
        self.lsp_client = Some(client);
    }

    /// Queries live diagnostics for this document from the attached LSP client.
    pub fn diagnostics(&self) -> Vec<crate::lsp::LspDiagnostic> {
        if let (Some(client), Some(path)) = (&self.lsp_client, &self.file_path) {
            let uri = format!("file://{}", path.display());
            client.diagnostics_for_file(&uri)
        } else {
            Vec::new()
        }
    }

    /// Enables the AST engine for a supported language.
    pub fn enable_ast(&mut self, lang: AstLanguage) -> Result<(), String> {
        let mut engine = AstEngine::new(lang)?;
        engine.update_from_buffer(&self.buffer);
        self.ast_engine = Some(engine);
        Ok(())
    }

    /// Re-parses AST if an engine is attached.
    pub fn update_ast(&mut self) {
        if let Some(engine) = &mut self.ast_engine {
            engine.update_from_buffer(&self.buffer);
        }
    }

    /// Extracts code folding ranges via AST.
    pub fn folding_ranges(&self) -> Vec<FoldingRange> {
        self.ast_engine
            .as_ref()
            .map(|e| e.folding_ranges())
            .unwrap_or_default()
    }

    /// Extracts semantic highlight tokens via AST.
    pub fn highlight_spans(&self) -> Vec<AstHighlightSpan> {
        self.ast_engine
            .as_ref()
            .map(|e| e.highlight_spans())
            .unwrap_or_default()
    }

    /// Sets the text content, normalizes CRLF, recalculates byte metrics, and updates AST & LSP.
    pub fn set_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n");
        self.buffer.set_text(&text);
        self.recalc_view_bytes();
        self.update_ast();
        if let (Some(client), Some(path)) = (&self.lsp_client, &self.file_path) {
            let uri = format!("file://{}", path.display());
            client.did_change(&uri, 2, &text);
        }
    }

    /// Recalculates total UTF-8 byte count.
    pub fn recalc_view_bytes(&mut self) {
        self.view_bytes = self.buffer.len_bytes();
    }

    /// Returns the full text with its configured line ending style.
    pub fn text_with_line_ending(&self) -> String {
        let text = self.buffer.to_string();
        serialize_line_ending(&text, self.line_ending)
    }

    /// Captures a snapshot for undo/redo if the text has changed since the last snapshot.
    pub fn snapshot_if_changed(&mut self) {
        let current = self.buffer.snapshot();
        if current != self.last_snapshot {
            self.undo_stack.push(Edit {
                before: self.last_snapshot.clone(),
                after: current.clone(),
            });
            self.last_snapshot = current;
            self.dirty = true;
        }
    }

    /// Undoes the last edit and returns the restored text if successful.
    pub fn undo(&mut self) -> Option<String> {
        self.snapshot_if_changed();
        let edit = self.undo_stack.undo()?;
        let snap = edit.before;
        self.buffer.restore_snapshot(snap.clone());
        self.last_snapshot = snap;
        self.recalc_view_bytes();
        Some(self.buffer.to_string())
    }

    /// Redoes the next edit and returns the restored text if successful.
    pub fn redo(&mut self) -> Option<String> {
        let edit = self.undo_stack.redo()?;
        let snap = edit.after;
        self.buffer.restore_snapshot(snap.clone());
        self.last_snapshot = snap;
        self.recalc_view_bytes();
        Some(self.buffer.to_string())
    }

    /// Applies a set of LSP text edits transactionally with undo support.
    /// Edits are sorted in descending document position order before application.
    pub fn apply_text_edits(&mut self, edits: &[crate::lsp::LspTextEdit]) {
        if edits.is_empty() {
            return;
        }
        self.snapshot_if_changed();
        let mut sorted = edits.to_vec();
        sorted.sort_by(|a, b| {
            (b.range.start.line, b.range.start.character)
                .cmp(&(a.range.start.line, a.range.start.character))
        });

        for edit in sorted {
            let start_char = self
                .buffer
                .line_col_to_char(edit.range.start.line, edit.range.start.character);
            let end_char = self
                .buffer
                .line_col_to_char(edit.range.end.line, edit.range.end.character);

            if start_char < end_char {
                self.buffer.remove(start_char..end_char);
            }
            if !edit.new_text.is_empty() {
                self.buffer.insert(start_char, &edit.new_text);
            }
        }

        self.recalc_view_bytes();
        self.update_ast();
        self.dirty = true;
        self.snapshot_if_changed();

        if let (Some(client), Some(path)) = (&self.lsp_client, &self.file_path) {
            let uri = format!("file://{}", path.display());
            client.did_change(&uri, 2, &self.buffer.to_string());
        }
    }

    /// Returns a user-friendly display title for tabs and window titles.
    pub fn display_title(&self) -> String {
        let name = if let Some(ref p) = self.file_path {
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

    /// Loads content from a disk file.
    pub fn load_from_file(id: usize, path: &Path) -> std::io::Result<Self> {
        let raw = fs::read(path)?;
        let text = String::from_utf8_lossy(&raw);
        let ending = detect_line_ending(&text);
        let mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());

        let mut doc = Self::new(id);
        doc.set_text(&text);
        doc.last_snapshot = Rope::from_str(&doc.buffer.to_string());
        doc.line_ending = ending;
        doc.last_disk_mtime = mtime;
        doc.file_path = Some(path.to_path_buf());
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            doc.file_path_input = name.to_string();
        }
        doc.dirty = false;
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_lifecycle_and_undo() {
        let mut doc = Document::new(1);
        assert_eq!(doc.display_title(), "Untitled");

        doc.set_text("hello");
        doc.snapshot_if_changed();

        doc.set_text("hello world");
        doc.snapshot_if_changed();

        assert_eq!(doc.buffer.to_string(), "hello world");
        assert!(doc.dirty);

        let undone = doc.undo().expect("should undo");
        assert_eq!(undone, "hello");
        assert_eq!(doc.buffer.to_string(), "hello");

        let redone = doc.redo().expect("should redo");
        assert_eq!(redone, "hello world");
        assert_eq!(doc.buffer.to_string(), "hello world");
    }

    #[test]
    fn test_document_ast_integration() {
        let mut doc = Document::new(2);
        doc.enable_ast(AstLanguage::Rust).expect("should enable rust AST");
        doc.set_text("fn test() {\n    let val = 42;\n}");

        let foldings = doc.folding_ranges();
        assert_eq!(foldings.len(), 2); // function and block
        let spans = doc.highlight_spans();
        assert!(!spans.is_empty());
    }

    #[test]
    fn test_document_lsp_integration() {
        use crate::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut doc = Document::new(3);
        doc.file_path = Some(PathBuf::from("/test/src/main.rs"));
        let client = LspClient::mock();

        client.set_diagnostics_for_file(
            "file:///test/src/main.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 1, character: 0 },
                    end: LspPosition { line: 1, character: 5 },
                },
                severity: Some(DiagnosticSeverity::Error),
                code: Some("E0425".to_string()),
                source: Some("rustc".to_string()),
                message: "cannot find value `x` in this scope".to_string(),
            }],
        );

        doc.attach_lsp(client, "rust");
        let diags = doc.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "cannot find value `x` in this scope");
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    }

    #[test]
    fn test_document_apply_text_edits() {
        use crate::lsp::{LspPosition, LspRange, LspTextEdit};

        let mut doc = Document::new(4);
        doc.set_text("fn main() {\n    let a = 1;\n    let b = 2;\n}\n");

        // Apply two edits: add an import at the top, and change `let b = 2` to `let b = 42`
        let edits = vec![
            LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 0 },
                },
                new_text: "use std::io;\n".to_string(),
            },
            LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: 2, character: 12 },
                    end: LspPosition { line: 2, character: 13 },
                },
                new_text: "42".to_string(),
            },
        ];

        doc.apply_text_edits(&edits);
        let text = doc.buffer.to_string();
        assert!(text.starts_with("use std::io;\nfn main()"));
        assert!(text.contains("let b = 42;"));
        assert!(doc.dirty);

        // Undo should restore previous state
        doc.undo();
        let restored = doc.buffer.to_string();
        assert_eq!(restored, "fn main() {\n    let a = 1;\n    let b = 2;\n}\n");
    }
}
