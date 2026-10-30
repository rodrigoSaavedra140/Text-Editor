//! Motor de búfer de texto industrial basado en `ropey::Rope`.
//!
//! Reemplaza representaciones ingenuas (como `Vec<String>`) proporcionando:
//! - Mutaciones (inserción / borrado) en $O(\log N)$ sin realocaciones globales de memoria.
//! - Indexación bidireccional instantánea entre coordenadas visuales `(línea, columna)`
//!   y desplazamientos de caracteres/bytes.
//! - Clonación de snapshots en tiempo $O(1)$ (Copy-On-Write) para el historial de deshacer/rehacer.
//! - Acceso seguro y acotado por líneas sin copiar cadenas intermedias.

use ropey::{Rope, RopeSlice};
use std::ops::Range;

#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    rope: Rope,
}

impl TextBuffer {
    /// Crea un búfer vacío (contiene una línea vacía).
    pub fn new() -> Self {
        Self {
            rope: Rope::from_str(""),
        }
    }

    /// Inicializa el búfer a partir de una cadena de texto.
    pub fn from_str(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
        }
    }

    /// Retorna la cantidad total de líneas lógicas en el documento.
    /// Garantiza siempre al menos 1 línea.
    pub fn len_lines(&self) -> usize {
        let count = self.rope.len_lines();
        if count == 0 {
            1
        } else {
            count
        }
    }

    /// Retorna la cantidad total de caracteres (puntos de código Unicode) del documento.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Retorna la cantidad total de bytes codificados en UTF-8.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Verifica si el búfer no contiene texto.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Retorna una rebanada (`RopeSlice`) de la línea especificada (0-based).
    pub fn line(&self, line_idx: usize) -> Option<RopeSlice<'_>> {
        if line_idx < self.rope.len_lines() {
            Some(self.rope.line(line_idx))
        } else if line_idx == 0 && self.rope.len_lines() == 0 {
            Some(self.rope.slice(..))
        } else {
            None
        }
    }

    /// Retorna el contenido de la línea sin el terminador de salto de línea final (`\n` o `\r\n`).
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

    /// Retorna la cantidad de caracteres en la línea sin contar el salto de línea final.
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

    /// Convierte coordenadas `(línea, columna)` al índice global de carácter del búfer.
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

    /// Convierte un índice global de carácter a coordenadas `(línea, columna)`.
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

    /// Inserta texto en una posición dada por índice de carácter. $O(\log N)$.
    pub fn insert(&mut self, char_idx: usize, text: &str) {
        let safe_idx = char_idx.min(self.rope.len_chars());
        self.rope.insert(safe_idx, text);
    }

    /// Elimina un rango de caracteres `[start..end)`. $O(\log N)$.
    pub fn remove(&mut self, range: Range<usize>) {
        let total = self.rope.len_chars();
        let start = range.start.min(total);
        let end = range.end.min(total);
        if start < end {
            self.rope.remove(start..end);
        }
    }

    /// Inserta texto en las coordenadas `(línea, columna)` y retorna la nueva posición de cursor.
    pub fn insert_at_line_col(&mut self, line: usize, col: usize, text: &str) -> (usize, usize) {
        let char_pos = self.line_col_to_char(line, col);
        self.insert(char_pos, text);
        let new_char_pos = char_pos + text.chars().count();
        self.char_to_line_col(new_char_pos)
    }

    /// Elimina el rango bidimensional `[start..(line, col), end..(line, col)]`.
    pub fn remove_line_col_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        let (s, e) = if start <= end { (start, end) } else { (end, start) };
        let char_start = self.line_col_to_char(s.0, s.1);
        let char_end = self.line_col_to_char(e.0, e.1);
        self.remove(char_start..char_end);
    }

    /// Retorna una copia instantánea $O(1)$ de la estructura interna del búfer
    /// para snapshots de historial (Copy-On-Write sobre el árbol B de Ropey).
    pub fn snapshot(&self) -> Rope {
        self.rope.clone()
    }

    /// Restaura el búfer a partir de un snapshot de Rope en tiempo $O(1)$.
    pub fn restore_snapshot(&mut self, snapshot: Rope) {
        self.rope = snapshot;
    }

    /// Exporta todo el contenido del búfer a un `String` estándar.
    pub fn to_string(&self) -> String {
        self.rope.to_string()
    }

    /// Reemplaza todo el contenido con una nueva cadena.
    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
    }

    /// Retorna las líneas lógicas del búfer como un vector de Strings.
    /// Útil para interoperabilidad gradual durante migraciones de subsistemas.
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

        let buf2 = TextBuffer::from_str("hola\nmundo\n");
        assert_eq!(buf2.len_lines(), 3);
        assert_eq!(buf2.line_without_newline(0), "hola");
        assert_eq!(buf2.line_without_newline(1), "mundo");
        assert_eq!(buf2.line_without_newline(2), "");
    }

    #[test]
    fn test_line_col_conversions() {
        let buf = TextBuffer::from_str("primera\nsegunda linea\ntercera");
        // línea 0: "primera\n" (7 chars + 1 newline = 8)
        assert_eq!(buf.line_col_to_char(0, 0), 0);
        assert_eq!(buf.line_col_to_char(0, 7), 7);
        // línea 1 arranca en char 8
        assert_eq!(buf.line_col_to_char(1, 0), 8);
        assert_eq!(buf.char_to_line_col(8), (1, 0));
        assert_eq!(buf.char_to_line_col(10), (1, 2));
    }

    #[test]
    fn test_insertion_and_deletion() {
        let mut buf = TextBuffer::from_str("hola mundo");
        buf.insert(4, " hermoso");
        assert_eq!(buf.to_string(), "hola hermoso mundo");

        buf.remove(4..12);
        assert_eq!(buf.to_string(), "hola mundo");

        let (nl, nc) = buf.insert_at_line_col(0, 4, "\n");
        assert_eq!(nl, 1);
        assert_eq!(nc, 0);
        assert_eq!(buf.len_lines(), 2);
        assert_eq!(buf.line_without_newline(0), "hola");
        assert_eq!(buf.line_without_newline(1), " mundo");
    }

    #[test]
    fn test_snapshot_o1_copy_on_write() {
        let mut buf = TextBuffer::from_str("version 1");
        let snap1 = buf.snapshot();

        buf.insert_at_line_col(0, 9, " con cambios");
        assert_eq!(buf.to_string(), "version 1 con cambios");

        // El snapshot original permanece intacto
        assert_eq!(snap1.to_string(), "version 1");

        // Restaurar snapshot
        buf.restore_snapshot(snap1);
        assert_eq!(buf.to_string(), "version 1");
    }

    #[test]
    fn test_stress_large_document() {
        // Generar 50.000 líneas rápidamente
        let mut big_text = String::with_capacity(1_000_000);
        for i in 0..50_000 {
            big_text.push_str("linea de prueba para stress ");
            big_text.push_str(&i.to_string());
            big_text.push('\n');
        }

        let mut buf = TextBuffer::from_str(&big_text);
        assert_eq!(buf.len_lines(), 50_001);

        // Snapshot instantáneo O(1)
        let snap = buf.snapshot();

        // Inserción en el medio O(log N)
        let mid_char = buf.line_col_to_char(25_000, 5);
        buf.insert(mid_char, "INSERCION_RAPIDA");
        assert!(buf.line_without_newline(25_000).contains("INSERCION_RAPIDA"));

        // Restaurar
        buf.restore_snapshot(snap);
        assert!(!buf.line_without_newline(25_000).contains("INSERCION_RAPIDA"));
    }
}
