# Roadmap: Next-Gen IDE Features — Fase 2 (F6 - F10)

**Objetivo**: Continuar la evolución de Karkinos IDE hacia el nivel de ergonomía de Zed y Cursor, implementando las 5 mejoras arquitectónicas de alto ROI definidas tras el release `v0.2.31`.
**Branch**: `main` (conventional commits por unidad de trabajo).
**Versionado**: Bumping estricto de dígitos menores/parche (`0.X.Y`), conservando siempre el primer dígito en `0`.
**Calidad**: Cero compiler warnings (`cargo check --workspace`), 100% tests pasando (`cargo test --workspace`).

---

## Matriz de Tareas

| ID | Feature | Cajas / Módulos Afectados | Complejidad | Versión Estimada |
| :--- | :--- | :--- | :--- | :--- |
| **F6** | **Búsqueda Global en Workspace (`Ctrl+Shift+F`)** | `karkinos-core::workspace_search`, `karkinos::ui`, `app.rs` | Media-Alta | `v0.2.32` |
| **F7** | **Selector de Símbolos (`Ctrl+Shift+O` / `@`)** | `karkinos-core::syntax_ast`, `karkinos::app` | Media | `v0.2.33` |
| **F8** | **Git Blame Inline & Branch Status** | `karkinos-core::git_status`, `karkinos::editor_widget`, `ui.rs` | Media | `v0.2.34` |
| **F9** | **Minimap Lateral de Código** | `karkinos::editor_widget` | Media | `v0.2.35` |
| **F10** | **Terminal Multi-tab** | `karkinos-core::terminal`, `karkinos::app`, `ui.rs` | Media-Alta | `v0.2.36` |

---

## Blueprint de Implementación Detallado

### Tarea F6: Búsqueda Global en Workspace (`Ctrl+Shift+F`)

#### 1. Concepto
Un panel lateral ("Buscar") accesible desde la barra lateral o con `Ctrl+Shift+F`. Permite buscar texto o expresiones regulares en todos los archivos de texto del proyecto abierto, respetando exclusiones estándar (`target/`, `.git/`, `node_modules/`).

#### 2. Componente Core (`crates/karkinos-core/src/workspace_search.rs`)
- Estructuras:
  ```rust
  #[derive(Debug, Clone)]
  pub struct SearchQuery {
      pub query: String,
      pub case_sensitive: bool,
      pub match_word: bool,
      pub use_regex: bool,
      pub max_results: usize, // e.g. 500 para evitar saturación
  }

  #[derive(Debug, Clone)]
  pub struct SearchMatch {
      pub line_number: usize,
      pub col_start: usize,
      pub col_end: usize,
      pub line_content: String,
  }

  #[derive(Debug, Clone)]
  pub struct FileMatches {
      pub path: std::path::PathBuf,
      pub relative_display: String,
      pub matches: Vec<SearchMatch>,
  }
  ```
- Motor de búsqueda asíncrono con canal `crossbeam-channel` o `std::sync::mpsc`:
  - Recorre el directorio raíz usando `ignore::WalkBuilder` (o `walkdir` filtrando `.git`, `target`, `node_modules`).
  - Emite resultados progresivos en streaming para que la UI los muestre en tiempo real sin bloquear.

#### 3. Integración en UI (`karkinos/src/ui.rs` & `app.rs`)
- Agregar icono de lupa en la barra lateral izquierda (activa vista `SidePanelTab::Search`).
- Atajo global `Ctrl+Shift+F` enfoca el campo de búsqueda global.
- UI:
  - Input field con debounce de 250ms o trigger al presionar Enter.
  - Toggles: Case Sensitive (`Aa`), Whole Word (`\b`), Use Regex (`.*`).
  - Árbol colapsable: Encabezado por archivo `nombre_archivo.rs (N coincidencias)` y lista de renglones coincidentes.
  - Al hacer clic en un resultado, abre la pestaña correspondiente y posiciona el cursor en la línea y columna exactas.

---

### Tarea F7: Selector de Símbolos de Documento (`Ctrl+Shift+O` / `@`)

#### 1. Concepto
Navegación instantánea a funciones, structs, enums, traits y métodos dentro del documento activo sin necesidad de LSP externo, utilizando el motor Tree-sitter que ya tenemos en memoria.

