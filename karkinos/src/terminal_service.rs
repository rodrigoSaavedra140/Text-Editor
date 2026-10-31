//! S5 (Roadmap v0.2.25): Interactive PTY Terminal Service.
//!
//! Manages the lifecycle of an interactive pseudo-terminal session (backed by portable-pty
//! and parsed by vt100). Translates egui keyboard events into VT/ANSI escape sequences
//! and streams them to the running shell child process.

use eframe::egui;
use karkinos_core::terminal::{PtyTerminal, TerminalSnapshot};
use std::path::Path;

/// Service managing the interactive PTY terminal session.
pub struct TerminalService {
    pub pty: Option<PtyTerminal>,
    pub cols: u16,
    pub rows: u16,
}

impl Default for TerminalService {
    fn default() -> Self {
        Self {
            pty: None,
            cols: 80,
            rows: 24,
        }
    }
}

impl TerminalService {
    /// Ensures that the PTY session is running and resized to the given dimensions.
    pub fn ensure_running(&mut self, working_dir: &Path, cols: u16, rows: u16) {
        let cols = cols.max(10);
        let rows = rows.max(3);

        if let Some(pty) = &mut self.pty {
            if pty.is_alive() {
                if self.cols != cols || self.rows != rows {
                    pty.resize(cols, rows);
                    self.cols = cols;
                    self.rows = rows;
                }
                return;
            }
        }

        // Spawn fresh PTY session
        match PtyTerminal::spawn(cols, rows, Some(working_dir)) {
            Ok(pty) => {
                self.cols = cols;
                self.rows = rows;
                self.pty = Some(pty);
            }
            Err(e) => {
                eprintln!("Failed to spawn PTY terminal: {e}");
            }
        }
    }

    /// Restarts the terminal shell session in the specified directory.
    pub fn restart(&mut self, working_dir: &Path) {
        if let Some(pty) = self.pty.take() {
            pty.kill();
        }
        self.ensure_running(working_dir, self.cols, self.rows);
    }

    /// Returns a snapshot of the terminal screen, if active.
    pub fn snapshot(&self) -> Option<TerminalSnapshot> {
        self.pty.as_ref().map(|p| p.snapshot())
    }

    /// Returns true if the child shell process is currently running.
    pub fn is_alive(&self) -> bool {
        self.pty.as_ref().map_or(false, |p| p.is_alive())
    }

    /// Returns the name of the active shell.
    pub fn shell_name(&self) -> &str {
        self.pty
            .as_ref()
            .map_or("Terminal", |p| p.shell_name.as_str())
    }

