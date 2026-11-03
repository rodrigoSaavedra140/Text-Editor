# Roadmap v0.2.42: Git Source Control Sidebar & Empty Folder Visibility

## Contexto & Objetivos
1. **Empty Folder Visibility en Explorer**:
   - Hasta v0.2.41, el explorador construía su árbol únicamente con `walk_files(&root)` que filtraba `is_file()`.
   - Las carpetas vacías (recién creadas con `+📁` o clonadas sin contenido) no aparecían hasta que se guardara un archivo dentro.
   - Solución: Recolectar también directorios (`walk_entries`), y permitir que `FileTreeNode` represente carpetas vacías visibles con indicador tenue `(empty)` o carpeta vacía sin hijos.

2. **Git Source Control Sidebar Tab (`SidebarTab::SourceControl`)**:
   - Integración nativa de Git en la barra lateral unificada y Activity Bar.
   - Atajo global: `Ctrl+Shift+G` para abrir directamente la pestaña de Source Control.
   - Icono en Activity Bar: `` / `🌿` ("Source Control").
   - Detección de cambios mediante `git status --porcelain`.
   - Clasificación de cambios: Modificados (`M`), Agregados (`A`), Borrados (`D`), Sin seguimiento (`?`/`U`).
   - Clic en cualquier archivo modificado abre el archivo en el editor de inmediato.
   - Caja de commit con mensaje y botón de confirmación ("Commit").
   - Feedback en la barra de estado o toast tras el commit.

---

## Tarea A: Soporte de Carpetas Vacías en `karkinos-core` y Explorer

### Archivos Afectados
- `crates/karkinos-core/src/workspace_search.rs`
- `karkinos/src/app.rs`
- `karkinos/src/ui.rs`

### Especificación Técnica
1. En `crates/karkinos-core/src/workspace_search.rs`:
   - Agregar función pública:
     ```rust
     pub fn walk_entries(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>)
     ```
     que recorre el directorio excluyendo `DEFAULT_EXCLUDED_DIRS` y devuelve `(files, directories)`.
     Preservar `walk_files` para retrocompatibilidad delegando en `walk_entries`.
2. En `karkinos/src/app.rs`:
   - Actualizar `FileTreeNode::build_from_paths_and_dirs(root: &Path, files: &[PathBuf], dirs: &[PathBuf]) -> Vec<FileTreeNode>` (o mantener `build_from_paths` y añadir constructor unificado).
   - Asegurar que si una carpeta no tiene archivos hijos, igual se genera un `FileTreeNode::Directory { name, path, children: vec![] }`.
   - En `ExplorerState`:
     - Guardar también `pub directories: Vec<std::path::PathBuf>`.
     - En `refresh_explorer()`: llamar a `walk_entries(&root)` para poblar archivos y directorios.
3. En `karkinos/src/ui.rs`:
   - Si `children.is_empty()`, renderizar la carpeta con icono `📁`, permitir expandir mostrando un renglón tenue `(empty)` si está expandida, y permitir hacer clic en `+📄` o `🗑`.

---

## Tarea B: Motor de Git Status y Operaciones en `karkinos-core`

### Archivos Afectados
- `crates/karkinos-core/src/git_status.rs`

