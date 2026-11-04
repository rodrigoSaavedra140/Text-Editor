# Roadmap v0.2.49: Explorer File Operations v2 (In-Place Renaming `F2`, Duplication & Active Tab Sync)

## Contexto & Objetivos
En v0.2.41 se introdujeron las operaciones de creación (`+ File`, `+ Folder`) y borrado en el explorador lateral, pero quedó pendiente la capacidad de **renombrar archivos y carpetas** y **duplicar archivos**.

Cuando un desarrollador renombra un archivo o carpeta desde el editor:
1. El archivo o carpeta debe renombrarse en disco con `std::fs::rename`.
2. Si el archivo está abierto en una o más pestañas del editor, las pestañas deben actualizar atómicamente su ruta y su título sin perder el buffer, el estado de undo ni el estado dirty.
3. Si se renombra una **carpeta**, todas las pestañas abiertas cuyos archivos residan dentro de esa carpeta deben reescribir su ruta prefijo.
4. El explorador y el árbol Git deben refrescarse de inmediato.
5. Duplicar archivo crea una copia fiel (`<name>_copy.<ext>`) y la abre en una nueva pestaña.

---

## Tarea A: Motor de Renombrado y Duplicado en `karkinos/src/app.rs`

### Archivos Afectados
- `karkinos/src/app.rs`

### Especificación Técnica
1. Estado en `ExplorerState`:
   ```rust
   pub renaming_item: Option<(std::path::PathBuf, String)>,
   ```
2. Métodos en `EditorApp`:
   - `pub fn begin_rename_explorer_item(&mut self, path: &std::path::Path)`:
     - Activa `self.explorer.renaming_item = Some((path.to_path_buf(), file_name))`.
   - `pub fn cancel_rename_explorer_item(&mut self)`:
     - `self.explorer.renaming_item = None;`
   - `pub fn confirm_rename_explorer_item(&mut self) -> Result<std::path::PathBuf, String>`:
     - Toma `(old_path, new_name)`.
     - Valida `new_name` (no vacío, sin `/` ni `\`, trim).
     - Construye `new_path = old_path.parent().unwrap_or(&self.doc.browse_dir).join(trimmed)`.
     - Si `new_path == old_path`, cancela sin error.
     - Si `new_path.exists()`, retorna `Err("Target already exists".into())`.
     - Ejecuta `std::fs::rename(&old_path, &new_path)`.
     - **Sincronización de pestañas**:
       - Si `old_path` era archivo:
         - Para cada `tab` en `self.doc.tabs`:
           - Si `tab.current_file.as_deref() == Some(&old_path)`:
             - `tab.current_file = Some(new_path.clone());`
             - `tab.title = trimmed.to_string();`
         - Si `self.doc.current_file.as_deref() == Some(&old_path)`:
           - `self.doc.current_file = Some(new_path.clone());`
           - `self.doc.file_path_input = trimmed.to_string();`
       - Si `old_path` era carpeta:
         - Para cada `tab` en `self.doc.tabs`:
           - Si el archivo del tab comienza con `old_path`:
             - Reemplaza el prefijo `old_path` por `new_path`.
         - Igual para `self.doc.current_file`.
     - Refresca explorador (`refresh_explorer()`) y Git (`refresh_git_status()`, `refresh_git_branch()`).
     - Notifica en `self.doc.status = format!("Renamed: {} -> {}", old_name, trimmed)`.
   - `pub fn explorer_duplicate_item(&mut self, path: &std::path::Path) -> Result<std::path::PathBuf, String>`:
     - Genera un nombre disponible `<stem>_copy.<ext>` o `<stem>_copy_<N>.<ext>`.
     - Copia con `std::fs::copy(path, &dest)`.
     - Refresca explorador y abre el archivo duplicado (`self.open_path(dest)`).
3. Comandos en `PALETTE_COMMANDS`:
   - `id: "file:rename_active"`, `title: "File: Rename Active File"`, `shortcut: Some("F2")`.
   - `id: "file:duplicate_active"`, `title: "File: Duplicate Active File"`, `shortcut: None`.
4. Tests unitarios:
   - Renombrar archivo en disco sincroniza pestañas abiertas y actualiza título.
   - Renombrar carpeta actualiza pestañas anidadas dentro de esa carpeta.
   - Renombrar a nombre existente o inválido es rechazado limpiamente.
   - Duplicar archivo genera copia libre y la abre en pestaña activa.

---

## Tarea B: Integración en UI del Explorador (`ui.rs`)

### Archivos Afectados
- `karkinos/src/ui.rs`

### Especificación Técnica
1. En `render_explorer_tab`:
   - En cada fila de archivo y carpeta:
     - Mostrar botón `✏` con tooltip `"Rename (F2)"` al lado del botón de borrar `✕`.
   - Si `self.explorer.renaming_item` tiene el path de la fila:
     - Renderizar campo inline con `TextEdit::singleline` enfocado automáticamente:
       `[ ✏ ] [ TextEdit ] [ ✓ ] [ ✕ ]`
     - Presionar `Enter` o click en `✓` ejecuta `confirm_rename_explorer_item()`.
     - Presionar `Escape` o click en `✕` cancela con `cancel_rename_explorer_item()`.
2. Atajo `F2`:
   - Si el editor tiene el foco y hay un archivo activo en `self.doc.current_file`, presionar `F2` abre el renombrado.

---

## Tarea C: Versión v0.2.49 y Verificación

### Archivos Afectados
- `karkinos/Cargo.toml` (bump a `0.2.49`)
- `crates/karkinos-core/Cargo.toml` (bump a `0.2.49`)
- `scripts/PKGBUILD`
- `docs/index.html`

### Especificación Técnica
1. Sincronizar versión `0.2.49` en Cargo.toml, Cargo.lock, PKGBUILD y docs/index.html.
2. `cargo check --workspace` y `cargo clippy --workspace --all-targets` con 0 warnings.
3. `cargo test --workspace` al 100% verde.

---

## Tasks

- **Tarea A** — Motor de renombrado y duplicado en `karkinos/src/app.rs` — ruta: inline — estado: ✅ — specs: S-A (secciones Tarea A arriba)
- **Tarea B** — Integración UI del explorador en `karkinos/src/ui.rs` — ruta: inline — estado: ✅ — specs: S-B
- **Tarea C** — Versión v0.2.49 y verificación — ruta: inline — estado: ✅ — specs: S-C

## Log

- **L1** (pedido original, verbatim): "Lee odd/tasks/explorer-file-ops-v2-rename-and-duplicate.md e implementa las Tareas A, B y C completas. Sube la version a 0.2.49 en ambos Cargo.toml, Cargo.lock, scripts/PKGBUILD y docs/index.html. Verifica que cargo clippy --workspace --all-targets termine con 0 warnings y cargo test --workspace pase al 100%."
- **Decisión (F2)**: `F2` se reasigna a "Renombrar archivo activo" según la spec (Tarea B.2). El Symbol Rename (LSP) existente se reubica a `Shift+F6` para no perder el atajo (asumido en corrida no interactiva; ver reporte).
- **Detalle de implementación**: `ExplorerState` gana `renaming_item: Option<(PathBuf, String)>` (y `rename_focus` para el autofoco del input). `confirm_rename_explorer_item` sincroniza `tabs` y `doc.current_file` con `sync_active_to_tab()` previo y reescribe prefijos de carpeta con `strip_prefix` (límite de componente, no prefijo de string).
- **Verificación**: `cargo check --workspace` ✅ · `cargo clippy --workspace --all-targets` 0 warnings ✅ · `cargo test --workspace` 307 passed / 0 failed ✅.
