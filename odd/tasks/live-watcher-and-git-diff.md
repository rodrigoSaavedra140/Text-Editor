# Roadmap v0.2.44: Live Workspace Watcher & Interactive Git Diff Viewer

## Contexto & Objetivos
1. **Live Workspace & Buffer Watcher (Sincronización en Caliente)**:
   - Hasta v0.2.43, si un desarrollador ejecuta `git checkout`, `git pull`, `cargo build` o formatea código desde la terminal PTY o externamente, el explorador y el badge de Git (`🌿`) no se actualizan hasta que el usuario hace clic manual en refrescar.
   - Además, si un archivo abierto cambia en disco pero el buffer está limpio (`!is_dirty`), el usuario tenía que cerrar y volver a abrir la pestaña para ver los cambios.
   - Solución: Un ciclo liviano de detección periódica (cadencia de 1.5s - 2s en `EditorApp::update` usando `Instant`) que:
     - Detecta cambios de mtime en `.git/HEAD` o `.git/index` y llama a `refresh_git_status()`.
     - Detecta cambios en la carpeta raíz (`browse_dir`) y llama a `refresh_explorer()`.
     - Detecta si las pestañas abiertas tienen mtime desactualizado en disco:
       - Si el buffer está limpio (`!tab.is_dirty`): recarga el buffer en caliente, actualiza mtime y limpia dirty.
       - Si el buffer tiene cambios locales (`tab.is_dirty`): preserva los cambios locales y notifica en la barra de estado (`"File modified externally: <file>"`).

2. **Visor de Diff de Git Interactivo (`query_git_diff`)**:
   - En la pestaña de Source Control (`SidebarTab::SourceControl`), el usuario ve los archivos modificados `[M]`, pero no puede ver exactamente qué líneas cambiaron antes de commitear.
   - Solución:
     - Motor en `crates/karkinos-core/src/git_status.rs`:
       `pub fn query_git_diff(repo_root: &Path, rel_path: Option<&Path>, staged: bool) -> Result<String, String>`
       que ejecuta `git -C <repo_root> diff HEAD -- <rel_path>` (o `--cached` si staged).
     - Estado en `karkinos/src/app.rs`:
       `pub git_diff_view: Option<GitDiffViewState>` con el path, contenido del diff y título.
     - Botón `↔` en cada fila de archivo en la pestaña Source Control que abre el visor de diff.
     - Renderizado en `karkinos/src/ui.rs`:
       Ventana modal o panel secundario con renderizado de hunks:
       - Renglones `+` en fondo/texto verde translúcido.
       - Renglones `-` en fondo/texto rojo translúcido.
       - Renglones `@@` en cyan tenue.
       - Botón cerrar (Escape o `✕`).
     - Comando en Command Palette: `git:view_diff` ("Git: View Diff of Active File").

---

## Tarea A: Motor de Git Diff en `karkinos-core`

### Archivos Afectados
- `crates/karkinos-core/src/git_status.rs`

### Especificación Técnica
1. Implementar función:
   ```rust
   pub fn query_git_diff(
       repo_root: &Path,
       rel_path: Option<&Path>,
       staged: bool,
   ) -> Result<String, String>
   ```
   - Si `staged` es true, pasa `--cached`.
   - Si `rel_path` es Some, pasa `--` y la ruta relativa.
   - Ejecuta `git -C <repo_root> diff [args]` con `std::process::Command`.
   - Retorna stdout como `String`. Si el diff está vacío, retorna cadena vacía o indicador claro.
