use eframe::egui;

/// S5 (Etapa 2): Modos de tema visual soportados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Monokai,
    Dracula,
    OneDark,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 4] = [
        ThemeMode::Dark,
        ThemeMode::Monokai,
        ThemeMode::Dracula,
        ThemeMode::OneDark,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark",
            ThemeMode::Monokai => "Monokai",
            ThemeMode::Dracula => "Dracula",
            ThemeMode::OneDark => "One Dark",
        }
    }

    pub fn to_storage_str(self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Monokai => "monokai",
            ThemeMode::Dracula => "dracula",
            ThemeMode::OneDark => "onedark",
        }
    }

    pub fn from_storage_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "monokai" => ThemeMode::Monokai,
            "dracula" => ThemeMode::Dracula,
            "onedark" | "one_dark" | "one-dark" => ThemeMode::OneDark,
            _ => ThemeMode::Dark,
        }
    }

    pub fn theme(&self) -> Theme {
        match self {
            // UI-1 (v0.2.19): "Graphite Modern" — jerarquía de
            // elevación en 3 niveles (canvas → superficie → elevado),
            // bordes sutiles de separación y acento cangrejo.
            ThemeMode::Dark => Theme {
                mode: ThemeMode::Dark,
                bg: egui::Color32::from_rgb(14, 17, 23),
                surface_bg: egui::Color32::from_rgb(22, 27, 34),
                elevated_bg: egui::Color32::from_rgb(31, 36, 44),
                border_subtle: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                border_focus: egui::Color32::from_rgb(222, 100, 50),
                badge_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                text: egui::Color32::from_rgb(230, 237, 243),
                text_muted: egui::Color32::from_rgb(139, 148, 158),
                accent: egui::Color32::from_rgb(222, 100, 50),
                gutter_bg: egui::Color32::from_rgb(22, 27, 34),
                gutter_text: egui::Color32::from_rgb(139, 148, 158),
                selection: egui::Color32::from_rgb(60, 90, 150),
                active_line: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6),
                caret: egui::Color32::WHITE,
                title_bar_bg: egui::Color32::from_rgb(22, 27, 34),
                panel_bg: egui::Color32::from_rgb(22, 27, 34),
                syntax_keyword: egui::Color32::from_rgb(197, 134, 192),
                syntax_type: egui::Color32::from_rgb(78, 201, 176),
                syntax_string: egui::Color32::from_rgb(206, 145, 120),
                syntax_comment: egui::Color32::from_rgb(106, 153, 85),
                syntax_number: egui::Color32::from_rgb(181, 206, 168),
            },
            ThemeMode::Monokai => Theme {
                mode: ThemeMode::Monokai,
                bg: egui::Color32::from_rgb(39, 40, 34),
                surface_bg: egui::Color32::from_rgb(33, 34, 28),
                elevated_bg: egui::Color32::from_rgb(44, 45, 38),
                border_subtle: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                border_focus: egui::Color32::from_rgb(249, 38, 114),
                badge_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                text: egui::Color32::from_rgb(248, 248, 242),
                // S2 (v0.2.81): 4.57:1 sobre elevated_bg (antes 2.83).
                text_muted: egui::Color32::from_rgb(154, 148, 124),
                accent: egui::Color32::from_rgb(249, 38, 114),
                gutter_bg: egui::Color32::from_rgb(28, 29, 23),
                gutter_text: egui::Color32::from_rgb(154, 148, 124),
                selection: egui::Color32::from_rgb(73, 72, 62),
                active_line: egui::Color32::from_rgb(62, 61, 50),
                caret: egui::Color32::from_rgb(248, 248, 240),
                title_bar_bg: egui::Color32::from_rgb(33, 34, 28),
                panel_bg: egui::Color32::from_rgb(33, 34, 28),
                syntax_keyword: egui::Color32::from_rgb(249, 38, 114),
                syntax_type: egui::Color32::from_rgb(102, 217, 239),
                syntax_string: egui::Color32::from_rgb(230, 219, 116),
                syntax_comment: egui::Color32::from_rgb(154, 148, 124),
                syntax_number: egui::Color32::from_rgb(174, 129, 255),
            },
            ThemeMode::Dracula => Theme {
                mode: ThemeMode::Dracula,
                bg: egui::Color32::from_rgb(40, 42, 54),
                surface_bg: egui::Color32::from_rgb(33, 34, 44),
                elevated_bg: egui::Color32::from_rgb(50, 52, 66),
                border_subtle: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                border_focus: egui::Color32::from_rgb(255, 121, 198),
                badge_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                text: egui::Color32::from_rgb(248, 248, 242),
                // S2 (v0.2.81): 4.60:1 sobre elevated_bg (antes 2.62).
                text_muted: egui::Color32::from_rgb(134, 156, 224),
                accent: egui::Color32::from_rgb(255, 121, 198),
                gutter_bg: egui::Color32::from_rgb(29, 30, 39),
                gutter_text: egui::Color32::from_rgb(134, 156, 224),
                selection: egui::Color32::from_rgb(68, 71, 90),
                active_line: egui::Color32::from_rgb(52, 55, 70),
                caret: egui::Color32::from_rgb(255, 121, 198),
                title_bar_bg: egui::Color32::from_rgb(33, 34, 44),
                panel_bg: egui::Color32::from_rgb(33, 34, 44),
                syntax_keyword: egui::Color32::from_rgb(255, 121, 198),
                syntax_type: egui::Color32::from_rgb(139, 233, 253),
                syntax_string: egui::Color32::from_rgb(241, 250, 140),
                syntax_comment: egui::Color32::from_rgb(134, 156, 224),
                syntax_number: egui::Color32::from_rgb(189, 147, 249),
            },
            // Paleta auténtica de Atom One Dark (#282c34, #abb2bf).
            ThemeMode::OneDark => Theme {
                mode: ThemeMode::OneDark,
                bg: egui::Color32::from_rgb(40, 44, 52),                    // #282c34
                surface_bg: egui::Color32::from_rgb(33, 37, 43),           // #21252b
                elevated_bg: egui::Color32::from_rgb(47, 52, 64),          // #2f3440
                border_subtle: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                border_focus: egui::Color32::from_rgb(97, 175, 239),       // #61afef
                badge_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                text: egui::Color32::from_rgb(171, 178, 191),              // #abb2bf
                // S2 (v0.2.81): 4.62:1 sobre elevated_bg (antes 2.06).
                text_muted: egui::Color32::from_rgb(147, 158, 179),        // #939eb3
                accent: egui::Color32::from_rgb(97, 175, 239),             // #61afef
                gutter_bg: egui::Color32::from_rgb(33, 37, 43),            // #21252b
                gutter_text: egui::Color32::from_rgb(147, 158, 179),       // #939eb3
                selection: egui::Color32::from_rgb(62, 68, 81),            // #3e4451
                active_line: egui::Color32::from_rgb(44, 49, 58),          // #2c313a
                caret: egui::Color32::from_rgb(82, 139, 255),              // #528bff
                title_bar_bg: egui::Color32::from_rgb(33, 37, 43),         // #21252b
                panel_bg: egui::Color32::from_rgb(33, 37, 43),             // #21252b
                syntax_keyword: egui::Color32::from_rgb(198, 120, 221),    // #c678dd
                syntax_type: egui::Color32::from_rgb(229, 192, 123),       // #e5c07b
                syntax_string: egui::Color32::from_rgb(152, 195, 121),     // #98c379
                syntax_comment: egui::Color32::from_rgb(147, 158, 179),    // #939eb3
                syntax_number: egui::Color32::from_rgb(209, 154, 102),     // #d19a66
            },
        }
    }
}

