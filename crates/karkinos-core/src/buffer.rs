//! Core text buffer engine based on `ropey::Rope`.
//!
//! Provides:
//! - Mutations (insertion / deletion) in $O(\log N)$ without global memory reallocations.
//! - Instant bidirectional index mapping between `(line, col)` coordinates and byte/char offsets.
//! - $O(1)$ snapshots (Copy-On-Write) for history and undo/redo stacks.
//! - Safe, bounded slice access by line.

use ropey::{Rope, RopeSlice};
use std::ops::Range;

#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    rope: Rope,
}

impl TextBuffer {
    /// Creates an empty buffer.
    pub fn new() -> Self {
        Self {
            rope: Rope::from_str(""),
        }
    }

    /// Initializes buffer from a string slice.
    pub fn from_str(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
        }
    }

    /// Returns the total logical line count. Always returns at least 1.
    pub fn len_lines(&self) -> usize {
        let count = self.rope.len_lines();
        if count == 0 {
            1
        } else {
            count
        }
    }

    /// Returns the total character count (Unicode scalar values).
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Returns the total byte count in UTF-8.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Returns whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Returns a `RopeSlice` for the specified line (0-based).
    pub fn line(&self, line_idx: usize) -> Option<RopeSlice<'_>> {
        if line_idx < self.rope.len_lines() {
            Some(self.rope.line(line_idx))
        } else if line_idx == 0 && self.rope.len_lines() == 0 {
            Some(self.rope.slice(..))
        } else {
            None
        }
    }

    /// Returns the line content without trailing line ending (`\n` or `\r\n`).
    pub fn line_without_newline(&self, line_idx: usize) -> String {
        let Some(slice) = self.line(line_idx) else {
            return String::new();
        };

        let mut s = slice.to_string();
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        s
    }

    /// Returns the character count in a line excluding the newline.
    pub fn line_char_count(&self, line_idx: usize) -> usize {
        let Some(slice) = self.line(line_idx) else {
            return 0;
        };
        let len = slice.len_chars();
        if len == 0 {
            return 0;
        }

        let mut sub = 0;
        if slice.char(len - 1) == '\n' {
            sub += 1;
            if len > 1 && slice.char(len - 2) == '\r' {
                sub += 1;
            }
        }
        len - sub
    }

    /// Converts `(line, col)` coordinates to a global character index.
    pub fn line_col_to_char(&self, line: usize, col: usize) -> usize {
        let total_lines = self.len_lines();
        let l = line.min(total_lines.saturating_sub(1));
        let line_start_char = if l < self.rope.len_lines() {
            self.rope.line_to_char(l)
        } else {
            self.rope.len_chars()
        };
        let line_chars = self.line_char_count(l);
        let c = col.min(line_chars);
        (line_start_char + c).min(self.rope.len_chars())
    }

    /// Converts a global character index to `(line, col)` coordinates.
    pub fn char_to_line_col(&self, char_idx: usize) -> (usize, usize) {
        if self.rope.len_chars() == 0 {
            return (0, 0);
        }
        let safe_char = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(safe_char);
        let line_start = self.rope.line_to_char(line);
        let col = safe_char.saturating_sub(line_start);
        (line, col)
    }

    /// Inserts text at a given character position in $O(\log N)$.
    pub fn insert(&mut self, char_idx: usize, text: &str) {
        let safe_idx = char_idx.min(self.rope.len_chars());
        self.rope.insert(safe_idx, text);
    }

    /// Removes a range of characters `[start..end)` in $O(\log N)$.
    pub fn remove(&mut self, range: Range<usize>) {
        let total = self.rope.len_chars();
        let start = range.start.min(total);
        let end = range.end.min(total);
        if start < end {
            self.rope.remove(start..end);
        }
    }

    /// Inserts text at `(line, col)` and returns the new cursor position.
    pub fn insert_at_line_col(&mut self, line: usize, col: usize, text: &str) -> (usize, usize) {
        let char_pos = self.line_col_to_char(line, col);
        self.insert(char_pos, text);
        let new_char_pos = char_pos + text.chars().count();
        self.char_to_line_col(new_char_pos)
    }

    /// Removes the two-dimensional range `[start..(line, col), end..(line, col)]`.
    pub fn remove_line_col_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        let (s, e) = if start <= end { (start, end) } else { (end, start) };
        let char_start = self.line_col_to_char(s.0, s.1);
        let char_end = self.line_col_to_char(e.0, e.1);
        self.remove(char_start..char_end);
    }

    /// Returns an instant $O(1)$ snapshot of the internal B-Tree Rope.
    pub fn snapshot(&self) -> Rope {
        self.rope.clone()
    }

    /// Restores the buffer from an $O(1)$ snapshot.
    pub fn restore_snapshot(&mut self, snapshot: Rope) {
        self.rope = snapshot;
    }

    /// Exports the full buffer contents to a standard `String`.
    pub fn to_string(&self) -> String {
        self.rope.to_string()
    }

    /// Replaces the entire buffer contents.
    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
    }

    /// Returns the logical lines as a vector of Strings.
    pub fn to_lines_vec(&self) -> Vec<String> {
        let total = self.len_lines();
        let mut out = Vec::with_capacity(total);
        for i in 0..total {
            out.push(self.line_without_newline(i));
        }
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_creation_and_lines() {
        let buf = TextBuffer::new();
        assert_eq!(buf.len_lines(), 1);
        assert_eq!(buf.len_chars(), 0);
        assert_eq!(buf.line_without_newline(0), "");

        let buf2 = TextBuffer::from_str("hello\nworld\n");
        assert_eq!(buf2.len_lines(), 3);
        assert_eq!(buf2.line_without_newline(0), "hello");
        assert_eq!(buf2.line_without_newline(1), "world");
        assert_eq!(buf2.line_without_newline(2), "");
    }

    #[test]
    fn test_line_col_conversions() {
        let buf = TextBuffer::from_str("first\nsecond line\nthird");
        assert_eq!(buf.line_col_to_char(0, 0), 0);
        assert_eq!(buf.line_col_to_char(0, 5), 5);
        assert_eq!(buf.line_col_to_char(1, 0), 6);
        assert_eq!(buf.char_to_line_col(6), (1, 0));
        assert_eq!(buf.char_to_line_col(8), (1, 2));
    }

    #[test]
    fn test_insertion_and_deletion() {
        let mut buf = TextBuffer::from_str("hello world");
        buf.insert(5, " beautiful");
        assert_eq!(buf.to_string(), "hello beautiful world");

        buf.remove(5..15);
        assert_eq!(buf.to_string(), "hello world");

        let (nl, nc) = buf.insert_at_line_col(0, 5, "\n");
        assert_eq!(nl, 1);
        assert_eq!(nc, 0);
        assert_eq!(buf.len_lines(), 2);
        assert_eq!(buf.line_without_newline(0), "hello");
        assert_eq!(buf.line_without_newline(1), " world");
    }

    #[test]
    fn test_snapshot_o1_copy_on_write() {
        let mut buf = TextBuffer::from_str("version 1");
        let snap1 = buf.snapshot();

        buf.insert(9, " modified");
        assert_eq!(buf.to_string(), "version 1 modified");
        assert_eq!(snap1.to_string(), "version 1");

        buf.restore_snapshot(snap1);
        assert_eq!(buf.to_string(), "version 1");
    }

    #[test]
    fn test_stress_large_document() {
        let initial_lines: Vec<String> = (0..50_000).map(|i| format!("Line content number {i}")).collect();
        let big_text = initial_lines.join("\n");

        let mut buf = TextBuffer::from_str(&big_text);
        assert_eq!(buf.len_lines(), 50_000);

        let snap = buf.snapshot();
        assert_eq!(snap.len_lines(), 50_000);

        let (l, c) = buf.insert_at_line_col(25_000, 4, "INJECTED ");
        assert_eq!(l, 25_000);
        assert_eq!(c, 13);
        assert!(buf.line_without_newline(25_000).starts_with("LineINJECTED "));

        buf.restore_snapshot(snap);
        assert!(!buf.line_without_newline(25_000).contains("INJECTED"));
    }
}
