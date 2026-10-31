# Roadmap: Ergonomía Total — Command Palette y Operaciones de Archivos (v0.2.40 - v0.2.41)

**Objetivo**: Dotar a Karkinos IDE del centro neurálgico de productividad moderno:
1. **Command Palette (`Ctrl+Shift+P` / `F1`)**: Modal de comandos unificado con búsqueda fuzzy, categorización de acciones (Editor, Layout, Cargo, DAP, Git, Theme) y ejecución instantánea.
2. **Operaciones de Archivos en el Explorer**: Creación de nuevos archivos y carpetas (`+📄`, `+📁`), renombrado y eliminación rápida desde la barra lateral.

**Branch**: `main` (conventional commits por unidad).
**Versiones**:
- `v0.2.40`: Command Palette (`Ctrl+Shift+P` / `F1`).
- `v0.2.41`: Operaciones de archivo en Explorer (`+ File`, `+ Folder`).
**Calidad**: Cero compiler warnings (`cargo check --workspace`), 100% tests verdes (`cargo test --workspace`).

---

## Tarea B: Operaciones de Archivos en Explorer (`+ File`, `+ Folder`, `Delete`) (v0.2.41)

### 1. Modelado de Estado (`karkinos/src/app.rs`)
Extender `ExplorerState` con los campos necesarios para la creación de archivos y carpetas:
```rust
#[derive(Debug, Default)]
pub struct ExplorerState {
    pub open: bool,
    pub files: Vec<std::path::PathBuf>,
    pub loaded_for: Option<std::path::PathBuf>,
    pub collapsed_dirs: std::collections::HashSet<std::path::PathBuf>,
    /// Diálogo de creación activo: None, Some(ItemKind::File), Some(ItemKind::Folder)
    pub creating_item: Option<ExplorerNewItemKind>,
    pub new_item_name: String,
    pub new_item_target_dir: Option<std::path::PathBuf>,
    pub item_to_delete: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerNewItemKind {
    File,
    Folder,
}
```

Métodos en `EditorApp`:
- `create_explorer_item(&mut self) -> Result<std::path::PathBuf, String>`:
  - Crea el archivo o directorio en disco dentro de `target_dir` (o `browse_dir` por defecto).
  - Si es archivo, lo abre de inmediato en el editor (`self.open_path(path)`).
  - Refresca el explorador con `self.refresh_explorer()`.
  - Notifica en `self.doc.status`.
- `delete_explorer_file(&mut self, path: &std::path::Path) -> Result<(), String>`:
  - Elimina el archivo de disco (`std::fs::remove_file`).
  - Si estaba abierto en alguna pestaña, la cierra.
  - Refresca el explorador.

### 2. Renderizado en UI (`karkinos/src/ui.rs`)
En `render_explorer_tab`:
- En la barra de herramientas del encabezado del Explorer:
  - Botón `+📄` con hover `New File`.
  - Botón `+📁` con hover `New Folder`.
- Si `creating_item.is_some()`:
  - Renderizar una fila destacada de entrada con fondo `elevated_bg`, borde sutil y foco automático:
    `[ 📄/📁 ] [ TextEdit input ] [ ✓ Ok ] [ ✕ Cancel ]`
  - Presionar `Enter` ejecuta la creación; `Escape` cancela.
- En cada fila de archivo del árbol:
  - Si el mouse está sobre la fila (hovered), mostrar a la derecha un botón sutil `✕` o `🗑` para eliminar el archivo con confirmación o click directo.


### 1. Modelado de Comandos (`karkinos/src/app.rs`)
Definir la estructura de una acción registrable en el Command Palette:
```rust
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str, // "Editor", "View", "Cargo", "Debug", "Layout", "Theme"
    pub shortcut: Option<&'static str>,
}

#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}
```

