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
}

impl Default for CargoService {
    fn default() -> Self {
        CargoService {
            runner: CargoRunner::new(),
            panel_open: false,
            active_tab: 0,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_service_default() {
        let service = CargoService::default();
        assert!(!service.panel_open);
        assert_eq!(service.active_tab, 0);
    }
}
