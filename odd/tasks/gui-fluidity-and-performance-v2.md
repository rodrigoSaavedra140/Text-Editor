# Task: Optimización de Fluidez UI y Rendimiento Extremo (Karkinos IDE)

**Objetivo**: Eliminar asignaciones redundantes por frame y llamadas innecesarias de repintado en reposo para lograr 120+ FPS constantes y 0% de uso de CPU en reposo con la terminal y editor abiertos.
**Target Agent**: Agente liviano (`flash` o `flash_lite`).
**Reglas del proyecto**:
1. Modificaciones estrictamente en Rust + Cargo.
2. Cero compiler warnings (`cargo check --workspace` limpio).
3. Todos los tests deben pasar (`cargo test --workspace`).
4. Versionado: incrementar solo dígitos menores/parche (0.X.Y), manteniendo intacto el primer dígito 0 (de `0.2.25` a `0.2.26`).
5. Commits convencionales, sin `Co-Authored-By` ni atribuciones de IA.

---

## Tareas Específicas para el Agente Liviano

### OPT-1: Dirty Tracking y Caché de Snapshots en Terminal PTY
- **Archivos a modificar**:
  - `crates/karkinos-core/src/terminal.rs`
  - `karkinos/src/terminal_service.rs`
  - `karkinos/src/ui.rs`
- **Problema**:
  1. `crates/karkinos-core/src/terminal.rs::snapshot()` genera miles de asignaciones de memoria (`cell.contents().to_string()`) por frame si la terminal está visible.
  2. `karkinos/src/ui.rs::show_interactive_terminal()` invoca `ctx.request_repaint_after(30ms)` incondicionalmente, forzando repintado perpetuo incluso cuando no hay salida en la consola.
- **Implementación paso a paso**:
  1. En `crates/karkinos-core/src/terminal.rs`:
     - Agregar a `PtyTerminal`: `revision: std::sync::Arc<std::sync::atomic::AtomicU64>`.
     - En el hilo lector del PTY (`reader.read(&mut buf)`), justo después de `p.process(&buf[..n])`:
       ```rust
       revision_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
       ```
     - Exponer método:
       ```rust
       pub fn revision(&self) -> u64 {
           self.revision.load(std::sync::atomic::Ordering::Relaxed)
       }
       ```
  2. En `karkinos/src/terminal_service.rs`:
     - Agregar a `TerminalService`:
       ```rust
       cached_snapshot: Option<(u64, TerminalSnapshot)>,
       ```
     - En `TerminalService::snapshot(&mut self) -> Option<&TerminalSnapshot>`:
       Consultar la revisión actual del PTY. Si coincide con la guardada en caché, retornar la referencia existente sin clonar ni iterar la grilla. Si cambió, regenerar el snapshot, actualizar la caché y retornar.
  3. En `karkinos/src/ui.rs` (`show_interactive_terminal`):
     - Reemplazar el `request_repaint_after(30ms)` incondicional por chequeo de actividad: solo pedir repintado reactivo si el PTY está vivo o si hubo eventos de teclado/paste. Si está inactivo, usar `request_repaint_after(Duration::from_millis(150))` para el parpadeo suave del cursor.

---

### OPT-2: Memoización de Inlay Hints en el Editor
- **Archivo a modificar**:
  - `karkinos/src/editor_widget.rs`
- **Problema**:
  En `karkinos/src/editor_widget.rs` (líneas ~1533-1554), para cada línea visible en cada frame, se recorre `state.inlay_hints` y se llama a:
  `ui.fonts(|f| f.layout_no_wrap(hint.label.clone(), hint_font, hint_color))`
  Esto asigna strings y recalcula glifos 60-144 veces por segundo por cada hint en pantalla.
- **Implementación paso a paso**:
  1. En `karkinos/src/editor_widget.rs`:
     - En el bucle de render de inlay hints, almacenar o cachear los `egui::Galley` de hints o calcularlos solo cuando cambie la lista de hints o el tamaño de fuente (`state.font_size`).
     - Alternativa directa y limpia: agregar un `HashMap<(String, u32), std::sync::Arc<egui::Galley>>` en `EditorState` para labels de inlay hints, evitando llamar a `f.layout_no_wrap` repetidamente para los mismos tipos (ej. `: i32`, `: &str`, `x:`).

---

### OPT-3: Suavizado Inercial de Scroll (Smooth Scrolling Easing)
- **Archivo a modificar**:
  - `karkinos/src/editor_widget.rs`
- **Problema**:
  El scroll con rueda o trackpad modifica `state.scroll` de forma discreta y abrupta, generando saltos visuales en pantallas de alta tasa de refresco (120Hz/144Hz).
- **Implementación paso a paso**:
  1. En `EditorState`:
     - Agregar: `pub target_scroll: f32`.
  2. En el manejo de rueda (`scroll_delta`):
     - `state.target_scroll = (state.target_scroll - scroll_delta.y).clamp(0.0, max_scroll);`
  3. En cada frame:
     - Interpolar: `let diff = state.target_scroll - state.scroll;`
     - Si `diff.abs() > 0.5`:
       ```rust
       state.scroll += diff * 0.25; // Interpolación exponencial suave
       ui.ctx().request_repaint(); // Mantener 60+ FPS durante la animación de scroll
       ```
     - Si `diff.abs() <= 0.5`: `state.scroll = state.target_scroll;`
  4. Mantener sincronizado `target_scroll` cuando el usuario arrastra la barra de scroll directamente.

---

### OPT-4: Actualización de Versión y Verificación
1. En `crates/karkinos-core/Cargo.toml` y `karkinos/Cargo.toml`:
   - Incrementar de `0.2.25` a `0.2.26`.
2. Ejecutar:
   ```bash
   cargo check --workspace
   cargo test --workspace
   ```
3. Asegurar que los 167+ tests pasen con 0 warnings.
4. Generar el commit convencional:
   ```bash
   git commit -am "perf(ui): zero-alloc terminal snapshot cache, memoized inlay hints, and smooth scroll (v0.2.26)"
   ```

---

## Prompt de Ejecución para Invocar al Agente Liviano

Copiar y pegar este prompt al invocar al sub-agente (ej. modelo `flash` o `flash_lite`):

```text
Lee el archivo odd/tasks/gui-fluidity-and-performance-v2.md y ejecuta las tareas OPT-1 a OPT-4 en orden secuencial.
Reglas obligatorias:
- Modifica solo los archivos indicados en cada tarea.
- Ejecuta `cargo check --workspace` y `cargo test --workspace` al finalizar cada paso.
- 0 warnings de compilación y todos los tests verdes.
- Incrementa la versión a 0.2.26 en ambos Cargo.toml.
- Haz el commit convencional final especificado en el documento. No agregues Co-Authored-By ni mención a IA.
```
