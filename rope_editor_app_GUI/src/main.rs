mod app;
mod rope;
mod title_bar;
mod ui;
mod undo;
mod util;

use eframe::egui;

use app::EditorApp;
use util::is_wsl;

/// Logo de Karkinos, embebido en el binario en tiempo de compilación
/// (no se lee de disco en tiempo de ejecución — así el ícono y el
/// fondo minimalista funcionan siempre, sin depender de que el
/// archivo esté presente donde sea que se ejecute el programa).
pub const LOGO_BYTES: &[u8] = include_bytes!("../assets/logo.png");

fn main() -> eframe::Result<()> {
    // WSLg usa una GPU virtual cuyo contexto OpenGL a veces falla en
    // silencio (la ventana nunca llega a abrirse, sin ningún error).
    // Forzamos renderizado por software SOLO si detectamos WSL — en
    // Linux nativo con una GPU de verdad, esto solo haría que ande
    // más lento sin necesidad.
    if is_wsl() {
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([900.0, 650.0]) // tamaño de referencia antes de maximizar
        .with_maximized(true)
        .with_resizable(true)
        .with_decorations(false); // barra de título propia, ver custom_title_bar en ui.rs

    // Ícono de la barra de título / taskbar, a partir del mismo logo
    // embebido. Si por algún motivo el PNG no decodifica bien, seguimos
    // sin ícono en vez de no abrir la app.
    if let Ok(img) = image::load_from_memory(LOGO_BYTES) {
        let img = img.to_rgba8();
        let (width, height) = (img.width(), img.height());
        viewport = viewport.with_icon(egui::IconData { rgba: img.into_raw(), width, height });
    }

    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Karkinos", options, Box::new(|_cc| Box::new(EditorApp::default())))
}