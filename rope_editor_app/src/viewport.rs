use std::ops::Range;

pub struct Viewport {
    top_line: usize,
    left_col: usize,
    width: usize,
    height: usize,
}

impl Viewport {
    pub fn new(width: usize, height: usize) -> Self {
        Viewport { top_line: 0, left_col: 0, width, height }
    }

    pub fn scroll(&mut self, delta: isize) {
        let new_top = self.top_line as isize + delta;
        self.top_line = new_top.max(0) as usize;
    }

    pub fn resize(&mut self, w: usize, h: usize) {
        self.width = w;
        self.height = h;
    }

    pub fn visible_lines(&self) -> Range<usize> {
        self.top_line..(self.top_line + self.height)
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn left_col(&self) -> usize {
        self.left_col
    }

    /// Ajusta top_line para que `line` quede dentro del área visible.
    /// No está en el UML original, pero es necesaria para que el
    /// cursor no "desaparezca" al moverse fuera de la pantalla.
    pub fn ensure_visible(&mut self, line: usize) {
        if line < self.top_line {
            self.top_line = line;
        } else if self.height > 0 && line >= self.top_line + self.height {
            self.top_line = line + 1 - self.height;
        }
    }
}
