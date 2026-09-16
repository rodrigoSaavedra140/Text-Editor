use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::rope::Rope;

pub struct Buffer {
    rope: Rope,
    file_path: Option<PathBuf>,
    dirty: bool,
    modified_at: SystemTime,
    // Extensión práctica sobre el UML: el cursor vive en Buffer porque
    // es quien conoce el contenido; Rope::Cursor queda como utilidad
    // standalone para quien quiera navegar un Rope sin un Buffer.
    cursor: usize,
}

impl Buffer {
    pub fn new() -> Self {
        Buffer {
            rope: Rope::new(""),
            file_path: None,
            dirty: false,
            modified_at: SystemTime::now(),
            cursor: 0,
        }
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Buffer {
            rope: Rope::new(&text),
            file_path: Some(path.to_path_buf()),
            dirty: false,
            modified_at: SystemTime::now(),
            cursor: 0,
        })
    }

    pub fn save(&mut self) -> io::Result<()> {
        match &self.file_path {
            Some(path) => {
                fs::write(path, self.rope.to_string())?;
                self.dirty = false;
                Ok(())
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, "buffer sin archivo asociado")),
        }
    }

    pub fn set_file_path(&mut self, path: PathBuf) {
        self.file_path = Some(path);
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn set_rope(&mut self, rope: Rope) {
        self.rope = rope;
    }

    pub fn insert(&mut self, pos: usize, text: &str) {
        self.rope = self.rope.insert(pos, text);
        self.dirty = true;
        self.modified_at = SystemTime::now();
    }

    pub fn delete(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        self.rope = self.rope.delete(start, end);
        self.dirty = true;
        self.modified_at = SystemTime::now();
    }

    pub fn line(&self, line_num: usize) -> String {
        self.rope.to_string().lines().nth(line_num).unwrap_or("").to_string()
    }

    pub fn line_count(&self) -> usize {
        let text = self.rope.to_string();
        if text.is_empty() {
            1
        } else {
            text.lines().count().max(1)
        }
    }

    pub fn is_modified(&self) -> bool {
        self.dirty
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos.min(self.rope.len());
    }
}
