use std::sync::{Arc, Mutex};

use rope_editor::backend::Backend;
use rope_editor::buffer::Buffer;
use rope_editor::renderer::Renderer;
use rope_editor::style::{Color, Style};
use rope_editor::viewport::Viewport;

const STYLE: Style = Style { fg: Color::Reset, bg: Color::Reset, bold: false };

const WIDTH_COLS: u16 = 20;
const HEIGHT_ROWS: u16 = 3;

/// Backend de prueba: no toca stdout; graba (x, y, texto) en un
/// Arc<Mutex<_>> para poder inspeccionarlo después de que el Renderer
/// se llevó el Box<dyn Backend>.
#[derive(Clone)]
struct RecordingBackend {
    drawn: Arc<Mutex<Vec<(u16, u16, String)>>>,
}

impl RecordingBackend {
    fn new() -> Self {
        RecordingBackend { drawn: Arc::new(Mutex::new(Vec::new())) }
    }

    /// Texto (sin el padding de espacios) de la última vez que una
    /// fila fue dibujada: el registro acumula entre renders.
    fn text_of(&self, y: u16) -> String {
        self.drawn
            .lock()
            .unwrap()
            .iter()
            .rfind(|(_, ry, _)| *ry == y)
            .map(|(_, _, t)| t.trim_end().to_string())
            .unwrap_or_default()
    }
}

impl Backend for RecordingBackend {
    fn draw_text(&mut self, x: u16, y: u16, text: &str, _style: Style) {
        self.drawn.lock().unwrap().push((x, y, text.to_string()));
    }
    fn clear(&mut self) {}
    fn clear_with_bg(&mut self, _bg: Color) {}
    fn flush(&mut self) {}
    fn size(&self) -> (u16, u16) {
        (WIDTH_COLS, HEIGHT_ROWS)
    }
}

/// La ventana visible sale de UN recorrido del árbol (lines_window) y
/// contiene EXACTAMENTE las líneas pedidas, en orden, con el relleno
/// de espacios hasta el ancho completo.
#[test]
fn render_window_muestra_solo_las_lineas_visibles() {
    let mut buffer = Buffer::new();
    buffer.insert(0, "uno\ndos\ntres\ncuatro");
    let backend = RecordingBackend::new();
    let mut renderer = Renderer::new(Box::new(backend.clone()));

    let mut viewport = Viewport::new(WIDTH_COLS as usize, HEIGHT_ROWS as usize);
    viewport.scroll(1); // desde la línea 1
    viewport.resize(WIDTH_COLS as usize, 2);
    renderer.render(&buffer, &viewport, STYLE);

    assert_eq!(backend.text_of(0), "dos");
    assert_eq!(backend.text_of(1), "tres");

    // El relleno hasta el ancho completo sigue igual que antes: la
    // fila tiene 20 columnas dibujadas.
    let drawn = backend.drawn.lock().unwrap();
    assert_eq!(drawn.iter().find(|(_, y, _)| *y == 1).unwrap().2.chars().count(), 20);
}

/// El renderer consulta el buffer VIVO en cada render: una edición
/// inmediata se refleja sin estado escondido.
#[test]
fn render_refleja_ediciones_inmediatas() {
    let mut buffer = Buffer::new();
    buffer.insert(0, "uno\ndos");
    let backend = RecordingBackend::new();
    let mut renderer = Renderer::new(Box::new(backend.clone()));

    let viewport = Viewport::new(WIDTH_COLS as usize, HEIGHT_ROWS as usize);
    renderer.render(&buffer, &viewport, STYLE);
    assert_eq!(backend.text_of(0), "uno");
    assert_eq!(backend.text_of(1), "dos");

    // Editar el buffer y volver a dibujar: la fila 0 ahora es otra.
    buffer.insert(3, " X"); // "uno X\ndos"
    renderer.render(&buffer, &viewport, STYLE);
    assert_eq!(backend.text_of(0), "uno X");

    // Quitar " X\n" completo: quedan las dos palabras pegadas en
    // UNA sola línea; las filas 1 y 2 vuelven a estar vacías.
    buffer.delete(3, 6); // "unodos"
    renderer.render(&buffer, &viewport, STYLE);
    assert_eq!(backend.text_of(0), "unodos");
    assert_eq!(backend.text_of(1), "");
    assert_eq!(backend.text_of(2), "");
}

/// Las filas más allá del final del documento se dibujan vacías
/// (con el padding que asegura el fondo pintado), tal como lo hacía
/// la versión con to_string completo.
#[test]
fn filas_mas_alla_del_final_quedan_vacias() {
    let mut buffer = Buffer::new();
    buffer.insert(0, "solo una");
    let backend = RecordingBackend::new();
    let mut renderer = Renderer::new(Box::new(backend.clone()));

    let viewport = Viewport::new(WIDTH_COLS as usize, HEIGHT_ROWS as usize);
    renderer.render(&buffer, &viewport, STYLE);

    assert_eq!(backend.text_of(0), "solo una");
    assert_eq!(backend.text_of(1), "");
    assert_eq!(backend.text_of(2), "");
    // Las filas vacías también llegan dibujadas con el padding completo.
    assert_eq!(backend.drawn.lock().unwrap().len(), 3);
}
