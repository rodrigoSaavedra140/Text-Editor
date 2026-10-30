//! Karkinos Core: Headless, high-performance text editing engine.
//!
//! Provides data structures and algorithms decoupled from graphics and UI:
//! - [`TextBuffer`]: Balanced B-Tree Rope text buffer via `ropey`.
//! - [`UndoStack`]: $O(1)$ Copy-On-Write undo/redo stack with idle typing chunking.
//! - [`LineEnding`]: Robust line ending detection and roundtrip serialization.
//! - [`Document`]: High-level headless document model.
//! - Text algorithms: Word boundary navigation, comment toggling, auto-indentation, and bracket matching.

pub mod buffer;
pub mod document;
pub mod ending;
pub mod text_ops;
pub mod undo;

pub use buffer::TextBuffer;
pub use document::Document;
pub use ending::{default_new_file_path, detect_line_ending, serialize_line_ending, LineEnding};
pub use text_ops::{
    char_to_byte, dedent_line, find_matching_bracket, indent_line, inherited_indent,
    is_word_char, line_char_count, next_word_pos, prev_word_pos, toggle_comment_range,
    word_range_at, BracketMatch,
};
pub use undo::{
    should_snapshot_immediately, ChunkTracker, Edit, EditAction, UndoStack, CHUNK_TIMEOUT,
    MAX_UNDO_EDITS,
};