2. Estructura de hunk parsed para soporte opcional o renderizado:
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq)]
   pub enum DiffLineKind {
       Header,
       HunkHeader,
       Added,
       Deleted,
       Context,
   }

   #[derive(Debug, Clone, PartialEq, Eq)]
   pub struct ParsedDiffLine {
       pub kind: DiffLineKind,
       pub text: String,
   }

   pub fn parse_unified_diff(diff: &str) -> Vec<ParsedDiffLine>
   ```
   - Clasifica líneas `+` (Added), `-` (Deleted), `@@` (HunkHeader), `diff --git`/`index`/`---`/`+++` (Header), y resto (Context).
3. Tests unitarios en `crates/karkinos-core/src/git_status.rs`:
   - `parse_unified_diff` con agregados, borrados y encabezados.
   - `query_git_diff` en repositorio temporal verificando que refleja cambios no commiteados.

---

## Tarea B: Watcher Periódico y Estado de Diff en `karkinos/src/app.rs`

### Archivos Afectados
- `karkinos/src/app.rs`

### Especificación Técnica
1. Estado del visor de diff:
   ```rust
   #[derive(Debug, Clone, Default)]
   pub struct GitDiffViewState {
       pub file_path: std::path::PathBuf,
       pub rel_name: String,
       pub diff_text: String,
       pub parsed_lines: Vec<karkinos_core::git_status::ParsedDiffLine>,
       pub is_staged: bool,
   }
   ```
   En `EditorApp`:
   - `pub git_diff_modal: Option<GitDiffViewState>`
   - `pub fn open_git_diff(&mut self, path: &std::path::Path, staged: bool)`
   - `pub fn close_git_diff(&mut self)`
2. Watcher periódico de disco:
   En `EditorApp`:
   - `pub last_disk_poll: Option<std::time::Instant>`
   - `pub fn poll_external_changes(&mut self)`:
     - Llamado desde `EditorApp::update` con debounce de 1.5 a 2.0 segundos.
     - Verifica mtime de `.git/HEAD` y `.git/index`. Si alguno cambió respecto al memorizado, refresca `refresh_git_status()` y `refresh_branch()`.
     - Para cada pestaña en `self.doc.tabs`:
       - Compara mtime del archivo en disco con `tab.last_disk_mtime`.
       - Si el mtime cambió:
         - Si `!tab.is_dirty`: lee el archivo de disco, actualiza `tab.buffer`, `tab.last_disk_mtime`, y si es el tab activo sincroniza `self.doc.buffer`.
         - Si `tab.is_dirty`: notifica `self.doc.status = "File modified externally on disk"`.
3. Comandos en Command Palette:
   - `git:view_diff` ("Git: View Diff of Current File")
   - `git:close_diff` ("Git: Close Diff Viewer")
4. Tests unitarios:
   - `poll_external_changes` recarga pestaña limpia cuando el archivo en disco cambia.
   - `poll_external_changes` respeta pestaña dirty sin pisar ediciones locales.
   - `open_git_diff` puebla `git_diff_modal` correctamente.

---

## Tarea C: Renderizado UI del Visor de Diff y Botón en Source Control

### Archivos Afectados
- `karkinos/src/ui.rs`

### Especificación Técnica
1. En `update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame)`:
   - Invocar `self.poll_external_changes()` al inicio de cada frame.
2. En `render_git_tab`:
   - En cada fila de archivo modificado `entry`:
     - Agregar botón pequeño `↔` con tooltip `"View Diff"`.
     - Al hacer clic, invoca `self.open_git_diff(&entry.path, entry.staged)`.
3. Renderizar ventana modal de diff cuando `self.git_diff_modal` es `Some`:
   - `egui::Window::new(format!("Git Diff — {}", diff_state.rel_name))`
   - Título con botón cerrar `✕`.
   - ScrollArea con texto monoespaciado:
     - Líneas `Added`: fondo verde translúcido (`Color32::from_rgba_unmultiplied(40, 160, 60, 45)`), texto verde claro.
     - Líneas `Deleted`: fondo rojo translúcido (`Color32::from_rgba_unmultiplied(200, 40, 40, 45)`), texto rojo claro.
     - Líneas `HunkHeader`: texto cyan (`Color32::from_rgb(100, 200, 255)`).
     - Líneas `Header` y `Context`: texto tenue o normal.
   - Cerrar modal con tecla `Escape` o clic en `✕`.
