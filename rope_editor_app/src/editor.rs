use std::io;
use std::path::Path;

use crate::backend::{Backend, TerminalBackend};
use crate::buffer::Buffer;
use crate::clipboard::Clipboard;
use crate::config::Config;
use crate::input::{Command, Direction, InputHandler, KeyEvent};
use crate::renderer::Renderer;
use crate::style::Theme;
use crate::undo::{Edit, EditKind, UndoStack};
use crate::viewport::Viewport;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
    Command,
}

pub struct Editor {
    buffer: Buffer,
    viewport: Viewport,
    renderer: Renderer,
    input_handler: InputHandler,
    mode: EditorMode,
    clipboard: Clipboard,
    undo_stack: UndoStack,
    config: Config,
    running: bool,
}

impl Editor {
    pub fn new() -> Self {
        let backend: Box<dyn Backend> = Box::new(TerminalBackend::new());
        let (w, h) = backend.size();
        Editor {
            buffer: Buffer::new(),
            viewport: Viewport::new(w as usize, h.saturating_sub(1).max(1) as usize),
            renderer: Renderer::new(backend),
            input_handler: InputHandler::new(),
            mode: EditorMode::Normal,
            clipboard: Clipboard::new(),
            undo_stack: UndoStack::new(),
            config: Config::default(),
            running: true,
        }
    }

    pub fn open_file(&mut self, path: &Path) -> io::Result<()> {
        self.buffer = Buffer::from_file(path)?;
        Ok(())
    }

    pub fn set_new_file_path(&mut self, path: &Path) {
        self.buffer.set_file_path(path.to_path_buf());
    }

    pub fn save_file(&mut self) -> io::Result<()> {
        self.buffer.save()
    }

    pub fn theme(&self) -> &Theme {
        &self.config.theme
    }

    pub fn status_line(&self) -> String {
        let mode = match self.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Visual => "VISUAL",
            EditorMode::Command => "COMMAND",
        };
        let modified = if self.buffer.is_modified() { "[+]" } else { "" };
        let name = self.buffer.file_path().and_then(|p| p.to_str()).unwrap_or("[sin nombre]");
        format!(" {} {} {} — pos {} ", mode, name, modified, self.buffer.cursor())
    }

    fn record_edit(&mut self, kind: EditKind, position: usize, before: crate::rope::Rope, after: crate::rope::Rope) {
        self.undo_stack.push(Edit { kind, position, before, after });
    }

    pub fn handle_input(&mut self, event: KeyEvent) {
        let command = self.input_handler.map_key(event, self.mode);
        self.execute(command);
    }

    fn execute(&mut self, command: Command) {
        match command {
            Command::InsertChar(c) => {
                let pos = self.buffer.cursor();
                let before = self.buffer.rope().clone();
                let mut tmp = [0u8; 4];
                self.buffer.insert(pos, c.encode_utf8(&mut tmp));
                let after = self.buffer.rope().clone();
                self.buffer.set_cursor(pos + c.len_utf8());
                self.record_edit(EditKind::Insert, pos, before, after);
            }
            Command::InsertNewline => {
                let pos = self.buffer.cursor();
                let before = self.buffer.rope().clone();
                self.buffer.insert(pos, "\n");
                let after = self.buffer.rope().clone();
                self.buffer.set_cursor(pos + 1);
                self.record_edit(EditKind::Insert, pos, before, after);
            }
            Command::DeleteCharBackward => {
                let pos = self.buffer.cursor();
                if pos > 0 {
                    let before = self.buffer.rope().clone();
                    self.buffer.delete(pos - 1, pos);
                    let after = self.buffer.rope().clone();
                    self.buffer.set_cursor(pos - 1);
                    self.record_edit(EditKind::Delete, pos - 1, before, after);
                }
            }
            Command::DeleteCharForward => {
                let pos = self.buffer.cursor();
                if pos < self.buffer.rope().len() {
                    let before = self.buffer.rope().clone();
                    self.buffer.delete(pos, pos + 1);
                    let after = self.buffer.rope().clone();
                    self.record_edit(EditKind::Delete, pos, before, after);
                }
            }
            Command::MoveCursor(dir, n) => self.move_cursor(dir, n),
            Command::Save => {
                let _ = self.save_file();
            }
            Command::Quit => {
                self.running = false;
            }
            Command::Undo => {
                if let Some(edit) = self.undo_stack.undo() {
                    self.buffer.set_rope(edit.before);
                    self.buffer.set_cursor(edit.position);
                }
            }
            Command::Redo => {
                if let Some(edit) = self.undo_stack.redo() {
                    self.buffer.set_rope(edit.after);
                }
            }
            Command::Copy => {
                let line_num = self.buffer.rope().line_at(self.buffer.cursor());
                let text = self.buffer.line(line_num);
                self.clipboard.copy(&text);
            }
            Command::Paste => {
                let pos = self.buffer.cursor();
                let text = self.clipboard.paste().to_string();
                let before = self.buffer.rope().clone();
                self.buffer.insert(pos, &text);
                let after = self.buffer.rope().clone();
                self.buffer.set_cursor(pos + text.len());
                self.record_edit(EditKind::Insert, pos, before, after);
            }
            Command::SwitchMode(m) => {
                self.mode = m;
            }
            Command::None => {}
        }
    }

    fn move_cursor(&mut self, dir: Direction, n: usize) {
        let pos = self.buffer.cursor();
        let new_pos = match dir {
            Direction::Left => pos.saturating_sub(n),
            Direction::Right => (pos + n).min(self.buffer.rope().len()),
            // Simplificado: mover por línea real (manteniendo columna)
            // queda pendiente; por ahora Up/Down no desplazan el cursor.
            Direction::Up | Direction::Down => pos,
        };
        self.buffer.set_cursor(new_pos);
        let line = self.buffer.rope().line_at(new_pos);
        self.viewport.ensure_visible(line);
    }

    pub fn run(&mut self) {
        while self.running {
            self.renderer.render(&self.buffer, &self.viewport);
            let status = self.status_line();
            let style = self.config.theme.status_bar;
            self.renderer.draw_status_bar(&status, style);

            if let Some(event) = self.input_handler.read_key() {
                self.handle_input(event);
            }
        }
    }
}
