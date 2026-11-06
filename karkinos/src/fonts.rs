//! Carga de fuentes de la aplicación.
//!
//! La instalación vive en un módulo propio para que `main.rs` y los tests
//! tipografíen con la MISMA fuente. Sin esto, un test de layout correría
//! con la fuente de relleno de egui y no mediría el avance real de Geist
//! Mono, que es justo lo que el caret necesita seguir.

use eframe::egui;

/// Instala Geist Mono como familia monoespaciada principal (editor,
/// terminal, logs). Es idempotente: llamarla dos veces deja el mismo
/// resultado, porque `set_fonts` reemplaza la definición completa.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "GeistMono-Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
    );
    fonts.font_data.insert(
        "GeistMono-Bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/GeistMono-Bold.ttf")),
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "GeistMono-Regular".to_owned());
    ctx.set_fonts(fonts);
}
