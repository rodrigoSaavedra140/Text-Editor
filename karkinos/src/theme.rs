use eframe::egui;

/// S5 (Etapa 2): Modos de tema visual soportados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Monokai,
    Dracula,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [
        ThemeMode::Dark,
        ThemeMode::Monokai,
        ThemeMode::Dracula,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark",
            ThemeMode::Monokai => "Monokai",
            ThemeMode::Dracula => "Dracula",
        }
    }

    pub fn to_storage_str(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Monokai => "monokai",
            ThemeMode::Dracula => "dracula",
        }
    }

    pub fn from_storage_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "monokai" => ThemeMode::Monokai,
            "dracula" => ThemeMode::Dracula,
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
                text_muted: egui::Color32::from_rgb(117, 113, 94),
                accent: egui::Color32::from_rgb(249, 38, 114),
                gutter_bg: egui::Color32::from_rgb(28, 29, 23),
                gutter_text: egui::Color32::from_rgb(117, 113, 94),
                selection: egui::Color32::from_rgb(73, 72, 62),
                active_line: egui::Color32::from_rgb(62, 61, 50),
                caret: egui::Color32::from_rgb(248, 248, 240),
                title_bar_bg: egui::Color32::from_rgb(33, 34, 28),
                panel_bg: egui::Color32::from_rgb(33, 34, 28),
                syntax_keyword: egui::Color32::from_rgb(249, 38, 114),
                syntax_type: egui::Color32::from_rgb(102, 217, 239),
                syntax_string: egui::Color32::from_rgb(230, 219, 116),
                syntax_comment: egui::Color32::from_rgb(117, 113, 94),
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
                text_muted: egui::Color32::from_rgb(98, 114, 164),
                accent: egui::Color32::from_rgb(255, 121, 198),
                gutter_bg: egui::Color32::from_rgb(29, 30, 39),
                gutter_text: egui::Color32::from_rgb(98, 114, 164),
                selection: egui::Color32::from_rgb(68, 71, 90),
                active_line: egui::Color32::from_rgb(52, 55, 70),
                caret: egui::Color32::from_rgb(255, 121, 198),
                title_bar_bg: egui::Color32::from_rgb(33, 34, 44),
                panel_bg: egui::Color32::from_rgb(33, 34, 44),
                syntax_keyword: egui::Color32::from_rgb(255, 121, 198),
                syntax_type: egui::Color32::from_rgb(139, 233, 253),
                syntax_string: egui::Color32::from_rgb(241, 250, 140),
                syntax_comment: egui::Color32::from_rgb(98, 114, 164),
                syntax_number: egui::Color32::from_rgb(189, 147, 249),
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
    // token en la unidad consumidora (elevated_bg y border_focus: T-UI3).
    pub surface_bg: egui::Color32,
    #[allow(dead_code)]
    pub elevated_bg: egui::Color32,
    pub border_subtle: egui::Color32,
    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_mode_roundtrip_storage() {
        for mode in ThemeMode::ALL {
            let s = mode.to_storage_str();
            assert_eq!(ThemeMode::from_storage_str(s), mode);
        }
        // Fallback defensivo ante basura en storage
        assert_eq!(ThemeMode::from_storage_str("desconocido"), ThemeMode::Dark);
    }

    #[test]
    fn theme_palettes_valid_contrasts() {
        for mode in ThemeMode::ALL {
            let theme = mode.theme();
            assert_ne!(theme.bg, theme.text, "el fondo y el texto deben contrastar");
            assert_ne!(theme.bg, theme.gutter_text);
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
}