### Especificación Técnica
1. Tipos de datos:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum GitStatusKind {
       Modified,
       Added,
       Deleted,
       Untracked,
       Renamed,
       Other,
   }

   #[derive(Debug, Clone, PartialEq, Eq)]
   pub struct GitStatusEntry {
       pub path: std::path::PathBuf,
       pub rel_path: String,
       pub staged: bool,
       pub kind: GitStatusKind,
   }
   ```
2. Funciones de parsing y ejecución:
   - `pub fn parse_porcelain_status(output: &str, repo_root: &std::path::Path) -> Vec<GitStatusEntry>`:
     - Procesa cada línea de `git status --porcelain`.
     - Si la línea empieza con `?? `, es `GitStatusKind::Untracked`, `staged: false`.
     - Si el primer carácter es `M`, `A`, `D`, `R`, es staged.
     - Si el segundo carácter es `M`, `D`, es unstaged.
     - Maneja rutas con renombres `R  old -> new` tomando la nueva ruta.
   - `pub fn query_git_status(repo_root: &std::path::Path) -> Result<Vec<GitStatusEntry>, String>`:
     - Lanza `git status --porcelain=v1` en `repo_root`.
   - `pub fn execute_git_commit(repo_root: &std::path::Path, message: &str) -> Result<String, String>`:
     - Valida que `message.trim()` no esté vacío.
     - Ejecuta `git commit -m <message>`.
   - `pub fn execute_git_stage_all(repo_root: &std::path::Path) -> Result<(), String>`:
     - Ejecuta `git add -A`.

---

## Tarea C: Pestaña Source Control en UI y Atajos de Teclado

### Archivos Afectados
- `karkinos/src/app.rs`
- `karkinos/src/ui.rs`

### Especificación Técnica
1. En `karkinos/src/app.rs`:
   - Añadir variante a `SidebarTab`:
     ```rust
     #[derive(Debug, Clone, Copy, PartialEq, Eq)]
     pub enum SidebarTab {
         Explorer,
         Search,
         SourceControl,
         Symbols,
     }
     ```
   - Añadir estructura `GitUiState`:
     ```rust
     #[derive(Default)]
     pub struct GitUiState {
         pub entries: Vec<karkinos_core::git_status::GitStatusEntry>,
         pub commit_message: String,
         pub is_loading: bool,
         pub last_error: Option<String>,
     }
     ```
   - Métodos en `EditorApp`:
     - `pub fn refresh_git_status(&mut self)`
     - `pub fn git_commit(&mut self) -> Result<String, String>`
     - `pub fn git_stage_all(&mut self) -> Result<(), String>`
   - Atajo `Ctrl+Shift+G`: conmuta `SidebarTab::SourceControl` y llama a `refresh_git_status()`.
2. En `karkinos/src/ui.rs`:
   - En la Activity Bar:
     - Renderizar botón `🌿` o `` para `SidebarTab::SourceControl` con tooltip `Source Control (Ctrl+Shift+G)`.
     - Si hay cambios pendientes, mostrar badge numérico pequeño con la cantidad de archivos.
   - En `unified_sidebar_panel`:
     - Título: `SOURCE CONTROL`.
     - Cabecera con nombre de la rama actual y botón `🔄` (Refresh).
     - Caja de texto para commit message (`TextEdit::singleline` con placeholder).
     - Botón "Commit" y botón "+ Stage All".
     - Lista de cambios agrupados o listados con badge de color:
       - `M` (Amarillo / Azul)
       - `A` (Verde)
       - `D` (Rojo)
       - `U` (Naranja / Púrpura)
     - Clic en una fila abre el archivo con `self.open_path_in_tab(entry.path)`.
3. Comandos en Command Palette:
   - `git:source_control` -> "Git: Open Source Control"
   - `git:refresh` -> "Git: Refresh Status"
   - `git:stage_all` -> "Git: Stage All Changes"

---

## Log

- **2026-10-31 — Tarea A completada**: `walk_entries(root) -> (Vec<PathBuf>, Vec<PathBuf>)` en `crates/karkinos-core/src/workspace_search.rs` recorre archivos y directorios excluyendo `DEFAULT_EXCLUDED_DIRS`; `walk_files` queda como delegado de `.0`. `FileTreeNode::build_from_paths_and_dirs` representa carpetas sin archivos hijos, `ExplorerState.directories` guarda los directorios y `refresh_explorer` usa `walk_entries`. La UI dibuja la carpeta vacía con `📁` y una fila tenue `(empty)`. Test: `walk_entries_reports_files_and_empty_directories`.
- **2026-10-31 — Tareas B y C completadas**: `crates/karkinos-core/src/git_status.rs` con `GitStatusKind`, `GitStatusEntry`, `parse_porcelain_status`, `query_git_status`, `execute_git_commit` y `execute_git_stage_all` (porcelain v1 `XY<space>path`, `??` para untracked y renombres `old -> new`). En la app: `SidebarTab::SourceControl`, `GitUiState`, `refresh_git_status`/`git_commit`/`git_stage_all`, atajo `Ctrl+Shift+G` (excluyendo el `Ctrl+G` de goto-line) y comandos `git:source_control`/`git:refresh`/`git:stage_all`. UI: botón 🌿 con badge de cambios, cabecera con rama y refresco, caja de commit, Commit/+ Stage All, badges de color por estado y clic para abrir el archivo. Evidencia: `cargo check --workspace` 0 warnings; 159 tests de `karkinos` + 96 de `karkinos-core` verdes (v0.2.42).
