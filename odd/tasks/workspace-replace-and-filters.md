# Roadmap v0.2.43: Multi-File Search & Replace (`Ctrl+Shift+H`) & File Filter

## Contexto & Objetivos
1. **Reemplazo Global en Workspace (`Ctrl+Shift+H`)**:
   - Hasta v0.2.42, Karkinos permitía buscar texto en todo el workspace (`Ctrl+Shift+F`), pero no reemplazar entre múltiples archivos.
   - El reemplazo global es una de las herramientas de refactorización más esenciales de cualquier IDE moderno (Zed, VS Code, JetBrains).
   - Solución: un motor transaccional de reemplazo en `karkinos-core::workspace_search` con soporte de texto literal y regex, y sincronización automática con pestañas abiertas en memoria.

2. **Filtro de Archivos (`files to include`)**:
   - Permitir restringir la búsqueda y el reemplazo a patrones de archivo específicos (e.g. `*.rs`, `*.toml`, `src/*`).

3. **Atajo y Comandos en Command Palette**:
   - `Ctrl+Shift+H`: abre la pestaña de búsqueda con el campo de reemplazo desplegado y enfocado.
   - Comandos en Command Palette: "Search: Replace in Files", "Search: Toggle Replace".

---

## Tarea A: Motor de Reemplazo y Filtros en `karkinos-core`

### Archivos Afectados
- `crates/karkinos-core/src/workspace_search.rs`

### Especificación Técnica
1. Extensión de `SearchQuery`:
   ```rust
   #[derive(Debug, Clone, Default)]
   pub struct SearchQuery {
       pub query: String,
       pub case_sensitive: bool,
       pub match_word: bool,
       pub use_regex: bool,
       pub max_results: usize,
       pub file_pattern: Option<String>,
   }
   ```
2. Helper de coincidencia de patrones de archivo:
   - `pub fn matches_file_pattern(path: &Path, pattern: &str) -> bool`:
     - Soporta extensiones (e.g. `*.rs`, `.rs`), prefijos de ruta (e.g. `src/`), o lista separada por comas (`*.rs, *.toml`).
3. Motor de reemplazo:
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq, Default)]
   pub struct ReplaceSummary {
       pub files_modified: usize,
       pub replacements_count: usize,
   }

   pub fn replace_in_string(
       content: &str,
       query: &SearchQuery,
       replacement: &str,
   ) -> Result<(String, usize), String>

   pub fn replace_in_file(
       path: &Path,
       query: &SearchQuery,
       replacement: &str,
   ) -> Result<usize, String>

   pub fn replace_in_workspace(
       root: &Path,
       query: &SearchQuery,
       replacement: &str,
   ) -> Result<ReplaceSummary, String>
   ```
   - Si `query.use_regex`: compila expresión regular (respetando `case_sensitive`) y aplica `replace_all`.
   - Si literal: reemplaza todas las ocurrencias (respetando `match_word` y `case_sensitive`).
   - `replace_in_file` solo reescribe el archivo en disco si hubo al menos un reemplazo, preservando permisos y final de línea.
   - Tests unitarios exhaustivos:
     - Reemplazo literal simple y sensible a mayúsculas.
     - Reemplazo con regex y grupos de captura.
     - Reemplazo con `match_word` evitando subcadenas.
     - Reemplazo en archivos en disco y reporte de `ReplaceSummary`.

---

## Tarea B: Integración en EditorApp y Sincronización de Buffers

### Archivos Afectados
- `karkinos/src/app.rs`

### Especificación Técnica
1. Extensión de `WorkspaceSearchState`:
   ```rust
   pub show_replace: bool,
   pub replace_input: String,
   pub file_filter: String,
   pub replace_summary: Option<String>,
   pub replace_focus_requested: bool,
   ```
2. Método en `EditorApp`:
   ```rust
   pub fn execute_workspace_replace(&mut self) -> Result<karkinos_core::workspace_search::ReplaceSummary, String>
   ```
   - Construye el `SearchQuery` actual con `file_pattern` si `file_filter` no está vacío.
   - Ejecuta `replace_in_workspace(&root, &query, &self.workspace_search.replace_input)`.
   - Itera las pestañas abiertas en `self.doc.tabs`:
     - Si un archivo modificado en disco está abierto en una pestaña, recarga su buffer o actualiza su contenido para reflejar la modificación sin perder la coherencia del mtime ni del historial.
   - Refresca la búsqueda activa para actualizar la lista de coincidencias.
   - Publica feedback en `self.status_message` y `self.workspace_search.replace_summary`.
3. Comandos en Command Palette:
   - `search:replace_in_workspace` ("Search: Replace in Files (Ctrl+Shift+H)")
   - `search:toggle_replace` ("Search: Toggle Replace Field")

---

## Tarea C: Interfaz UI y Atajos de Teclado

### Archivos Afectados
- `karkinos/src/ui.rs`

### Especificación Técnica
1. Atajo global:
   - `Ctrl+Shift+H`: conmuta a `SidebarTab::Search`, asegura `left_sidebar_visible = true`, pone `show_replace = true`, y solicita foco en el campo de reemplazo.
2. Renderizado en `render_search_tab`:
   - Botón toggle `▶` / `▼` al lado del encabezado o barra de búsqueda.
   - Campo de entrada para Replace con placeholder `"Replace... (Enter / Replace All)"`.
   - Botón `Replace All` con icono `🔄` o `Replace All` (con tooltip explicativo).
   - Campo colapsable o secundario `Files to include` (e.g. `*.rs, *.toml`).
   - Notificación de resultado en caso de éxito o error.
