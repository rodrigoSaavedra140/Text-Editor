mod app;
mod editor_widget;
mod quick_open;
mod syntax;
mod theme;
mod title_bar;
mod ui;
mod util;

pub use karkinos_core::buffer;
pub use karkinos_core::undo;

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

    // Ícono de la barra de título / taskbar con transparencia alfa para
    // que no quede un recuadro negro en el dock o panel del sistema.
    if let Some(icon) = crate::ui::load_window_icon() {
        viewport = viewport.with_icon(icon);
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