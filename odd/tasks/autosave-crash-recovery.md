# Roadmap v0.2.51: Auto-Save Journaling & Crash Recovery

## Contexto & Objetivos
En un editor de código profesional, la pérdida de trabajo debido a cierres inesperados, pánico de drivers gráficos, cuelgues del sistema o reinicios accidentales es inaceptable.
Karkinos debe implementar un sistema de **Journaling & Crash Recovery** que guarde periódicamente snapshots de los buffers modificados (dirty) en un directorio seguro de estado del usuario (`~/.local/state/karkinos/recovery/`), los elimine limpiamente al guardar o descartar, y permita restaurar automáticamente o bajo demanda el trabajo pendiente al reiniciar la aplicación.

---

## Tarea A: Módulo Core de Recovery en `crates/karkinos-core/src/recovery.rs`

### Archivos Afectados
- `crates/karkinos-core/Cargo.toml` (asegurar serde/serde_json si aplica o formato texto estructurado)
- `crates/karkinos-core/src/lib.rs` (exportar `recovery`)
- `crates/karkinos-core/src/recovery.rs` (nuevo)

### Especificación Técnica
1. Estructura `RecoveryEntry`:
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq)]
   pub struct RecoveryEntry {
       pub original_path: Option<std::path::PathBuf>,
       pub title: String,
       pub content: String,
       pub cursor_line: usize,
       pub cursor_col: usize,
       pub timestamp_secs: u64,
   }
   ```
2. Funciones puras y de I/O seguro:
   - `recovery_dir() -> std::path::PathBuf`:
     Resuelve `$XDG_STATE_HOME/karkinos/recovery` o `$HOME/.local/state/karkinos/recovery` (fallback a `std::env::temp_dir().join("karkinos/recovery")`).
   - `journal_filename(path: Option<&std::path::Path>, tab_id: usize) -> String`:
     Genera un nombre estable y seguro para el sistema de archivos (ej. hash SHA256 o FNV-1a de la ruta absoluta o `untitled_tab_<id>.recovery`).
   - `serialize_entry(entry: &RecoveryEntry) -> String`:
     Serialización estructurada (cabecera con metadatos + delimitador + contenido íntegro del buffer).
   - `deserialize_entry(data: &str) -> Option<RecoveryEntry>`:
     Parser robusto que tolera recuperaciones parciales y extrae metadatos y contenido.
   - `save_recovery_file(dir: &std::path::Path, filename: &str, entry: &RecoveryEntry) -> std::io::Result<std::path::PathBuf>`:
     Escritura atómica: escribe primero en `.tmp` en el mismo directorio y renombra a destino.
   - `remove_recovery_file(dir: &std::path::Path, filename: &str) -> std::io::Result<()>`:
     Eliminación limpia y segura si el archivo existe.
   - `scan_recovery_dir(dir: &std::path::Path) -> Vec<(std::path::PathBuf, RecoveryEntry)>`:
     Escanea el directorio y devuelve todas las entradas válidas encontradas.
   - `clear_recovery_dir(dir: &std::path::Path) -> std::io::Result<()>`:
     Elimina todos los archivos `.recovery` del directorio.
3. Tests Unitarios (100% de cobertura en `recovery.rs`):
   - Serialización y deserialización idéntica (roundtrip) de archivos con ruta y buffers sin guardar (untitled).
   - Escritura atómica y verificación en disco en directorio temporal.
   - Borrado de archivo y manejo cuando el archivo no existe.
   - Escaneo y detección de múltiples entradas de recuperación.
   - Resiliencia ante datos corruptos o cabeceras truncadas.

---

## Tarea B: Integración en DocumentService y App Lifecycle (`karkinos`)

### Archivos Afectados
- `karkinos/src/document_service.rs`
- `karkinos/src/app.rs`
- `karkinos/src/ui.rs`

### Especificación Técnica
1. **Detección al Inicio (`app.rs`)**:
   - Durante `EditorApp::new()`, se ejecuta `scan_recovery_dir(&recovery_dir())`.
   - Si existen entradas recuperables:
     - Se guardan en `app.recovery_pending: Vec<(PathBuf, RecoveryEntry)>`.
     - `app.show_recovery_modal = true`.
2. **Modal de Recuperación (`ui.rs`)**:
   - Modal visible al abrir si `app.show_recovery_modal == true`:
     - Título: "⚠️ Sesión previa recuperada (Crash Recovery)".
     - Descripción: "Se encontraron archivos modificados sin guardar de una sesión anterior:".
     - Lista de archivos con nombre, hora y líneas.
     - Botón "Restaurar todo":
       Abre cada archivo en una pestaña nueva o existente, asigna el texto recuperado, posiciona el cursor, marca `tab.dirty = true` y limpia los archivos de recovery.
     - Botón "Descartar":
       Limpia el directorio de recovery y cierra el modal sin alterar los archivos en disco.
3. **Journaling Periódico en Background (`app.rs` / `document_service.rs`)**:
   - En cada tick de la UI (en `poll_recovery_autosave(&mut self)`):
     - Si han pasado más de 1.5 segundos desde la última modificación en alguna pestaña dirty y el contenido difiere del último journal emitido:
       - Guarda la entrada de recovery atómica en `recovery_dir()`.
4. **Limpieza en Guardado y Cierre**:
   - Al ejecutar `save` / `Ctrl+S` exitosamente:
     - Se elimina inmediatamente el archivo de recovery correspondiente a esa pestaña.
   - Al cerrar una pestaña sin cambios o confirmando descarte:
     - Se elimina el archivo de recovery correspondiente.

---

## Criterios de Aceptación
1. `cargo check --workspace` sin advertencias (0 warnings).
2. `cargo test --workspace` con 100% de tests pasando.
3. Si la app se cierra forzadamente (ej. `kill -9`), al volver a abrir se ofrece la recuperación intacta del contenido no guardado.
4. Al guardar con `Ctrl+S`, el archivo de recuperación desaparece inmediatamente del directorio de estado.

---

## Log

- **L1** (2026-11-03): "Read odd/tasks/autosave-crash-recovery.md and implement Tarea B: Integrate crash recovery into karkinos/src/app.rs and karkinos/src/ui.rs. Add recovery_pending, show_recovery_modal, poll_recovery_autosave, restore_all_recovery, discard_all_recovery, cleanup on save and tab close, and recovery modal in ui.rs. Add unit tests in app.rs and verify cargo check --workspace passes with 0 warnings."
- **L2 — Tarea B completa**: `EditorApp::new()` escanea `recovery_dir()` y llena `recovery_pending`/`show_recovery_modal` (Default queda sin I/O para tests); `poll_recovery_autosave` escribe journals atómicos con debounce de 1.5 s y throttle de 250 ms; `restore_all_recovery` reabre cada entrada en pestaña nueva, restaura contenido/cursor y marca dirty; `discard_all_recovery` limpia el directorio; limpieza de journals al guardar, cerrar pestaña y descartar el cierre completo. UI: modal de Crash Recovery con lista (nombre, edad, líneas), "Restaurar todo" y "Descartar", backdrop y llamada al final de `update()`. Tests nuevos en `app.rs` (detección, debounce, modal abierto, restore, discard, cleanup y formato de edad).
- **L3 — Validación**: `cargo check --workspace` 0 warnings; `cargo test --workspace` 343 tests verdes (205 `karkinos` + 138 `karkinos-core`, 0 fallos).
