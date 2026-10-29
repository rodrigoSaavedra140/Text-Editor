use crate::backend::Backend;
use crate::buffer::Buffer;
use crate::style::{Color, Style};
use crate::viewport::Viewport;

pub struct Renderer {
    backend: Box<dyn Backend>,
}

impl Renderer {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Renderer { backend }
    }

    /// Limpia toda la pantalla YA con `bg` como fondo — se llama UNA
    /// sola vez al arrancar (no en cada frame, para no titilar), y
    /// evita que se vea el color original de la terminal (ej. el
    /// violeta de Ubuntu) aunque sea por un instante.
    pub fn clear_screen(&mut self, bg: Color) {
        self.backend.clear_with_bg(bg);
        self.backend.flush();
    }

    pub fn render(&mut self, buffer: &Buffer, viewport: &Viewport, style: Style) {
        // Ya NO limpiamos toda la pantalla acá (eso causaba el
        // parpadeo). Cada línea se limpia sola, individualmente,
        // dentro de Backend::draw_text justo antes de escribirse.
        //
        // Una sola pasada recolecta la ventana visible; antes cada
        // línea visible era un Buffer::line() que alocaba el texto
        // completo, o sea height x to_string() por tecla.
        let visible = viewport.visible_lines();
        let window: Vec<String> = buffer.lines_window(visible.start, viewport.height());
        for row in 0..viewport.height() {
            // Filas más allá del final del documento quedan vacías,
            // igual que antes (line_num >= line_count → "").
            let line = window.get(row).map(|s| s.as_str()).unwrap_or("");
            let visible: String =
                line.chars().skip(viewport.left_col()).take(viewport.width()).collect();
            // Rellenamos con espacios hasta el ancho completo: así el
            // color de fondo cubre TODA la fila, no solo donde hay
            // texto — si no, el resto de la fila se queda con el
            // color que tuviera la terminal antes (ej. violeta).
            let padded = pad_to_width(&visible, viewport.width());
            self.backend.draw_text(0, row as u16, &padded, style);
        }
        self.backend.flush();
    }

    // El UML original pasaba `editor: &Editor`, pero eso genera un
    // conflicto de borrow en Rust (Editor tendría que prestarse a sí
    // mismo mientras presta su propio Renderer). En su lugar recibe
    // el texto y el estilo ya calculados por Editor.
    pub fn draw_status_bar(&mut self, status: &str, style: Style) {
        let (w, h) = self.backend.size();
        let padded = pad_to_width(status, w as usize);
        self.backend.draw_text(0, h.saturating_sub(1), &padded, style);
        self.backend.flush();
    }

    /// Barra de ayuda con los keybinds vigentes según el modo actual,
    /// justo arriba de la barra de estado (estilo nano/vim).
    pub fn draw_help_bar(&mut self, hints: &str, style: Style) {
        let (w, h) = self.backend.size();
        let row = h.saturating_sub(2);
        let padded = pad_to_width(hints, w as usize);
        self.backend.draw_text(0, row, &padded, style);
        self.backend.flush();
    }
}

/// Rellena `text` con espacios hasta `width` caracteres. Si `text` ya
/// es más largo que `width`, lo recorta (evita que se desborde a la
/// siguiente línea y rompa el layout).
fn pad_to_width(text: &str, width: usize) -> String {
    let char_count = text.chars().count();
    if char_count >= width {
        text.chars().take(width).collect()
    } else {
        let mut s = String::with_capacity(width);
        s.push_str(text);
        s.push_str(&" ".repeat(width - char_count));
        s
    }
}