#### 2. Extracción de Símbolos (`crates/karkinos-core/src/syntax_ast.rs`)
- Extender `AstEngine` para extraer símbolos estructurados:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum SymbolKind {
      Function,
      Struct,
      Enum,
      Trait,
      Method,
      Constant,
  }

  #[derive(Debug, Clone)]
  pub struct DocumentSymbol {
      pub name: String,
      pub kind: SymbolKind,
      pub line: usize,
      pub character: usize,
  }

  impl AstEngine {
      pub fn extract_document_symbols(&self, source: &str) -> Vec<DocumentSymbol> {
          // Recorre el árbol AST buscando nodos tipo function_item, struct_item, enum_item, etc.
      }
  }
  ```

#### 3. Quick Open Integration (`karkinos/src/app.rs`)
- Al presionar `Ctrl+Shift+O` o al tipear `@` en el Quick Open (`Ctrl+P`):
  - El modal pasa a modo "Símbolos del Documento".
  - Filtro fuzzy por nombre del símbolo.
  - Badge visual con ícono/letra (`ƒ` función, `S` struct, `E` enum, `T` trait).
  - Al navegar con flechas o presionar Enter, el editor scrollea suavemente al símbolo seleccionado.

---

### Tarea F8: Git Blame Inline & Branch Status

#### 1. Concepto
- **Barra de estado**: Mostrar la rama actual activa (` main`) y conteo de archivos modificados/staged.
- **Ghost text en el editor**: Al posar el cursor en una línea, mostrar texto tenue al final de esa línea con el autor, tiempo relativo y mensaje del commit (`Linus Torvalds, 2 days ago • fix: buffer boundary`).

#### 2. Módulo Git Status (`crates/karkinos-core/src/git_status.rs`)
- Detección de rama activa leyendo `.git/HEAD` sin invocar subprocesos pesados.
- Worker no bloqueante para `git blame`:
  - Cache LRU de blame por archivo / revisión.
  - Consulta perezosa: solo solicita el blame de la línea donde el cursor permanece quieto por más de 300ms.

#### 3. Renderizado (`karkinos/src/editor_widget.rs` & `ui.rs`)
- En el footer status bar: Widget con ícono de branch e indicador de estado de sync.
- En el editor: Dibujado de texto tenue (`egui::Color32::from_gray(100)`) a la derecha del último caracter del renglón activo.

---

### Tarea F9: Minimap Lateral de Código

#### 1. Concepto
Una franja lateral derecha (~100-120px) que proporciona una visión panorámica de todo el archivo para orientación espacial rápida y scroll instantáneo.

#### 2. Algoritmo de Renderizado (`karkinos/src/editor_widget.rs`)
- Relación de aspecto: Cada línea de código equivale a 2px o 3px de alto en el minimap.
- Renderizado sintético rápido:
  - En lugar de dibujar texto con fuentes completas, dibujar micro-rectángulos o líneas horizontales proporcionales al largo de los tokens y con sus colores de sintaxis.
  - Omitir espacios en blanco.
- Rectángulo de Viewport:
  - Resalta en color translúcido (`rgba(255, 255, 255, 0.08)`) el rango de líneas actualmente visible en el viewport principal.
- Interactividad:
  - Click o arrastre en el minimap reposiciona el scroll vertical proporcionalmente en tiempo real.

---

### Tarea F10: Pestañas Múltiples en la Terminal PTY

#### 1. Concepto
Evolucionar el panel inferior PTY (`PtyTerminal`) para admitir múltiples pestañas de consola independientes (`Terminal 1`, `Terminal 2`, `htop`, `cargo watch`), permitiendo crear, renombrar y cerrar consolas interactivas sin perder su estado.

#### 2. Arquitectura de Pestañas (`crates/karkinos-core/src/terminal.rs` & `karkinos/src/app.rs`)
- Estructura:
  ```rust
  pub struct TerminalInstance {
      pub id: usize,
      pub title: String,
      pub pty: PtyTerminal,
      pub cache: TerminalSnapshotCache,
  }

  pub struct TerminalDockState {
      pub instances: Vec<TerminalInstance>,
      pub active_index: usize,
  }
  ```
- Ciclo de vida:
  - Botón `+` en el encabezado del dock para instanciar un nuevo PTY en background.
  - Botón `x` o atajo `Ctrl+Shift+W` para matar el shell y liberar recursos.
  - Tabs conmutables con un solo click. Cero overhead en pestañas inactivas (no renderizan snapshot si están ocultas).

---

## Protocolo de Ejecución Recomendado

1. **Implementar una tarea a la vez** respetando la arquitectura hexagonal de Karkinos.
2. Mantener la suite de tests al 100% verde (`cargo test --workspace`).
3. Commit convencional por tarea (ej. `feat(search): implement multi-threaded workspace search (v0.2.32)`).
4. Incrementar la versión en `Cargo.toml` (`0.2.32`, `0.2.33`, etc.).

---

## Log de Ejecución

- **F6 (v0.2.32) — completada**: `crates/karkinos-core/src/workspace_search.rs` con `SearchQuery` (texto, mayúsculas, palabra completa, regex, tope de resultados), `SearchMatch`, `FileMatches` y un `WorkspaceSearch` que recorre el árbol en un hilo propio emitiendo resultados por archivo en streaming. El recorrido usa `std::fs` (sin dependencias nuevas) y salta `target/`, `.git/`, `.codegraph/`, `node_modules/`, `dist/`, `build/`, `.idea/` y `.vscode/`; descarta binarios (heurística de NUL) y archivos de más de 1 MB.
  - UI: panel lateral derecho con `Ctrl+Shift+F` o el botón 🔎 de la barra superior; input con debounce de 250 ms y Enter inmediato; toggles `Aa`, `\b`, `.*`; árbol colapsable por archivo con el número de coincidencias; clic en un resultado abre el archivo y posiciona cursor y selección en línea/columna exactas.
  - Corrección de carreras: cada pedido lleva generación y el `poll` descarta eventos de búsquedas anteriores.
  - Tests: 10 en el core (ocurrencias, mayúsculas, palabra completa, regex, regex inválida, tope de resultados, binarios, exclusiones del walker, streaming del worker, consulta vacía) + 3 en la app (regex inválida reportada, consulta vacía no busca, salto con cursor/selección exactos).
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 127, karkinos-core 80.
  - Desvío menor: no se agregó el icono de lupa a una barra lateral izquierda porque la app no tiene barra lateral izquierda; el acceso es el atajo, el botón de la barra superior y el ✕ del propio panel.
- **Barra lateral (v0.2.33) — completada** (seguimiento de F6): `SidePanel::left` de 44 px con explorador (📁), búsqueda global (🔎), extensiones (🧩), consola Cargo (⚡) y debugger (🐞). El explorador lista los archivos del proyecto con el mismo recorrido del buscador (sin `target/`, `.git`, `node_modules` ni binarios), con refresco manual y clic para abrir. Tests: 1 (toggle abre/carga/cierra y respeta exclusiones).
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 128, karkinos-core 80.
- **F7 (v0.2.34) — completada**: `SymbolKind` y `DocumentSymbol` en `syntax_ast` con `AstEngine::extract_document_symbols` (funciones, structs, enums, traits, constantes y métodos dentro de `impl`/`trait`, todo del AST, sin LSP). Los símbolos viajan en el mismo `AstResult` del worker y se guardan en `EditorState.symbols`. UI: modal `Ctrl+Shift+O` (o botón ƒ de la barra lateral) con filtro por nombre, navegación con flechas, Enter/clic para saltar y badges `ƒ S E T M C`.
  - Desvío: no se interceptó `@` dentro de Quick Open; el selector tiene su propio atajo y modal.
  - Tests: 2 en el core (cobertura de tipos/métodos y líneas; sin árbol) + 2 en la app (filtro sin mayúsculas; salto que posiciona y cierra).
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 130, karkinos-core 82.
- **F8 (v0.2.35) — completada**: `crates/karkinos-core/src/git_status.rs` lee la rama de `.git/HEAD` sin subprocesos (incluye detached HEAD) y parsea `git blame --line-porcelain` reutilizando metadatos por sha. Worker `git_status_worker.rs` corre el blame una vez por archivo y lo cachea. UI: pill `⎇ rama` en la barra de estado y ghost text tenue al final de la línea activa (`Autor, N days ago • mensaje`) que aparece solo cuando el cursor queda quieto 300 ms.
  - Tests: 4 en el core (rama, detached, porcelain con commits repetidos, formato y edad relativa) + 2 en la app (se limpia al moverse; aparece tras el dwell con cache).
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 132, karkinos-core 86.