/// S5 (Etapa 2) / UI-1 (v0.2.19): Paleta de colores concreta
/// para un tema, con jerarquía de elevación:
/// - `bg`: canvas (fondo del editor),
/// - `surface_bg`: paneles, barras y tabs (nivel 2),
/// - `elevated_bg`: modales flotantes, menús y dropdowns (nivel 3),
/// - `border_subtle`: separación de 1px entre superficies,
/// - `border_focus`: borde de inputs con foco y pestaña activa,
/// - `badge_bg`: chips de estado y hints de teclado.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    #[allow(dead_code)]
    pub mode: ThemeMode,
    pub bg: egui::Color32,
    // UI-1: tokens nuevos; cada `allow` que queda se retira al cablear su
    // token en la unidad consumidora (border_focus: foco de inputs, T-UI5).
    pub surface_bg: egui::Color32,
    pub elevated_bg: egui::Color32,
    pub border_subtle: egui::Color32,
    pub border_focus: egui::Color32,
    pub badge_bg: egui::Color32,
    pub text: egui::Color32,
    pub text_muted: egui::Color32,
    pub accent: egui::Color32,
    pub gutter_bg: egui::Color32,
    pub gutter_text: egui::Color32,
    pub selection: egui::Color32,
    pub active_line: egui::Color32,
    pub caret: egui::Color32,
    pub title_bar_bg: egui::Color32,
    #[allow(dead_code)]
    pub panel_bg: egui::Color32,
    pub syntax_keyword: egui::Color32,
    pub syntax_type: egui::Color32,
    pub syntax_string: egui::Color32,
    pub syntax_comment: egui::Color32,
    pub syntax_number: egui::Color32,
}

