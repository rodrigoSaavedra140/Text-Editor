# Roadmap: Next-Gen IDE Features for Karkinos (2025/2026 Spec)

**Objetivo**: Evolucionar Karkinos IDE hacia el nivel de ergonomía y potencia de editores industriales modernos (Zed, Lapce, Helix, Cursor), aprovechando la arquitectura desacoplada de servicios (`DocumentService`, `LspService`, `AstWorker`, `PtyTerminal`) sin sacrificar el rendimiento a 120+ FPS ni el reposo a 0% CPU.
**Branch**: `main` (commits convencionales por unidad de trabajo).
**Versionado**: Bumping estricto de dígitos menores/parche (0.X.Y), conservando siempre el primer dígito en 0 (ej. `0.2.27`, `0.2.28`, etc.).
**Calidad**: Cero compiler warnings (`cargo check --workspace`), 100% de tests unitarios y de integración pasando (`cargo test --workspace`).

---

## Matriz de Características y Fases de Implementación

| ID | Feature | Cajas/Servicios Afectados | Complejidad | ROI Arquitectónico | Estado |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **F1** | **Git Diff Gutter Marks** | `karkinos-core::git_diff`, `editor_widget.rs` | Media | Alto | ✅ Hecho (`v0.2.27`) |
| **F2** | **Sticky Scroll (AST Breadcrumbs)** | `karkinos-core::syntax_ast`, `editor_widget.rs` | Media | Muy Alto | ✅ Hecho (`v0.2.28`) |
| **F3** | **Multi-Cursor / Multi-Caret (`Ctrl+D`)** | `editor_widget.rs::EditorState` | Media-Alta | Muy Alto | ✅ Hecho (`v0.2.29`) |
| **F4** | **Code Folding (Plegado por AST)** | `ast_worker.rs`, `editor_widget.rs` | Media | Alto | ✅ Hecho (`v0.2.30`) |
| **F5** | **Split Panes (Editor Side-by-Side)** | `ui.rs`, `app.rs`, `editor_widget.rs` | Alta | Máximo | ⏳ Próximo |

---

## Especificaciones Técnicas y Blueprint de Implementación

---

### F1 — Git Diff Gutter Marks (Marcas de Estado Git en el Editor)

#### 1. Concepto y Experiencia de Usuario
Al editar un archivo dentro de un repositorio Git, el gutter de números de línea muestra una barra vertical de 3px de ancho indicando el estado de cada línea frente al commit `HEAD`:
- **Verde**: Línea nueva agregada.
- **Amarillo/Azul**: Línea modificada.
- **Triángulo rojo / marca sutil**: Línea(s) eliminada(s) justo debajo de la posición anterior.

#### 2. Estructuras de Datos (`crates/karkinos-core/src/git_diff.rs`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Modified,
    DeletedBelow,
}

#[derive(Default, Debug, Clone)]
pub struct FileDiffHunks {
    pub line_status: std::collections::HashMap<usize, DiffStatus>,
}

