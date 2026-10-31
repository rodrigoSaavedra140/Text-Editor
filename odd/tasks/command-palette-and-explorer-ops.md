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

## Tarea A: Command Palette (`Ctrl+Shift+P` / `F1`) (v0.2.40)

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
