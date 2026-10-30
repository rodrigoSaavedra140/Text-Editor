mod app;
mod editor_widget;
mod theme;
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
    eframe::run_native("Karkinos", options, Box::new(|cc| {
        // S10: el zoom elegido la sesión anterior vuela al arrancar
        // desde el Storage de la app (app-storage.json). Sin Storage
        // (o valor corrupto) queda el default de 14 px.
        let mut app = EditorApp::default();
        if let Some(storage) = &cc.storage {
            if let Some(size) = storage.get_string("editor_font_size") {
                if let Some(parsed) = crate::app::font_size_from_string(&size) {
                    app.font_size = parsed;
                    app.editor_state.set_font_size(parsed);
                }
            }
            if let Some(theme_str) = storage.get_string("editor_theme") {
                app.theme_mode = crate::theme::ThemeMode::from_storage_str(&theme_str);
            }
        }
        Box::new(app)
    }))
}