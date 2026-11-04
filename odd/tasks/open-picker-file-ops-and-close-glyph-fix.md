# Roadmap v0.2.52: Open Picker File/Folder Creation & Cross-Platform Close Glyphs

## Contexto & Objetivos
1. **Creación de archivos y carpetas desde el botón Open**:
   Al desplegar el selector de archivos "📂 Open…", el usuario actualmente solo puede navegar por el árbol existente o abrir archivos. Se debe permitir crear carpetas y archivos directamente dentro del popup, refrescando el listado y abriendo los archivos creados automáticamente en pestañas nuevas.
2. **Corrección de glifos de cierre (tofu/cuadrados en Linux/Arch)**:
   Múltiples botones de cierre y pestañas usaban caracteres Unicode específicos como `✕` (`\u{2715}`) y `×` (`\u{00D7}`). En distribuciones Linux ligeras o sistemas con fuentes estándar (como Arch Linux), estos glifos no existen en el mapa tipográfico de `egui`, resultando en un cuadrado vacío (tofu). Deben reemplazarse por trazos vectoriales independientes de fuentes (`painter.line_segment`) o caracteres ASCII universales (`"x"`).

---

## Tarea A: Creación de Archivos y Carpetas en `show_picker_popup`

### Archivos Afectados
- `karkinos/src/app.rs`:
  - Definir `PickerNewItemKind`:
    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PickerNewItemKind {
        File,
        Folder,
    }
    ```
  - Agregar a `EditorApp`:
    - `pub picker_new_kind: Option<PickerNewItemKind>`
    - `pub picker_new_name: String`
  - Implementar método `pub fn picker_create_item(&mut self) -> Result<std::path::PathBuf, String>`:
    - Valida nombre con `is_valid_explorer_item_name`.
    - Resuelve `path = self.doc.browse_dir.join(&name)`.
    - Comprueba si ya existe.
    - Crea archivo o carpeta en disco.
    - Si es archivo: abre `open_path`, limpia modal/popup (`self.picker_open = false`, `self.picker_new_kind = None`).
    - Si es carpeta: crea el directorio, refresca `dir_cache`, limpia input y mantiene el selector abierto para continuar navegando.
- `karkinos/src/ui.rs`:
  - En `show_picker_popup`:
    - Renderizar botones `📄+ New File` y `📁+ New Folder`.
    - Si `picker_new_kind.is_some()`, mostrar campo de texto con auto-foco, tecla `Enter` para confirmar y botón `x` / tecla `Escape` para cancelar.

---

## Tarea B: Sustitución de Glifos Unicode `✕` / `×` por Trazos Vectoriales y ASCII `"x"`

### Archivos Afectados
- `karkinos/src/ui.rs`:
  - Botón de cierre de pestaña (`tab` en la barra superior):
    Sustituir `ui.painter().text("×")` por trazo vectorial de 2 segmentos (`ui.painter().line_segment`) idéntico al motor de `title_bar.rs`.
  - Pestañas de la consola/terminal:
    Sustituir `ui.small_button("×")` por `ui.small_button("x")`.
  - Encabezados de paneles y modales:
    Sustituir `"✕"` por `"x"` en Explorer, Workspace Search, Git Diff, Symbols, Extensions, Find/Replace, Terminal Dock y Cargo Dock.

---

## Criterios de Aceptación
1. `cargo check --workspace` y `cargo clippy --workspace --all-targets` con 0 warnings.
2. `cargo test --workspace` con 100% de tests pasando.
3. Tests unitarios en `app.rs` validando la creación de archivos y carpetas desde el selector.
4. Cero glifos en forma de cuadrado en cualquier botón de cierre de la interfaz.
