use std::time::Duration;

use crossterm::event::{self, Event, KeyCode as CtKeyCode, KeyEventKind, KeyModifiers};

use crate::editor::EditorMode;

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Clone, Debug)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Arrow(Direction),
}

#[derive(Clone, Debug)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

#[derive(Clone, Debug)]
pub enum Command {
    InsertChar(char),
    InsertNewline,
    DeleteCharBackward,
    DeleteCharForward,
    MoveCursor(Direction, usize),
    Save,
    Quit,
    Undo,
    Redo,
    Copy,
    Paste,
    SwitchMode(EditorMode),
    // Comandos del modo ":" (estilo vim) para escribir un nombre de
    // archivo y ejecutar la orden, ej: ":w archivo.txt"
    CommandChar(char),
    CommandBackspace,
    CommandExecute,
    None,
}

pub struct InputHandler;

impl InputHandler {
    pub fn new() -> Self {
        InputHandler
    }

    pub fn read_key(&self) -> Option<KeyEvent> {
        if event::poll(Duration::from_millis(100)).ok()? {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press {
                    return None;
                }
                let code = match k.code {
                    CtKeyCode::Char(c) => KeyCode::Char(c),
                    CtKeyCode::Enter => KeyCode::Enter,
                    CtKeyCode::Esc => KeyCode::Esc,
                    CtKeyCode::Backspace => KeyCode::Backspace,
                    CtKeyCode::Up => KeyCode::Arrow(Direction::Up),
                    CtKeyCode::Down => KeyCode::Arrow(Direction::Down),
                    CtKeyCode::Left => KeyCode::Arrow(Direction::Left),
                    CtKeyCode::Right => KeyCode::Arrow(Direction::Right),
                    _ => return None,
                };
                let modifiers = Modifiers {
                    ctrl: k.modifiers.contains(KeyModifiers::CONTROL),
                    alt: k.modifiers.contains(KeyModifiers::ALT),
                    shift: k.modifiers.contains(KeyModifiers::SHIFT),
                };
                return Some(KeyEvent { code, modifiers });
            }
        }
        None
    }

    // El UML no pasaba el modo como parámetro, pero el mapeo de teclas
    // depende necesariamente del modo actual (Normal vs Insert no
    // interpretan las mismas teclas igual) — ajuste práctico necesario.
    pub fn map_key(&self, event: KeyEvent, mode: EditorMode) -> Command {
        if event.modifiers.ctrl {
            match event.code {
                KeyCode::Char('s') => return Command::Save,
                KeyCode::Char('q') => return Command::Quit,
                KeyCode::Char('r') => return Command::Redo,
                _ => {}
            }
        }

        match mode {
            EditorMode::Normal => match event.code {
                KeyCode::Char('i') => Command::SwitchMode(EditorMode::Insert),
                KeyCode::Char(':') => Command::SwitchMode(EditorMode::Command),
                KeyCode::Char('u') => Command::Undo,
                KeyCode::Char('x') => Command::DeleteCharForward,
                KeyCode::Char('y') => Command::Copy,
                KeyCode::Char('p') => Command::Paste,
                KeyCode::Char('q') => Command::Quit,
                KeyCode::Arrow(d) => Command::MoveCursor(d, 1),
                _ => Command::None,
            },
            EditorMode::Insert => match event.code {
                KeyCode::Esc => Command::SwitchMode(EditorMode::Normal),
                KeyCode::Char(c) => Command::InsertChar(c),
                KeyCode::Enter => Command::InsertNewline,
                KeyCode::Backspace => Command::DeleteCharBackward,
                KeyCode::Arrow(d) => Command::MoveCursor(d, 1),
                _ => Command::None,
            },
            EditorMode::Command => match event.code {
                KeyCode::Esc => Command::SwitchMode(EditorMode::Normal),
                KeyCode::Enter => Command::CommandExecute,
                KeyCode::Backspace => Command::CommandBackspace,
                KeyCode::Char(c) => Command::CommandChar(c),
                _ => Command::None,
            },
            // Visual queda como trabajo futuro (ver nota en el diagrama).
            EditorMode::Visual => Command::None,
        }
    }
}