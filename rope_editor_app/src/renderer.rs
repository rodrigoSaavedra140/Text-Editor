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

    /// Limpia toda la pantalla — se llama UNA sola vez al arrancar
    /// (no en cada frame, para no titilar).
    pub fn clear_screen(&mut self) {
        self.backend.clear();
        self.backend.flush();
    }

    pub fn render(&mut self, buffer: &Buffer, viewport: &Viewport) {
        // Ya NO limpiamos toda la pantalla acá (eso causaba el
        // parpadeo). Cada línea se limpia sola, individualmente,
        // dentro de Backend::draw_text justo antes de escribirse.
        let style = Style::default();
        for (row, line_num) in viewport.visible_lines().enumerate() {
            let line = if line_num < buffer.line_count() {
                buffer.line(line_num)
            } else {
                String::new() // limpia filas que quedaron sin contenido
            };
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

    /// Barra de ayuda con los keybinds vigentes según el modo actual,
    /// justo arriba de la barra de estado (estilo nano/vim).
    pub fn draw_help_bar(&mut self, hints: &str, style: Style) {
        let (_, h) = self.backend.size();
        let row = h.saturating_sub(2);
        self.backend.draw_text(0, row, hints, style);
        self.backend.flush();
    }
}