//! Karkinos Core: Headless, high-performance text editing engine.
//!
//! Provides data structures and algorithms decoupled from graphics and UI:
//! - [`TextBuffer`]: Balanced B-Tree Rope text buffer via `ropey`.
//! - [`UndoStack`]: $O(1)$ Copy-On-Write undo/redo stack with idle typing chunking.
//! - [`LineEnding`]: Robust line ending detection and roundtrip serialization.
//! - [`Document`]: High-level headless document model.
//! - Text algorithms: Word boundary navigation, comment toggling, auto-indentation, and bracket matching.

pub mod buffer;
pub mod cargo;
pub mod dap;
pub mod document;
pub mod ending;
pub mod lsp;
pub mod open_vsx;
pub mod platform;
pub mod plugin;
pub mod syntax_ast;
pub mod terminal;
pub mod text_ops;
pub mod undo;

pub use buffer::TextBuffer;
pub use cargo::{CargoAction, CargoDiagnostic, CargoOutputLine, CargoRunner, CargoStatus};
pub use dap::{DapBreakpoint, DapClient, DapStackFrame, DapState, DapStopReason, DapVariable};
pub use document::Document;
pub use ending::{default_new_file_path, detect_line_ending, serialize_line_ending, LineEnding};
pub use lsp::{
    DiagnosticSeverity, InlayHintKind, LspClient, LspCodeAction, LspCompletionItem, LspDiagnostic,
    LspHover, LspInlayHint, LspLocation, LspPosition, LspRange, LspTextEdit, LspWorkspaceEdit,
};
pub use open_vsx::{OpenVsxClient, OpenVsxExtension, OpenVsxSearchResult};
pub use platform::{configure_background_command, path_to_uri, uri_to_path};
pub use plugin::{Plugin, PluginCommand, PluginContext, PluginManager};
pub use syntax_ast::{
    AstEngine, AstHighlightSpan, AstLanguage, AstNodeInfo, FoldingKind, FoldingRange,
    SemanticTokenType,
};
pub use terminal::{PtyTerminal, TerminalCell, TerminalColor, TerminalSnapshot};
pub use text_ops::{
    char_to_byte, dedent_line, find_matching_bracket, indent_line, inherited_indent,
    is_word_char, line_char_count, next_word_pos, prev_word_pos, toggle_comment_range,
    word_range_at, BracketMatch,
};
pub use undo::{
    should_snapshot_immediately, ChunkTracker, Edit, EditAction, UndoStack, CHUNK_TIMEOUT,
    MAX_UNDO_EDITS,
};
