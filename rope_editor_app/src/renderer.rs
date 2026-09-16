use crate::backend::Backend;
use crate::buffer::Buffer;
use crate::style::Style;
use crate::viewport::Viewport;

pub struct Renderer {
    backend: Box<dyn Backend>,
}

impl Renderer {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Renderer { backend }
    }

    pub fn render(&mut self, buffer: &Buffer, viewport: &Viewport) {
        self.backend.clear();
        let style = Style::default();
        for (row, line_num) in viewport.visible_lines().enumerate() {
            if line_num >= buffer.line_count() {
                break;
            }
            let line = buffer.line(line_num);
            let visible: String = line.chars().skip(viewport.left_col()).take(viewport.width()).collect();
            self.backend.draw_text(0, row as u16, &visible, style);
        }
        self.backend.flush();
    }

    // El UML original pasaba `editor: &Editor`, pero eso genera un
    // conflicto de borrow en Rust (Editor tendría que prestarse a sí
    // mismo mientras presta su propio Renderer). En su lugar recibe
    // el texto y el estilo ya calculados por Editor.
    pub fn draw_status_bar(&mut self, status: &str, style: Style) {
        let (_, h) = self.backend.size();
        self.backend.draw_text(0, h.saturating_sub(1), status, style);
        self.backend.flush();
    }
}
