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
    // OPT-1 (v0.2.26): snapshot de la última grilla generada junto a la
    // revisión con la que se generó. Mientras la terminal está en reposo se
    // reutiliza sin volver a recorrer las celdas ni asignar strings.
    cached_snapshot: Option<(u64, TerminalSnapshot)>,
}

impl Default for TerminalService {
    fn default() -> Self {
        Self {
            pty: None,
            cols: 80,
            rows: 24,
            cached_snapshot: None,
        }
    }
}

/// OPT-1 (v0.2.26): decide si el snapshot cacheado quedó viejo. Se regenera
/// cuando no hay caché o cuando la revisión del PTY avanzó (bytes nuevos o
/// un resize); con la misma revisión se reutiliza tal cual.
fn needs_snapshot_refresh(cached_revision: Option<u64>, revision: u64) -> bool {
    cached_revision != Some(revision)
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
                // OPT-1: una sesión nueva arranca en revisión 0, que puede
                // coincidir con la vieja; la caché se descarta explícitamente.
                self.cached_snapshot = None;
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
        self.cached_snapshot = None;
        self.ensure_running(working_dir, self.cols, self.rows);
    }

    /// OPT-1 (v0.2.26): revisión actual de la grilla del PTY (0 si no hay
    /// sesión). Sirve para saber si la UI necesita repintar.
    pub fn revision(&self) -> u64 {
        self.pty.as_ref().map_or(0, |p| p.revision())
    }

    /// Returns a snapshot of the terminal screen, if active.
    ///
    /// OPT-1: la grilla sólo se recorre cuando cambió la revisión del PTY;
    /// si no, se devuelve la misma referencia cacheada (cero asignaciones
    /// en reposo). La revisión se lee ANTES de fotografiar: si llegan bytes
    /// mientras se genera, la caché queda vieja y el próximo frame la
    /// regenera, en vez de guardar contenido viejo con revisión nueva.
    pub fn snapshot(&mut self) -> Option<&TerminalSnapshot> {
        let revision = self.pty.as_ref()?.revision();
        let cached_revision = self.cached_snapshot.as_ref().map(|(cached, _)| *cached);
        if needs_snapshot_refresh(cached_revision, revision) {
            let snapshot = self.pty.as_ref()?.snapshot();
            self.cached_snapshot = Some((revision, snapshot));
        }
        self.cached_snapshot.as_ref().map(|(_, snapshot)| snapshot)
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

    /// OPT-1: sólo se regenera el snapshot cuando la revisión cambió.
    #[test]
    fn snapshot_refresh_depends_on_revision() {
        assert!(needs_snapshot_refresh(None, 0), "sin caché siempre se genera");
        assert!(!needs_snapshot_refresh(Some(3), 3), "misma revisión: se reutiliza");
        assert!(needs_snapshot_refresh(Some(3), 4), "revisión nueva: se regenera");
        assert!(needs_snapshot_refresh(Some(4), 3), "sesión nueva en revisión 0: se regenera");
    }

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

        {
            let snap = svc.snapshot().expect("snapshot de la sesión viva");
            assert_eq!(snap.cols, 60);
            assert_eq!(snap.rows, 20);
        }

        // Test resize
        svc.ensure_running(&tmp, 70, 25);
        assert_eq!(svc.cols, 70);
        assert_eq!(svc.rows, 25);
    }

    /// OPT-1: en reposo, dos pedidos seguidos devuelven el MISMO snapshot
    /// (se reutiliza la caché en vez de recorrer la grilla otra vez).
    #[test]
    fn test_terminal_snapshot_cache_reuses_same_grid() {
        let mut svc = TerminalService::default();
        let tmp = std::env::temp_dir();
        svc.ensure_running(&tmp, 40, 12);

        // El shell imprime su prompt al arrancar: esperamos a que la
        // revisión se estabilice para que la comparación sea determinista.
        let mut revision = svc.revision();
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let next = svc.revision();
            if next == revision {
                break;
            }
            revision = next;
        }

        let first = svc.snapshot().unwrap() as *const TerminalSnapshot;
        let second = svc.snapshot().unwrap() as *const TerminalSnapshot;
        assert_eq!(first, second, "sin cambios de revisión se reutiliza la misma grilla");
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
