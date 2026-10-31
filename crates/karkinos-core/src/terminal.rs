//! Headless cross-platform PTY and VT100 terminal emulation.
//!
//! Provides a real pseudo-terminal (PTY) backed by `portable-pty`
//! (Unix `openpty` + Windows `ConPTY`) and parsed with `vt100`.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::env;
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

/// Simplified color representation for terminal screen rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalColor {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

impl From<vt100::Color> for TerminalColor {
    fn from(c: vt100::Color) -> Self {
        match c {
            vt100::Color::Default => TerminalColor::Default,
            vt100::Color::Idx(i) => TerminalColor::Idx(i),
            vt100::Color::Rgb(r, g, b) => TerminalColor::Rgb(r, g, b),
        }
    }
}

/// An individual cell in the 2D terminal grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCell {
    pub contents: String,
    pub fg: TerminalColor,
    pub bg: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            contents: " ".to_string(),
            fg: TerminalColor::Default,
            bg: TerminalColor::Default,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }
}

/// A snapshot of the 2D terminal screen for UI rendering.
#[derive(Debug, Clone)]
pub struct TerminalSnapshot {
    pub rows: u16,
    pub cols: u16,
    pub cursor_pos: (u16, u16),
    pub hide_cursor: bool,
    pub grid: Vec<Vec<TerminalCell>>,
}

/// A headless cross-platform pseudo-terminal session.
pub struct PtyTerminal {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    child: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
    is_alive: Arc<AtomicBool>,
    pub shell_name: String,
    cols: u16,
    rows: u16,
}

impl PtyTerminal {
    /// Detects the system's preferred default interactive shell.
    pub fn detect_default_shell() -> (String, Vec<String>) {
        if cfg!(target_os = "windows") {
            // Prefer PowerShell if available, fallback to cmd.exe
            if let Ok(system_root) = env::var("SystemRoot") {
                let ps_path = PathBuf::from(&system_root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
                if ps_path.exists() {
                    return (ps_path.to_string_lossy().to_string(), vec!["-NoLogo".to_string()]);
                }
            }
            ("cmd.exe".to_string(), vec![])
        } else {
            // Linux / macOS: check $SHELL, then fallback to /bin/bash or /bin/sh
            if let Ok(shell) = env::var("SHELL") {
                if !shell.trim().is_empty() {
                    return (shell, vec![]);
                }
            }
            if Path::new("/bin/bash").exists() {
                ("/bin/bash".to_string(), vec![])
            } else {
                ("/bin/sh".to_string(), vec![])
            }
        }
    }

    /// Spawns a new interactive PTY terminal session with the specified size and working directory.
    pub fn spawn(cols: u16, rows: u16, working_dir: Option<&Path>) -> Result<Self, String> {
        let cols = cols.max(10);
        let rows = rows.max(3);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY: {e}"))?;

        let (shell_cmd, args) = Self::detect_default_shell();
        let shell_basename = Path::new(&shell_cmd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&shell_cmd)
            .to_string();

        let mut cmd = CommandBuilder::new(&shell_cmd);
        for arg in args {
            cmd.arg(arg);
        }
        if let Some(dir) = working_dir {
            if dir.exists() {
                cmd.cwd(dir);
            }
        }
        // Set TERM=xterm-256color and COLORTERM=truecolor for full modern terminal compatibility
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn shell '{shell_cmd}': {e}"))?;

        // Drop the slave immediately in the parent process so reader gets EOF when child exits
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY master reader: {e}"))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take PTY master writer: {e}"))?;

        let nz_rows = NonZeroU16::new(rows).unwrap_or(NonZeroU16::new(24).unwrap());
        let nz_cols = NonZeroU16::new(cols).unwrap_or(NonZeroU16::new(80).unwrap());
        let parser = Arc::new(Mutex::new(vt100::Parser::new(nz_rows, nz_cols, 1000)));
        let is_alive = Arc::new(AtomicBool::new(true));
        let child_arc = Arc::new(Mutex::new(Some(child)));