    /// Sends raw bytes to the PTY.
    pub fn send_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        if let Some(pty) = &self.pty {
            pty.write_bytes(bytes)
        } else {
            Err("Terminal is not running".to_string())
        }
    }

    /// Sends a UTF-8 string to the PTY.
    pub fn send_str(&self, text: &str) -> Result<(), String> {
        if let Some(pty) = &self.pty {
            pty.write_str(text)
        } else {
            Err("Terminal is not running".to_string())
        }
    }

    /// Clears the terminal screen (sends Form Feed / Ctrl+L).
    pub fn clear(&self) {
        let _ = self.send_bytes(b"\x0c");
    }

    /// Translates an egui key event into terminal escape sequences and sends it to the PTY.
    pub fn handle_key_event(&self, key: egui::Key, modifiers: &egui::Modifiers) -> bool {
        let Some(pty) = &self.pty else { return false };

        // Handle Ctrl combinations (SIGINT, EOF, line editing, etc.)
        if modifiers.ctrl || modifiers.command {
            let ctrl_seq: Option<&[u8]> = match key {
                egui::Key::A => Some(b"\x01"),
                egui::Key::B => Some(b"\x02"),
                egui::Key::C => Some(b"\x03"), // SIGINT
                egui::Key::D => Some(b"\x04"), // EOF
                egui::Key::E => Some(b"\x05"),
                egui::Key::F => Some(b"\x06"),
                egui::Key::G => Some(b"\x07"),
                egui::Key::H => Some(b"\x08"),
                egui::Key::I => Some(b"\t"),
                egui::Key::J => Some(b"\n"),
                egui::Key::K => Some(b"\x0b"),
                egui::Key::L => Some(b"\x0c"), // Clear
                egui::Key::N => Some(b"\x0e"),
                egui::Key::P => Some(b"\x10"),
                egui::Key::R => Some(b"\x12"), // Reverse search
                egui::Key::U => Some(b"\x15"), // Kill line
                egui::Key::W => Some(b"\x17"), // Kill word
                egui::Key::Z => Some(b"\x1a"), // SIGTSTP
                _ => None,
            };
            if let Some(seq) = ctrl_seq {
                let _ = pty.write_bytes(seq);
                return true;
            }
        }

        // Handle standard terminal control keys
        let seq: Option<&[u8]> = match key {
            egui::Key::Enter => Some(b"\r"),
            egui::Key::Backspace => Some(b"\x7f"),
            egui::Key::Tab => Some(b"\t"),
            egui::Key::Escape => Some(b"\x1b"),
            egui::Key::ArrowUp => Some(b"\x1b[A"),
            egui::Key::ArrowDown => Some(b"\x1b[B"),
            egui::Key::ArrowRight => Some(b"\x1b[C"),
            egui::Key::ArrowLeft => Some(b"\x1b[D"),
            egui::Key::Home => Some(b"\x1b[H"),
            egui::Key::End => Some(b"\x1b[F"),
            egui::Key::PageUp => Some(b"\x1b[5~"),
            egui::Key::PageDown => Some(b"\x1b[6~"),
            egui::Key::Delete => Some(b"\x1b[3~"),
            egui::Key::Insert => Some(b"\x1b[2~"),
            _ => None,
        };

        if let Some(s) = seq {
            let _ = pty.write_bytes(s);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_service_default() {
        let svc = TerminalService::default();
        assert!(svc.pty.is_none());
        assert_eq!(svc.cols, 80);
        assert_eq!(svc.rows, 24);
        assert_eq!(svc.shell_name(), "Terminal");
    }

    #[test]
    fn test_terminal_service_lifecycle() {
        let mut svc = TerminalService::default();
        let tmp = std::env::temp_dir();
        svc.ensure_running(&tmp, 60, 20);
        assert!(svc.pty.is_some());
        assert!(svc.is_alive());
        assert_eq!(svc.cols, 60);
        assert_eq!(svc.rows, 20);

        let snap = svc.snapshot();
        assert!(snap.is_some());
        let snap = snap.unwrap();
        assert_eq!(snap.cols, 60);
        assert_eq!(snap.rows, 20);

        // Test resize
        svc.ensure_running(&tmp, 70, 25);
        assert_eq!(svc.cols, 70);
        assert_eq!(svc.rows, 25);
    }

    #[test]
    fn test_terminal_service_key_handling() {
        let mut svc = TerminalService::default();
        let tmp = std::env::temp_dir();
        svc.ensure_running(&tmp, 60, 20);

        let none_mod = egui::Modifiers::NONE;
        let ctrl_mod = egui::Modifiers { ctrl: true, ..Default::default() };

        assert!(svc.handle_key_event(egui::Key::Enter, &none_mod));
        assert!(svc.handle_key_event(egui::Key::Backspace, &none_mod));
        assert!(svc.handle_key_event(egui::Key::ArrowUp, &none_mod));
        assert!(svc.handle_key_event(egui::Key::C, &ctrl_mod));
        assert!(svc.handle_key_event(egui::Key::L, &ctrl_mod));
        assert!(!svc.handle_key_event(egui::Key::A, &none_mod)); // regular character without modifiers returns false
    }
}
