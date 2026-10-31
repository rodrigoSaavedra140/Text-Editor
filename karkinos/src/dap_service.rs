//! S5 (Roadmap v0.2.9): servicio del debugger DAP (lldb-dap).
//!
//! T5d: `DapService` posee la sesión (cliente `DapClient`, que vive en
//! `karkinos-core`) y su estado asociado: breakpoints y la línea donde
//! se detuvo la ejecución. Los orquestadores (toggle/start/step/stop/
//! poll) quedan en `app.rs` porque sincronizan presentación del widget
//! (`EditorState`), abren el archivo del frame activo (`open_path`) y
/// escriben el `status` del documento — es el puente debugger↔editor.

/// S4 (Rust IDE): servicio del debugger DAP nativo.
pub struct DapService {
    pub client: Option<karkinos_core::DapClient>,
    /// Breakpoint set por (ruta, línea) — el estado fuente de verdad,
    /// sincronizado al widget activo por `sync_editor_breakpoints`.
    pub breakpoints: std::collections::HashSet<(std::path::PathBuf, usize)>,
    pub panel_open: bool,
    /// Última línea donde el debugger se detuvo: (archivo, línea).
    pub stopped_line: Option<(std::path::PathBuf, usize)>,
}

impl Default for DapService {
    fn default() -> Self {
        DapService {
            client: None,
            breakpoints: std::collections::HashSet::new(),
            panel_open: false,
            stopped_line: None,
        }
    }
}
