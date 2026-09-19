use std::io;
use std::path::{Path, PathBuf};

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
    // Buffer de texto para el modo ":" (estilo vim) — ej. ":w archivo.txt"
    command_buffer: String,
    // Último mensaje para mostrar en la barra de estado (guardado
    // exitoso, error, comando desconocido, etc.)
    message: String,
}

impl Editor {
    pub fn new() -> Self {
        let backend: Box<dyn Backend> = Box::new(TerminalBackend::new());
        let (w, h) = backend.size();
        Editor {
            buffer: Buffer::new(),
            viewport: Viewport::new(w as usize, h.saturating_sub(2).max(1) as usize),
            renderer: Renderer::new(backend),
            input_handler: InputHandler::new(),
            mode: EditorMode::Normal,
            clipboard: Clipboard::new(),
            undo_stack: UndoStack::new(),
            config: Config::default(),
            running: true,
            command_buffer: String::new(),
            message: String::new(),
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
        // Mientras estás escribiendo un comando (":w archivo.txt"),
        // la barra de estado muestra justamente eso, para que veas
        // lo que vas tipeando.
        if self.mode == EditorMode::Command {
            return format!(" :{}", self.command_buffer);
        }
        let mode = match self.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Visual => "VISUAL",
            EditorMode::Command => "COMMAND",
        };
        let modified = if self.buffer.is_modified() { "[+]" } else { "" };
        let name = self.buffer.file_path().and_then(|p| p.to_str()).unwrap_or("[sin nombre]");
        if self.message.is_empty() {
            format!(" {} {} {} — pos {} ", mode, name, modified, self.buffer.cursor())
        } else {
            format!(" {} {} {} — {} ", mode, name, modified, self.message)
        }
    }

    fn record_edit(&mut self, kind: EditKind, position: usize, before: crate::rope::Rope, after: crate::rope::Rope) {
        self.undo_stack.push(Edit { kind, position, before, after });
    }

    /// Keybinds vigentes según el modo actual, para mostrar en la
    /// barra de ayuda (estilo nano: separados por "│").
    pub fn keybind_hints(&self) -> String {
        let global = "^S Guardar │ ^Q Salir │ ^R Rehacer";
        match self.mode {
            EditorMode::Normal => format!(
                "i Insertar │ : Comando │ x Borrar │ y Copiar línea │ p Pegar │ u Deshacer │ ←↑↓→ Mover │ {}",
                global
            ),
            EditorMode::Insert => format!(
                "Esc Volver a Normal │ Enter Nueva línea │ Backspace Borrar │ {}",
                global
            ),
            EditorMode::Command => "Enter Ejecutar │ Esc Cancelar │ :w [archivo] Guardar │ :e archivo Abrir │ :q Salir │ :wq Guardar y salir".to_string(),
            EditorMode::Visual => format!("(modo sin atajos propios todavía) │ {}", global),
        }
    }

    pub fn handle_input(&mut self, event: KeyEvent) {
        let command = self.input_handler.map_key(event, self.mode);
        self.execute(command);
    }

    /// Parsea y ejecuta lo que se escribió en el modo ":" — soporta
    /// "w", "w archivo.txt", "q" y "wq [archivo.txt]".
    fn run_command_line(&mut self) {
        let line = self.command_buffer.trim().to_string();
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next();

        match cmd {
            "w" => self.do_save(arg),
            "q" => self.running = false,
            "wq" | "x" => {
                self.do_save(arg);
                self.running = false;
            }
            "e" => match arg {
                Some(path) => self.do_open(path, false),
                None => self.message = "uso: :e archivo.txt".to_string(),
            },
            "e!" => match arg {
                Some(path) => self.do_open(path, true),
                None => self.message = "uso: :e! archivo.txt".to_string(),
            },
            "" => {}
            other => {
                self.message = format!("comando desconocido: {}", other);
            }
        }
    }

    /// Abre `path_str` en el buffer actual. Si hay cambios sin
    /// guardar y `force` es false, se niega y avisa (estilo vim);
    /// con `force = true` (":e!") descarta los cambios sin preguntar.
    fn do_open(&mut self, path_str: &str, force: bool) {
        if self.buffer.is_modified() && !force {
            self.message = "hay cambios sin guardar — usá :e! para descartarlos".to_string();
            return;
        }
        match self.open_file(Path::new(path_str)) {
            Ok(()) => {
                // El historial de undo pertenece al archivo anterior;
                // no tendría sentido deshacer sobre el archivo nuevo.
                self.undo_stack = UndoStack::new();
                self.message = format!("abierto {}", path_str);
            }
            Err(e) => {
                self.message = format!("error al abrir: {}", e);
            }
        }
    }

    fn do_save(&mut self, arg: Option<&str>) {
        if let Some(path_str) = arg {
            self.buffer.set_file_path(PathBuf::from(path_str));
        }
        match self.save_file() {
            Ok(()) => {
                let name = self.buffer.file_path().and_then(|p| p.to_str()).unwrap_or("?");
                self.message = format!("guardado en {}", name);
            }
            Err(e) => {
                self.message = format!("error al guardar: {}", e);
            }
        }
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
                self.do_save(None);
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
                if m == EditorMode::Command {
                    self.command_buffer.clear();
                    self.message.clear();
                }
                self.mode = m;
            }
            Command::CommandChar(c) => {
                self.command_buffer.push(c);
            }
            Command::CommandBackspace => {
                self.command_buffer.pop();
            }
            Command::CommandExecute => {
                self.run_command_line();
                self.mode = EditorMode::Normal;
                self.command_buffer.clear();
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
        // Limpiamos la pantalla UNA sola vez al arrancar (ya no en
        // cada frame, para no titilar).
        self.renderer.clear_screen();

        while self.running {
            self.renderer.render(&self.buffer, &self.viewport);

            let hints = self.keybind_hints();
            let help_style = self.config.theme.help_bar;
            self.renderer.draw_help_bar(&hints, help_style);

            let status = self.status_line();
            let status_style = self.config.theme.status_bar;
            self.renderer.draw_status_bar(&status, status_style);

            if let Some(event) = self.input_handler.read_key() {
                self.handle_input(event);
            }
        }
    }
}