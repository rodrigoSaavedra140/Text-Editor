//! Headless Document abstraction decoupled from GUI and rendering logic.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ropey::Rope;

use crate::buffer::TextBuffer;
use crate::ending::{detect_line_ending, serialize_line_ending, LineEnding};
use crate::undo::{Edit, UndoStack};

/// Represents an active document managed by the core editing engine.
#[derive(Debug, Clone)]
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
        }
    }

    /// Sets the text content, normalizes CRLF, and recalculates byte metrics.
    pub fn set_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n");
        self.buffer.set_text(&text);
        self.recalc_view_bytes();
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
}
