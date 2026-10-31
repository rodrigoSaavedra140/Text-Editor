//! S5 (Roadmap v0.2.9): servicio de ejecución de Cargo.
//!
//! T5c: `CargoService` posee el runner del subproceso (el streaming de
//! mensajes del compilador vive en `karkinos-core/src/cargo.rs`) y el
//! estado del panel de salida. `jump_to_cargo_diagnostic` queda como
//! orquestación en `app.rs` porque salta a través del documento activo
//! y del estado de presentación del widget.

use karkinos_core::cargo::CargoRunner;

/// S1 (Rust IDE): servicio de comandos de Cargo.
pub struct CargoService {
    pub runner: CargoRunner,
    pub panel_open: bool,
    pub active_tab: usize,
    pub terminal_input: String,
    pub command_history: Vec<String>,
    pub history_index: Option<usize>,
}

impl Default for CargoService {
    fn default() -> Self {
        CargoService {
            runner: CargoRunner::new(),
            panel_open: false,
            active_tab: 0,
            terminal_input: String::new(),
            command_history: Vec::new(),
            history_index: None,
        }
    }
}

impl CargoService {
    /// S1 (Rust IDE): Ejecuta un comando de Cargo en el directorio de
    /// trabajo dado (el orquestador resuelve `dir` a partir del archivo
    /// activo o la carpeta mostrada).
    pub fn run(&mut self, action: karkinos_core::cargo::CargoAction, dir: &std::path::Path) {
        self.panel_open = true;
        let _ = self.runner.spawn(action, dir, &[]);
    }

    /// Retrocede en el historial de comandos de la terminal (flecha arriba).
    pub fn history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        let next_idx = match self.history_index {
            None => self.command_history.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        };
        self.history_index = Some(next_idx);
        if let Some(cmd) = self.command_history.get(next_idx) {
            self.terminal_input = cmd.clone();
        }
    }

    /// Avanza en el historial de comandos de la terminal (flecha abajo).
    pub fn history_next(&mut self) {
        if let Some(i) = self.history_index {
            if i + 1 < self.command_history.len() {
                let next_idx = i + 1;
                self.history_index = Some(next_idx);
                self.terminal_input = self.command_history[next_idx].clone();
            } else {
                self.history_index = None;
                self.terminal_input.clear();
            }
        }
    }

    /// Ejecuta el comando ingresado en la terminal o lo envía como stdin al proceso en ejecución.
    pub fn execute_or_input(&mut self, working_dir: &std::path::Path) {
        let input = self.terminal_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        if self.command_history.last().map(|s| s.as_str()) != Some(&input) {
            self.command_history.push(input.clone());
        }
        self.history_index = None;
        self.terminal_input.clear();

        if self.runner.is_running() {
            let _ = self.runner.write_stdin(&input);
        } else {
            let _ = self.runner.spawn_command(&input, working_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_service_history_navigation() {
        let mut service = CargoService::default();
        service.command_history.push("cargo check".to_string());
        service.command_history.push("cargo test".to_string());

        service.history_prev();
        assert_eq!(service.terminal_input, "cargo test");

        service.history_prev();
        assert_eq!(service.terminal_input, "cargo check");

        service.history_next();
        assert_eq!(service.terminal_input, "cargo test");

        service.history_next();
        assert_eq!(service.terminal_input, "");
        assert_eq!(service.history_index, None);
    }
}
