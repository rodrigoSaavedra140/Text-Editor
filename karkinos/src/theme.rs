use eframe::egui;

/// S5 (Etapa 2): Modos de tema visual soportados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    Monokai,
    Dracula,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 4] = [
        ThemeMode::Dark,
        ThemeMode::Light,
        ThemeMode::Monokai,
        ThemeMode::Dracula,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "Oscuro",
            ThemeMode::Light => "Claro",
            ThemeMode::Monokai => "Monokai",
            ThemeMode::Dracula => "Dracula",
        }
    }

    pub fn to_storage_str(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
            ThemeMode::Monokai => "monokai",
            ThemeMode::Dracula => "dracula",
        }
    }

    pub fn from_storage_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "light" => ThemeMode::Light,
            "monokai" => ThemeMode::Monokai,
            "dracula" => ThemeMode::Dracula,
            _ => ThemeMode::Dark,
        }
    }

    pub fn theme(&self) -> Theme {
        match self {
            ThemeMode::Dark => Theme {
                mode: ThemeMode::Dark,
                bg: egui::Color32::from_rgb(10, 10, 10),
                text: egui::Color32::from_gray(225),
                gutter_bg: egui::Color32::from_white_alpha(5),
                gutter_text: egui::Color32::from_gray(110),
                selection: egui::Color32::from_rgb(60, 90, 150),
                active_line: egui::Color32::from_white_alpha(12),
                caret: egui::Color32::WHITE,
                title_bar_bg: egui::Color32::from_rgb(18, 18, 18),
                panel_bg: egui::Color32::from_rgb(22, 22, 22),
                syntax_keyword: egui::Color32::from_rgb(197, 134, 192),
                syntax_type: egui::Color32::from_rgb(78, 201, 176),
                syntax_string: egui::Color32::from_rgb(206, 145, 120),
                syntax_comment: egui::Color32::from_rgb(106, 153, 85),
                syntax_number: egui::Color32::from_rgb(181, 206, 168),
            },
            ThemeMode::Light => Theme {
                mode: ThemeMode::Light,
                bg: egui::Color32::from_rgb(250, 250, 250),
                text: egui::Color32::from_rgb(30, 30, 30),
                gutter_bg: egui::Color32::from_rgb(240, 240, 240),
                gutter_text: egui::Color32::from_rgb(130, 130, 130),
                selection: egui::Color32::from_rgb(180, 215, 255),
                active_line: egui::Color32::from_rgb(238, 238, 238),
                caret: egui::Color32::from_rgb(20, 20, 20),
                title_bar_bg: egui::Color32::from_rgb(230, 230, 230),
                panel_bg: egui::Color32::from_rgb(240, 240, 240),
                syntax_keyword: egui::Color32::from_rgb(175, 0, 219),
                syntax_type: egui::Color32::from_rgb(38, 127, 153),
                syntax_string: egui::Color32::from_rgb(163, 21, 21),
                syntax_comment: egui::Color32::from_rgb(0, 128, 0),
                syntax_number: egui::Color32::from_rgb(9, 134, 88),
            },
            ThemeMode::Monokai => Theme {
                mode: ThemeMode::Monokai,
                bg: egui::Color32::from_rgb(39, 40, 34),
                text: egui::Color32::from_rgb(248, 248, 242),
                gutter_bg: egui::Color32::from_rgb(30, 31, 26),
                gutter_text: egui::Color32::from_rgb(117, 113, 94),
                selection: egui::Color32::from_rgb(73, 72, 62),
                active_line: egui::Color32::from_rgb(62, 61, 50),
                caret: egui::Color32::from_rgb(248, 248, 240),
                title_bar_bg: egui::Color32::from_rgb(26, 27, 22),
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
                text: egui::Color32::from_rgb(248, 248, 242),
                gutter_bg: egui::Color32::from_rgb(33, 34, 44),
                gutter_text: egui::Color32::from_rgb(98, 114, 164),
                selection: egui::Color32::from_rgb(68, 71, 90),
                active_line: egui::Color32::from_rgb(52, 55, 70),
                caret: egui::Color32::from_rgb(255, 121, 198),
                title_bar_bg: egui::Color32::from_rgb(25, 26, 33),
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

/// S5 (Etapa 2): Paleta de colores concreta para un tema.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    #[allow(dead_code)]
    pub mode: ThemeMode,
    pub bg: egui::Color32,
    pub text: egui::Color32,
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
}
