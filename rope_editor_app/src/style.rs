#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    White,
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
}

impl Theme {
    pub fn default() -> Self {
        Theme {
            normal: Style::default(),
            cursor: Style { fg: Color::Black, bg: Color::White, bold: false },
            status_bar: Style { fg: Color::White, bg: Color::Blue, bold: true },
        }
    }
}
