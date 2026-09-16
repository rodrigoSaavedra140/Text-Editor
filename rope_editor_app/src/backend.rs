use std::io::{stdout, Stdout, Write};

use crossterm::{
    cursor::MoveTo,
    execute, queue,
    style::{Attribute, Color as CtColor, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{size, Clear, ClearType},
};

use crate::style::{Color, Style};

pub trait Backend {
    fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style);
    fn clear(&mut self);
    fn flush(&mut self);
    fn size(&self) -> (u16, u16);
}

pub struct TerminalBackend {
    stdout: Stdout,
}

impl TerminalBackend {
    pub fn new() -> Self {
        TerminalBackend { stdout: stdout() }
    }
}

fn to_ct_color(c: Color) -> CtColor {
    match c {
        Color::Reset => CtColor::Reset,
        Color::Black => CtColor::Black,
        Color::Red => CtColor::Red,
        Color::Green => CtColor::Green,
        Color::Yellow => CtColor::Yellow,
        Color::Blue => CtColor::Blue,
        Color::White => CtColor::White,
    }
}

impl Backend for TerminalBackend {
    fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style) {
        let _ = queue!(
            self.stdout,
            MoveTo(x, y),
            SetForegroundColor(to_ct_color(style.fg)),
            SetBackgroundColor(to_ct_color(style.bg)),
            SetAttribute(if style.bold { Attribute::Bold } else { Attribute::NoBold }),
        );
        let _ = write!(self.stdout, "{}", text);
        let _ = queue!(self.stdout, ResetColor);
    }

    fn clear(&mut self) {
        let _ = execute!(self.stdout, Clear(ClearType::All));
    }

    fn flush(&mut self) {
        let _ = self.stdout.flush();
    }

    fn size(&self) -> (u16, u16) {
        size().unwrap_or((80, 24))
    }
}