        // Background reader thread: continuously consumes bytes from PTY master and feeds vt100
        {
            let parser_clone = Arc::clone(&parser);
            let is_alive_clone = Arc::clone(&is_alive);
            thread::Builder::new()
                .name("pty-reader".to_string())
                .spawn(move || {
                    let mut buf = [0u8; 4096];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) => break, // EOF reached
                            Ok(n) => {
                                if let Ok(mut p) = parser_clone.lock() {
                                    p.process(&buf[..n]);
                                }
                            }
                            Err(_) => break, // Error or PTY closed
                        }
                    }
                    is_alive_clone.store(false, Ordering::SeqCst);
                })
                .map_err(|e| format!("Failed to spawn PTY reader thread: {e}"))?;
        }

        Ok(Self {
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            parser,
            child: child_arc,
            is_alive,
            shell_name: shell_basename,
            cols,
            rows,
        })
    }

    /// Resizes the PTY window and VT100 virtual screen buffer to match UI dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(10);
        let rows = rows.max(3);
        if self.cols == cols && self.rows == rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;

        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });

        if let Ok(mut p) = self.parser.lock() {
            let nz_rows = NonZeroU16::new(rows).unwrap_or(NonZeroU16::new(24).unwrap());
            let nz_cols = NonZeroU16::new(cols).unwrap_or(NonZeroU16::new(80).unwrap());
            p.set_size(nz_rows, nz_cols);
        }
    }

    /// Writes raw bytes directly to the PTY master stream.
    pub fn write_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| "Failed to lock PTY writer".to_string())?;
        writer
            .write_all(bytes)
            .map_err(|e| format!("Failed to write to PTY: {e}"))?;
        writer
            .flush()
            .map_err(|e| format!("Failed to flush PTY writer: {e}"))?;
        Ok(())
    }

    /// Writes a UTF-8 string directly into the PTY.
    pub fn write_str(&self, text: &str) -> Result<(), String> {
        self.write_bytes(text.as_bytes())
    }

    /// Takes a thread-safe snapshot of the current 2D screen grid for UI presentation.
    pub fn snapshot(&self) -> TerminalSnapshot {
        let parser = match self.parser.lock() {
            Ok(p) => p,
            Err(poisoned) => poisoned.into_inner(),
        };
        let screen = parser.screen();
        let (screen_rows, screen_cols) = screen.size();
        let num_rows = screen_rows.get();
        let num_cols = screen_cols.get();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let hide_cursor = screen.hide_cursor();

        let mut grid = Vec::with_capacity(num_rows as usize);
        for row in 0..num_rows {
            let mut row_cells = Vec::with_capacity(num_cols as usize);
            for col in 0..num_cols {
                if let Some(cell) = screen.cell(row, col) {
                    row_cells.push(TerminalCell {
                        contents: cell.contents().to_string(),
                        fg: cell.fgcolor().into(),
                        bg: cell.bgcolor().into(),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
                    });
                } else {
                    row_cells.push(TerminalCell::default());
                }
            }
            grid.push(row_cells);
        }

        TerminalSnapshot {
            rows: num_rows,
            cols: num_cols,
            cursor_pos: (cursor_row, cursor_col),
            hide_cursor,
            grid,
        }
    }

    /// Returns true if the child shell process and PTY stream are still running.
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }

    /// Gracefully kills the child process.
    pub fn kill(&self) {
        if let Ok(mut lock) = self.child.lock() {
            if let Some(mut child) = lock.take() {
                let _ = child.kill();
            }
        }
        self.is_alive.store(false, Ordering::SeqCst);
    }
}

impl Drop for PtyTerminal {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_default_shell_not_empty() {
        let (shell, _args) = PtyTerminal::detect_default_shell();
        assert!(!shell.trim().is_empty());
    }

    #[test]
    fn test_pty_terminal_spawn_and_snapshot() {
        let term = PtyTerminal::spawn(80, 24, None);
        assert!(term.is_ok(), "PtyTerminal::spawn should succeed: {:?}", term.err());
        let term = term.unwrap();
        assert!(term.is_alive());

        let snapshot = term.snapshot();
        assert_eq!(snapshot.cols, 80);
        assert_eq!(snapshot.rows, 24);
        assert_eq!(snapshot.grid.len(), 24);
        assert_eq!(snapshot.grid[0].len(), 80);
    }

    #[test]
    fn test_pty_terminal_write_and_resize() {
        let mut term = PtyTerminal::spawn(60, 20, None).expect("spawn failed");
        assert!(term.write_str("echo 'karkinos test'\n").is_ok());

        term.resize(100, 30);
        let snapshot = term.snapshot();
        assert_eq!(snapshot.cols, 100);
        assert_eq!(snapshot.rows, 30);
        assert_eq!(snapshot.grid.len(), 30);
        assert_eq!(snapshot.grid[0].len(), 100);
    }

    #[test]
    fn test_terminal_color_conversion() {
        assert_eq!(TerminalColor::from(vt100::Color::Default), TerminalColor::Default);
        assert_eq!(TerminalColor::from(vt100::Color::Idx(42)), TerminalColor::Idx(42));
        assert_eq!(TerminalColor::from(vt100::Color::Rgb(10, 20, 30)), TerminalColor::Rgb(10, 20, 30));
    }
}