/// UI-3 (v0.2.21): marco de las tarjetas flotantes (Quick Open, Code Actions
/// y Rename). Sin barra de título: fondo de elevación, borde sutil, esquinas
/// de 10 px y sombra de elevación con el desfase de la UI-1.
pub fn modal_card_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::none()
        .fill(theme.elevated_bg)
        .rounding(egui::Rounding::same(10.0))
        .stroke(egui::Stroke::new(1.0_f32, theme.border_subtle))
        .inner_margin(egui::Margin::same(12.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(160),
        })
}

/// Interpola linealmente entre dos colores en el espacio sRGB/RGBA con alfa pre-multiplicado.
pub fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgba_premultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t).round() as u8,
    )
}

/// S2 (v0.2.81): luminancia relativa WCAG 2.x de un color, con la
/// linealización sRGB que pide la norma. Ignora el alfa.
/// Hoy solo la usan los tests de contraste (y las mediciones del Log).
#[allow(dead_code)]
fn relative_luminance(color: egui::Color32) -> f32 {
    fn channel(value: u8) -> f32 {
        let v = value as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// S2 (v0.2.81): relación de contraste WCAG 2.x entre dos colores
/// (de 1.0 a 21.0). Es simétrica e ignora el alfa. La usan los tests
/// de legibilidad y las mediciones de estilo; el runtime no la necesita.
#[allow(dead_code)]
pub fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (claro, oscuro) = if la > lb { (la, lb) } else { (lb, la) };
    (claro + 0.05) / (oscuro + 0.05)
}

/// S2 (v0.2.81): copia la paleta del tema a los `egui::Visuals` para que
/// menús, popups, ventanas, tooltips, botones y labels que no fijan su
/// color usen los mismos colores que el resto de la app. Antes egui
/// seguía su propia paleta (clara si el sistema estaba en modo claro) y
/// el texto quedaba ilegible sobre los fondos oscuros de Karkinos.
pub fn apply_visuals(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();

    // Superficies base: paneles y ventanas.
    visuals.panel_fill = theme.surface_bg;
    visuals.window_fill = theme.elevated_bg;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, theme.border_subtle);
    // Fondo de los `TextEdit` y de otros widgets que necesitan
    // distinguirse del panel.
    visuals.extreme_bg_color = theme.bg;
    visuals.faint_bg_color = theme.badge_bg;
    visuals.code_bg_color = theme.badge_bg;
    // El color de texto se resuelve por widget; nunca se fuerza uno global.
    visuals.override_text_color = None;
    visuals.selection.bg_fill = theme.selection;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, theme.text);
    visuals.hyperlink_color = theme.accent;

    // Escala de superficies para los estados de los widgets: en reposo
    // usan la elevación y en hover/activo se aclaran un poco hacia el
    // texto del tema.
    let hovered_bg = lerp_color(theme.elevated_bg, theme.text, 0.08);
    let active_bg = lerp_color(theme.elevated_bg, theme.text, 0.14);
    let borde = egui::Stroke::new(1.0_f32, theme.border_subtle);
    let borde_foco = egui::Stroke::new(1.0_f32, theme.border_focus);
    let texto = egui::Stroke::new(1.0_f32, theme.text);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = theme.surface_bg;
    widgets.noninteractive.weak_bg_fill = theme.surface_bg;
    widgets.noninteractive.bg_stroke = borde;
    widgets.noninteractive.fg_stroke = texto;
    widgets.noninteractive.rounding = egui::Rounding::same(6.0);

    widgets.inactive.bg_fill = theme.elevated_bg;
    widgets.inactive.weak_bg_fill = theme.elevated_bg;
    widgets.inactive.bg_stroke = borde;
    widgets.inactive.fg_stroke = texto;
    widgets.inactive.rounding = egui::Rounding::same(6.0);

    widgets.hovered.bg_fill = hovered_bg;
    widgets.hovered.weak_bg_fill = hovered_bg;
    widgets.hovered.bg_stroke = borde_foco;
    widgets.hovered.fg_stroke = texto;
    widgets.hovered.rounding = egui::Rounding::same(6.0);

    widgets.active.bg_fill = active_bg;
    widgets.active.weak_bg_fill = active_bg;
    widgets.active.bg_stroke = borde_foco;
    widgets.active.fg_stroke = texto;
    widgets.active.rounding = egui::Rounding::same(6.0);

    widgets.open.bg_fill = active_bg;
    widgets.open.weak_bg_fill = active_bg;
    widgets.open.bg_stroke = borde;
    widgets.open.fg_stroke = texto;
    widgets.open.rounding = egui::Rounding::same(6.0);

    // UI-1 (v0.2.19): se conserva el sistema de diseño que antes fijaba
    // `main.rs` a mano (esquinas de 6/10 px y sombra de elevación).
    visuals.window_rounding = egui::Rounding::same(10.0);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 8.0),
        blur: 20.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(150),
    };

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_color_interpolates_properly() {
        let c1 = egui::Color32::from_rgba_premultiplied(100, 50, 200, 100);
        let c2 = egui::Color32::from_rgba_premultiplied(200, 150, 100, 200);
        assert_eq!(lerp_color(c1, c2, 0.0), c1);
        assert_eq!(lerp_color(c1, c2, 1.0), c2);
        assert_eq!(
            lerp_color(c1, c2, 0.5),
            egui::Color32::from_rgba_premultiplied(150, 100, 150, 150)
        );
        assert_eq!(lerp_color(c1, c2, -1.0), c1);
        assert_eq!(lerp_color(c1, c2, 2.0), c2);
    }

    #[test]
    fn theme_mode_roundtrip_storage() {
        for mode in ThemeMode::ALL {
            let s = mode.to_storage_str();
            assert_eq!(ThemeMode::from_storage_str(s), mode);
        }
        // Fallback defensivo ante basura en storage
        assert_eq!(ThemeMode::from_storage_str("desconocido"), ThemeMode::Dark);
    }

    /// S2 (v0.2.81): la fórmula de contraste WCAG 2.x debe dar 21:1
    /// entre negro y blanco, 1:1 con el mismo color y ser simétrica.
    #[test]
    fn contrast_ratio_coincide_con_wcag() {
        let white = egui::Color32::WHITE;
        let black = egui::Color32::BLACK;
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.05);
        assert!(
            (contrast_ratio(white, black) - 21.0).abs() < 0.05,
            "la relación es simétrica"
        );

        let mismo = egui::Color32::from_rgb(120, 80, 200);
        assert!((contrast_ratio(mismo, mismo) - 1.0).abs() < f32::EPSILON);

        let a = egui::Color32::from_rgb(30, 40, 50);
        let b = egui::Color32::from_rgb(200, 210, 220);
        assert!(
            (contrast_ratio(a, b) - contrast_ratio(b, a)).abs() < f32::EPSILON,
            "invertir los colores no cambia la relación"
        );
    }

    /// S2 (v0.2.81): en los cuatro temas el texto principal y el
    /// secundario se leen sobre las tres superficies de la paleta.
    #[test]
    fn texto_legible_en_todos_los_temas() {
        for mode in ThemeMode::ALL {
            let theme = mode.theme();
            for (nombre, color) in [("text", theme.text), ("text_muted", theme.text_muted)] {
                for (fondo, bg) in [
                    ("bg", theme.bg),
                    ("surface_bg", theme.surface_bg),
                    ("elevated_bg", theme.elevated_bg),
                ] {
                    let ratio = contrast_ratio(color, bg);
                    assert!(
                        ratio >= 4.5,
                        "{:?}: {nombre} sobre {fondo} mide {ratio:.2}, mínimo 4.5",
                        mode
                    );
                }
            }
        }
    }

    /// S2 (v0.2.81): `apply_visuals` copia los tokens del tema a los
    /// `Visuals` que usan menús, popups, ventanas y widgets de egui.
    #[test]
    fn apply_visuals_usa_los_tokens_del_tema() {
        let ctx = egui::Context::default();
        for mode in ThemeMode::ALL {
            let theme = mode.theme();
            apply_visuals(&ctx, &theme);

            let style = ctx.style();
            let visuals = &style.visuals;
            assert_eq!(visuals.window_fill, theme.elevated_bg, "{:?}: window_fill", mode);
            assert_eq!(visuals.panel_fill, theme.surface_bg, "{:?}: panel_fill", mode);
            assert_eq!(
                visuals.widgets.noninteractive.fg_stroke.color, theme.text,
                "{:?}: el texto no interactivo usa el texto del tema",
                mode
            );
            assert!(visuals.dark_mode, "{:?}: los temas son oscuros", mode);
            let ratio = contrast_ratio(
                visuals.widgets.inactive.fg_stroke.color,
                visuals.widgets.inactive.bg_fill,
            );
            assert!(
                ratio >= 4.5,
                "{:?}: el texto de widgets inactivos mide {ratio:.2}, mínimo 4.5",
                mode
            );
        }
    }

    /// UI-1: los 3 temas exponen la jerarquía de elevación completa
    /// (canvas / superficie / elevado + bordes y badges).
    #[test]
    fn theme_exposes_elevation_tokens_para_los_3_modos() {
        for mode in ThemeMode::ALL {
            let t = mode.theme();
            assert_ne!(t.bg, t.surface_bg, "{:?}: superficie != canvas", mode);
            assert_ne!(t.surface_bg, t.elevated_bg, "{:?}: elevado != superficie", mode);
            assert_ne!(t.text, t.text_muted, "{:?}: texto != texto secundario", mode);
            assert_ne!(t.accent, t.bg, "{:?}: el acento debe ser visible sobre el canvas", mode);
            assert_eq!(
                t.border_focus, t.accent,
                "{:?}: el focus usa el acento del tema",
                mode
            );
            assert!(t.border_subtle.a() > 0, "{:?}: borde sutil visible", mode);
            assert!(t.badge_bg.a() > 0, "{:?}: badge visible", mode);
        }
    }

    /// UI-1: la paleta Dark ("Graphite Modern") usa los valores exactos
    /// del diseño (#0e1117 canvas, #161b22 superficie, #1f242c elevado,
    /// acento cangrejo #de6432).
    #[test]
    fn dark_usa_la_paleta_graphite_del_diseno() {
        let t = ThemeMode::Dark.theme();
        assert_eq!(t.bg, egui::Color32::from_rgb(14, 17, 23));
        assert_eq!(t.surface_bg, egui::Color32::from_rgb(22, 27, 34));
        assert_eq!(t.elevated_bg, egui::Color32::from_rgb(31, 36, 44));
        assert_eq!(t.accent, egui::Color32::from_rgb(222, 100, 50));
        assert_eq!(t.border_focus, egui::Color32::from_rgb(222, 100, 50));
        assert_eq!(t.text, egui::Color32::from_rgb(230, 237, 243));
        assert_eq!(t.text_muted, egui::Color32::from_rgb(139, 148, 158));
        assert_eq!(t.active_line, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6));
        assert_eq!(
            t.border_subtle,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        );
    }

    /// One Dark: la paleta usa los valores auténticos de Atom One Dark.
    #[test]
    fn one_dark_usa_la_paleta_de_atom() {
        let t = ThemeMode::OneDark.theme();
        assert_eq!(t.mode, ThemeMode::OneDark);
        assert_eq!(t.bg, egui::Color32::from_rgb(40, 44, 52));
        assert_eq!(t.surface_bg, egui::Color32::from_rgb(33, 37, 43));
        assert_eq!(t.elevated_bg, egui::Color32::from_rgb(47, 52, 64));
        assert_eq!(
            t.border_subtle,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        );
        assert_eq!(t.border_focus, egui::Color32::from_rgb(97, 175, 239));
        assert_eq!(
            t.badge_bg,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14)
        );
        assert_eq!(t.text, egui::Color32::from_rgb(171, 178, 191));
        assert_eq!(t.text_muted, egui::Color32::from_rgb(147, 158, 179));
        assert_eq!(t.accent, egui::Color32::from_rgb(97, 175, 239));
        assert_eq!(t.gutter_bg, egui::Color32::from_rgb(33, 37, 43));
        assert_eq!(t.gutter_text, egui::Color32::from_rgb(147, 158, 179));
        assert_eq!(t.selection, egui::Color32::from_rgb(62, 68, 81));
        assert_eq!(t.active_line, egui::Color32::from_rgb(44, 49, 58));
        assert_eq!(t.caret, egui::Color32::from_rgb(82, 139, 255));
        assert_eq!(t.title_bar_bg, egui::Color32::from_rgb(33, 37, 43));
        assert_eq!(t.panel_bg, egui::Color32::from_rgb(33, 37, 43));
        assert_eq!(t.syntax_keyword, egui::Color32::from_rgb(198, 120, 221));
        assert_eq!(t.syntax_type, egui::Color32::from_rgb(229, 192, 123));
        assert_eq!(t.syntax_string, egui::Color32::from_rgb(152, 195, 121));
        assert_eq!(t.syntax_comment, egui::Color32::from_rgb(147, 158, 179));
        assert_eq!(t.syntax_number, egui::Color32::from_rgb(209, 154, 102));
    }

    #[test]
    fn one_dark_from_storage_acepta_alias() {
        for alias in ["onedark", "one_dark", "one-dark", "OneDark"] {
            assert_eq!(
                ThemeMode::from_storage_str(alias),
                ThemeMode::OneDark,
                "alias {alias}"
            );
        }
        assert_eq!(ThemeMode::OneDark.to_storage_str(), "onedark");
        assert_eq!(ThemeMode::OneDark.name(), "One Dark");
    }

    /// UI-3 (v0.2.21): las tarjetas flotantes salen de los tokens de
    /// elevación, con esquinas de 10 px, margen de 12 px y sombra de 24 px.
    #[test]
    fn modal_card_frame_usa_tokens_y_elevacion() {
        let theme = ThemeMode::Dark.theme();
        let frame = modal_card_frame(&theme);
        assert_eq!(frame.fill, theme.elevated_bg);
        assert_eq!(frame.rounding, egui::Rounding::same(10.0));
        assert_eq!(frame.stroke.color, theme.border_subtle);
        assert_eq!(frame.inner_margin, egui::Margin::same(12.0));
        assert_eq!(frame.shadow.blur, 24.0);
        assert_eq!(frame.shadow.color, egui::Color32::from_black_alpha(160));
    }
}