Catálogo estático de comandos disponibles:
- `layout:split_vertical`: "View: Split Editor Vertically" (`Ctrl+\`)
- `layout:split_horizontal`: "View: Split Editor Horizontally" (`Ctrl+Shift+\`)
- `layout:close_split`: "View: Close Split Editor"
- `editor:toggle_minimap`: "View: Toggle Minimap" (`Ctrl+Shift+M`)
- `sidebar:explorer`: "View: Show Explorer" (`Ctrl+Shift+E`)
- `sidebar:search`: "View: Search in Workspace" (`Ctrl+Shift+F`)
- `sidebar:symbols`: "View: Show Document Symbols" (`Ctrl+Shift+O`)
- `terminal:toggle`: "Terminal: Toggle Terminal Dock" (``Ctrl+` ``)
- `cargo:check`: "Cargo: Run Check" (`F7`)
- `cargo:test`: "Cargo: Run Tests"
- `cargo:clippy`: "Cargo: Run Clippy"
- `cargo:run`: "Cargo: Run Project" (`F5`)
- `debug:toggle_breakpoint`: "Debug: Toggle Breakpoint" (`F9`)
- `debug:start`: "Debug: Start Debugging" (`F5`)
- `debug:step_over`: "Debug: Step Over" (`F10`)
- `debug:step_into`: "Debug: Step Into" (`F11`)
- `debug:stop`: "Debug: Stop Debugging"
- `theme:dark`: "Preferences: Theme: Dark (Graphite)"
- `theme:monokai`: "Preferences: Theme: Monokai"
- `theme:dracula`: "Preferences: Theme: Dracula"
- `editor:format`: "Editor: Format Document with rustfmt"
- `file:save`: "File: Save Current File" (`Ctrl+S`)
- `file:new`: "File: New Untitled File" (`Ctrl+N`)

### 2. Ejecución y Filtrado Fuzzy (`karkinos/src/app.rs`)
- `execute_palette_command(&mut self, id: &str)`: ejecuta el método correspondiente en `EditorApp` y cierra la paleta.
- `filtered_palette_commands(&self) -> Vec<&PaletteCommand>`: filtra por coincidencia insensible a mayúsculas sobre `category` y `title`.
- Navegación con flechas `↑`/`↓`, `Enter` para ejecutar, `Escape` para cerrar.

### 3. Renderizado del Modal (`karkinos/src/ui.rs`)
En `show_command_palette`:
- Ventana centrada en la parte superior: `egui::Window::new("command_palette_modal")` con anchor `CENTER_TOP`, offset Y de 60px, ancho fijo de 560px.
- TextEdit con prompt `> Type a command...` y foco automático al abrir (`Ctrl+Shift+P` o `F1`).
- Lista desplazable:
  - Fila con altura de 26px, hover y selección activa resaltada con fondo `theme.active_line`.
  - Icono o categoría tenue (`theme.text_muted`).
  - Título en `theme.text`.
  - Atajo a la derecha en tipografía monoespaciada en chip sutil (`theme.badge_bg`).

---

## Log

- L1 (2026-10-31): "Lee odd/tasks/command-palette-and-explorer-ops.md e implementa la Tarea B: Operaciones de Archivos en Explorer (+ File, + Folder, Delete) (v0.2.41). Modifica karkinos/src/app.rs y karkinos/src/ui.rs agregando los botones de nuevo archivo y carpeta, el input inline con Enter/Escape, la creacion en disco, apertura inmediata si es archivo, y boton de borrado con delete_explorer_file. Agrega tests unitarios para create_explorer_item y delete_explorer_file, verifica con cargo test --workspace, e incrementa la version a 0.2.41 en Cargo.toml."
- 2026-10-31 — Tarea B implementada: `ExplorerState` extendido (`creating_item`, `new_item_name`, `new_item_target_dir`, `item_to_delete`, `new_item_focus`), `create_explorer_item` (nombre de un solo componente, no sobrescribe existentes, abre el archivo creado) y `delete_explorer_file` (borra en disco, cierra su pestaña y refresca). UI: botones `+📄`/`+📁`, fila inline con Enter/✓ y Escape/✕, confirmación de borrado y `🗑` por fila. 5 tests nuevos. `cargo check --workspace` sin warnings; `cargo test --workspace`: 153 + 86 verdes. Versión 0.2.41 en ambos manifests y `Cargo.lock`.
- Nota: `refresh_explorer` corta temprano si `loaded_for` no cambió; creación y borrado lo invalidan antes de refrescar. Las carpetas vacías no aparecen en el árbol porque se construye desde la lista de archivos.
