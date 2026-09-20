#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    White,
    // Color RGB real (no uno de los 8 básicos de terminal), necesario
    // para tonos específicos como el gris de fondo o el salmón.
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl Style {
    pub fn default() -> Self {
        Style { fg: Color::Reset, bg: Color::Reset, bold: false }
    }
}

pub struct Theme {
    pub normal: Style,
    pub cursor: Style,
    pub status_bar: Style,
    pub help_bar: Style,
}

impl Theme {
    pub fn default() -> Self {
        Theme {
            // Fondo del área de edición: gris oscuro, parecido al
            // fondo de un chat de Claude en modo oscuro.
            normal: Style { fg: Color::Rgb(220, 220, 215), bg: Color::Rgb(38, 38, 36), bold: false },
            cursor: Style { fg: Color::Black, bg: Color::White, bold: false },
            status_bar: Style { fg: Color::White, bg: Color::Blue, bold: true },
            // Barra de keybinds: texto salmón sobre fondo gris ceniza.
            help_bar: Style { fg: Color::Rgb(250, 128, 114), bg: Color::Rgb(70, 70, 68), bold: true },
        }
    }
}