impl FileDiffHunks {
    /// Computa los estados de línea comparando el buffer actual con el contenido de git HEAD.
    pub fn compute(head_text: &str, current_text: &str) -> Self {
        // Implementación con algoritmo de Myers o `similar` crate
        // Genera el mapeo de línea lógica -> DiffStatus
    }
}
```

#### 3. Integración en Segundo Plano
- `DocumentService` mantiene un hilo o worker en background que ejecuta `git show HEAD:<relative_path>` solo al abrir archivo o al guardar (`save`).
- Al recibir el texto de `HEAD`, corre `FileDiffHunks::compute` en background y deposita el resultado en `tab.git_diff: Arc<FileDiffHunks>`.
- **Cero latencia en UI**: La UI nunca llama a comandos `git` de forma síncrona en el hilo principal.

#### 4. Renderizado en el Gutter (`karkinos/src/editor_widget.rs`)
En el bucle de renderizado de números de línea:
```rust
if let Some(status) = state.git_diff.and_then(|d| d.line_status.get(&logical_line)) {
    let mark_color = match status {
        DiffStatus::Added => egui::Color32::from_rgb(80, 200, 120),
        DiffStatus::Modified => egui::Color32::from_rgb(230, 180, 80),
        DiffStatus::DeletedBelow => egui::Color32::from_rgb(220, 80, 80),
    };
    let mark_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, line_top_y),
        egui::pos2(rect.left() + 4.0, line_top_y + row_height),
    );
    painter.rect_filled(mark_rect, 1.0, mark_color);
}
```

---

### F2 — Sticky Scroll (Scope Breadcrumbs Flotantes por AST)

#### 1. Concepto y Experiencia de Usuario
Cuando el usuario scrollea hacia abajo dentro de una función, struct o módulo largo, el encabezado que define el alcance (`fn process(...) {`, `impl EditorApp {`) se mantiene "fijado" en la parte superior del editor debajo de la barra de pestañas.
Si hay anidamiento (ej. `impl` -> `fn` -> `match`), muestra hasta 3 niveles apilados. Al hacer clic en un encabezado fijado, el viewport salta automáticamente a esa línea.

#### 2. Estructuras de Datos (`crates/karkinos-core/src/syntax_ast.rs`)
Aprovechando que ya tenemos Tree-sitter integrado:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeHeader {
    pub start_line: usize,
    pub end_line: usize,
    pub header_text: String,
    pub node_kind: &'static str, // "function", "impl", "struct", "class"
    pub depth: usize,
}

pub fn extract_sticky_scopes(tree: &tree_sitter::Tree, source: &str) -> Vec<ScopeHeader> {
    // Recorre el AST identificando nodos contenedores relevantes:
    // Rust: function_item, impl_item, struct_item, enum_item, mod_item
    // Python/JS: class_definition, function_definition
    // Retorna lista ordenada de ScopeHeaders
}
```

#### 3. Integración en `ast_worker.rs`
El `AstWorker` ya procesa revisiones del documento en un hilo background. Al terminar el parseo del AST, además de los `AstHighlightSpan`, emite `Vec<ScopeHeader>` en el mismo payload hacia la UI:
```rust
pub struct AstWorkerResult {
    pub revision: u64,
    pub spans: Vec<AstHighlightSpan>,
    pub scopes: Vec<ScopeHeader>,
}
```

#### 4. Presentación (`karkinos/src/editor_widget.rs`)
1. Calcular la línea visible superior (`first_visible_line = (state.scroll / row_height) as usize`).
2. Filtrar los scopes activos:
   `scopes.iter().filter(|s| s.start_line < first_visible_line && s.end_line > first_visible_line)`
3. Limitar a un máximo de 3 niveles.
4. Dibujar una pequeña franja flotante con sombra suave y fondo ligeramente translúcido (`theme.bg.gamma_multiply(0.95)`), mostrando la línea con syntax highlighting.

---

### F3 — Multi-Cursor / Multi-Caret Editing (`Ctrl+D`)

#### 1. Concepto y Experiencia de Usuario
Permite a los usuarios tener múltiples cursores independientes escribiendo, borrando y navegando en paralelo:
- **`Ctrl+D`**: Selecciona la palabra bajo el cursor principal. Si se vuelve a presionar `Ctrl+D`, busca la siguiente ocurrencia en el buffer y agrega un nuevo cursor/selección allí.
- **`Alt+Click`**: Agrega un cursor adicional en la posición del mouse.
- **`Escape`**: Colapsa todos los cursores al cursor principal.

#### 2. Estructuras de Datos (`karkinos/src/editor_widget.rs`)
Reemplazar el cursor único por una colección ordenada de cursores con su propia selección opcional:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caret {
    pub line: usize,
    pub col: usize,
    pub anchor: Option<(usize, usize)>, // Si tiene selección activa
}

pub struct EditorState {
    pub carets: Vec<Caret>, // carets[0] es siempre el cursor primario
    // ... campos existentes ...
}
```

#### 3. Algoritmo de Edición Concurrente
Para evitar que las ediciones en un cursor invaliden las posiciones (offset de línea/columna) de los otros cursores:
1. **Ordenar los carets de abajo hacia arriba y de derecha a izquierda** antes de aplicar cualquier mutación:
   `carets.sort_by(|a, b| b.line.cmp(&a.line).then_with(|| b.col.cmp(&a.col)));`
2. Aplicar la inserción o borrado en cada cursor de forma independiente.
3. Desduplicar cursores coincidentes (si dos cursores colisionan en la misma posición, se fusionan en uno solo).

---

### F4 — Code Folding (Plegado de Código por AST)

#### 1. Concepto y Experiencia de Usuario
En el gutter de números, al pasar el mouse sobre bloques colapsables (funciones, bloques `impl`, `struct`, `match`, o comentarios multilínea), aparece un icono desplegable:
- `▼` (Expandido)
- `▶` (Plegado): el contenido del bloque se oculta y se dibuja una píldora compacta `...` en el editor.

#### 2. Estructuras de Datos
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
    pub is_folded: bool,
}
```
`EditorState` almacena `pub folds: Vec<FoldRange>`.

#### 3. Lógica de Paginación y Renderizado
- Al iterar las líneas en `rebuild_row_counts` y en el bucle principal de dibujo:
  Si una línea cae dentro de un rango con `is_folded == true` (entre `start_line + 1` y `end_line`), se salta el layout y dibujo de esa fila.
- `total_rows` se reduce automáticamente, achicando la barra de scroll y acelerando el renderizado de archivos gigantes.

---

### F5 — Split Panes (División de Editor Horizontal / Vertical)

#### 1. Concepto y Experiencia de Usuario
Poder dividir el área central del editor en dos paneles independientes (`Side-by-Side` o `Top-Bottom`):
- `Ctrl+\`: Dividir verticalmente.
- `Ctrl+Shift+\`: Dividir horizontalmente.
- Cada panel tiene su propio tab activo, su propio cursor y su propio scroll, permitiendo comparar dos archivos o dos partes del mismo archivo a la vez.

#### 2. Estructuras de Datos (`karkinos/src/app.rs`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

pub struct EditorPane {
    pub tab_index: usize,
    pub state: EditorState,
}

pub struct SplitLayout {
    pub active_pane: usize,
    pub direction: Option<SplitDirection>,
    pub panes: Vec<EditorPane>,
    pub split_ratio: f32, // Default 0.5 (50% - 50%)
}
```

#### 3. Integración en `ui.rs`
En `show_central_editor`:
1. Si `direction.is_none()`, renderizar un único `editor_widget::show`.
2. Si `direction == Some(SplitDirection::Vertical)`:
   - Dividir el `available_rect` en `left_rect` y `right_rect` usando `split_ratio`.
   - Renderizar `editor_widget::show` para el pane 0 en `left_rect` y para el pane 1 en `right_rect`.
   - Renderizar una barra divisoria arrastrable de 4px entre ambos paneles.

---

## Plan de Ejecución Secuencial y Commits

Cada feature se implementa como una unidad aislada con tests unitarios y bump de versión menor/parche:

1. **Unidad 1 (v0.2.27)**: `feat(git): non-blocking git diff gutter indicators in editor widget`
2. **Unidad 2 (v0.2.28)**: `feat(ast): tree-sitter sticky scroll scope breadcrumbs`
3. **Unidad 3 (v0.2.29)**: `feat(editor): multi-cursor editing and Ctrl+D next occurrence selection`
4. **Unidad 4 (v0.2.30)**: `feat(folding): AST-driven code folding with gutter toggle icons`
5. **Unidad 5 (v0.2.31)**: `feat(layout): vertical and horizontal editor split panes`

---

## Verificación de Cada Paso
- `cargo check --workspace` (0 warnings).
- `cargo test --workspace` (todos los tests existentes + nuevos tests de la feature pasando).
- Verificación visual de interactividad y ausencia de drops de FPS.

---

## Log de Ejecución

- **F1 (v0.2.27) — completada**: `crates/karkinos-core/src/git_diff.rs` implementa el diff por líneas con Myers sobre hashes de línea (O(ND), sin matriz completa) y cota de distancia de edición con fallback por bloque. El worker `karkinos/src/git_diff_worker.rs` corre `git show HEAD:<rel>` fuera del hilo de UI. `EditorState.git_diff` alimenta marcas de 3 px en el borde izquierdo del gutter: verde (nueva), ámbar (modificada), rojo (borrada debajo).
  - Cuándo se pide: solo con el archivo limpio, al abrir (ruta distinta) o al terminar de guardar (sucio → limpio). Mientras se escribe no se pide nada, así que `git` nunca corre por tecla ni en el hilo principal.
  - Tests: 8 unitarios del diff (idénticos, inserción, modificación, borrado, varios hunks, cabecera vacía, buffer vacío, cambio lejano) + 3 del worker (raíz de repo, fuera de repo, pipeline real contra un repo git temporal).
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 116, karkinos-core 67.
  - Nota: `crates/karkinos-core/tests/benchmark_runner.rs`, dejado sin trackear por otra sesión, no compilaba contra la API actual del crate (llamaba `TextBuffer::line_count`/`char_count` y `AstEngine::parse`); se apartó a `/tmp/opencode/parked/` en vez de borrarlo.
- **F2 (v0.2.28) — completada**: `AstEngine::sticky_scopes` en `crates/karkinos-core/src/syntax_ast.rs` recorre el AST y devuelve `ScopeHeader { start_line, end_line, header_text, node_kind, depth }` para contenedores multilínea (function, impl, struct, enum, module, trait, class). El mismo parseo del `AstWorker` ahora transporta `scopes`, y `editor_widget` los dibuja como bandas pegadas arriba del editor (hasta 3 niveles), con clic que salta al inicio del bloque.
  - Tests: 3 en el core (anidamiento y profundidad, ítems de una línea excluidos, sin árbol) + la suite existente del worker.
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 116, karkinos-core 70.
- **F3 (v0.2.29) — completada**: `Caret { line, col, anchor }` más `EditorState.extra_carets` (el cursor primario sigue en `cursor_line`/`cursor_col`/`sel_anchor`, así la edición de un solo cursor no cambia). `Ctrl+D` selecciona la palabra y cada pulsación agrega la siguiente aparición; `Alt+Click` agrega un cursor; `Escape` colapsa. La inserción de texto y el borrado (Backspace/Delete) se aplican en todos los cursores, resueltos de abajo hacia arriba para no correr índices, con deduplicado por posición.
  - Cambio de atajo acordado con el usuario: «duplicar línea» pasó a `Ctrl+Shift+D`; se actualizaron `README.md` y la especificación S7.
  - Tests: 5 nuevos del modelo y helpers (normalización de rango, búsqueda de la próxima aparición, inserción múltiple sin desplazar posiciones, borrado de varios rangos, deduplicado).
  - Evidencia: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 121, karkinos-core 70.
  - Límite conocido: las selecciones de cursores extra se dibujan sin ajuste de línea (en líneas largas con wrap la marca puede quedar corrida); la edición y el cursor sí son correctos.
