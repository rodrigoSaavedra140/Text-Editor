# Feature: Robustez y Fluidez Arquitectónica de Karkinos IDE

**Objetivo**: Transformar Karkinos en un IDE de ultra-bajo consumo y latencia de entrada imperceptible, adoptando patrones probados de editores de alto rendimiento (Zed, Lapce, Helix) adaptados al pipeline gráfico de `egui`.
**Branch**: `main` (commits convencionales por unidad de trabajo).
**Versionado**: Bumping de dígitos menores/parche (0.X.Y), nunca el primer dígito (0).

---

## Especificaciones y Plan de Implementación

### S1 — Caché de Layout de Líneas (`LineLayoutCache`)
- **Problema actual**: `editor_widget.rs` y `syntax.rs` invocan `build_layout_job` en cada frame para cada línea visible. En un monitor de 144Hz, el lexer y el formateador de egui corren cientos de veces por segundo aunque el usuario no toque el teclado.
- **Solución / Cómo implementarlo**:
  1. En `karkinos/src/syntax.rs` o `editor_widget.rs`, crear:
     ```rust
     #[derive(Default, Clone)]
     pub struct LineLayoutCache {
         entries: std::collections::HashMap<(u64, crate::theme::ThemeMode), std::sync::Arc<egui::epaint::text::LayoutJob>>,
     }
     ```
  2. Clave de hash: `fxhash` o `std::hash::DefaultHasher` sobre los bytes de la línea (`&str`) + el tema actual.
  3. En `build_layout_job`, consultar primero la caché: si existe, retornar `Arc::clone(&job)`. Si no existe, computar el `LayoutJob`, guardarlo en la caché y retornarlo.
  4. Invalidación: Al editar el documento o cambiar de tema, limpiar o podar líneas obsoletas (o limitar el tamaño a un LRU de 2.000 líneas visibles).

### S2 — Repintado Reactivo Inteligente (Idle CPU Throttling)
- **Problema actual**: Si no se gestiona el repintado, egui solicita redibujo continuo, consumiendo 5-10% de CPU y batería aún cuando el editor está inactivo.
- **Solución / Cómo implementarlo**:
  1. En `karkinos/src/main.rs` o `karkinos/src/app.rs::update()`:
     Detectar si hay actividad activa (animación de cursor, carga en background, o interacción del mouse/teclado).
  2. Si está en reposo absoluto, no llamar a `ctx.request_repaint()` incondicionalmente. Usar `ctx.request_repaint_after(Duration::from_millis(500))` para el parpadeo del cursor.
  3. Resultado: 0.0% de CPU en reposo; al recibir un evento de teclado/mouse, el sistema operativo despierta el hilo inmediatamente a 120+ FPS.

### S3 — Tree-sitter AST Parsing Asíncrono en Background Worker (Estilo Zed)
- **Problema actual**: `update_ast()` corre `self.ast_engine.parse(&text)` en el hilo de la UI al tipear. En archivos medianos/grandes (>3.000 líneas), el parseo síncrono produce micro-tirones (frame drops).
- **Solución / Cómo implementarlo**:
  1. En `crates/karkinos-core/src/syntax_ast.rs` o `karkinos/src/app.rs`:
     Crear un `AstWorker` con un canal `std::sync::mpsc::channel`.
  2. Al detectar cambio en el buffer, en lugar de parsear en la UI:
     - Tomar un snapshot $O(1)$ del buffer: `let text_snapshot = self.doc.buffer().to_string();` (o clonar el `Rope`).
     - Enviar `(revision, text_snapshot, language)` al worker.
  3. El worker corre `tree_sitter::Parser::parse()` en segundo plano sin tocar la GPU.
  4. En cada frame, la UI hace `ast_worker.try_recv()`. Si llegó un resultado nuevo, actualiza `ast_spans` y solicita repaint suave.
  5. Cero congelamiento al escribir en archivos gigantes.

### S4 — Quick Open Asíncrono con Indexación en Background (Ctrl+P)
- **Problema actual**: Al presionar `Ctrl+P`, `quick_open.rs` lee carpetas y corre fuzzy matching en el hilo de dibujo. En monorepos con 20.000 archivos puede demorar 200-500ms bloqueando la pantalla.
- **Solución / Cómo implementarlo**:
  1. Lanzar la indexación inicial de archivos en un hilo background al abrir una carpeta (`browse_dir`).
  2. Guardar la lista plana de rutas en memoria `Arc<Vec<PathBuf>>`.
  3. Cuando el usuario tipea en el buscador, procesar el scoring difuso en un canal o hilo ligero y alimentar el selector de forma asíncrona.

### S5 — Desacople Modular de `EditorApp` (Clean Architecture)
- **Problema actual**: `karkinos/src/app.rs` concentra ~3,000 líneas de código con responsabilidades cruzadas.
- **Solución / Cómo implementarlo**:
  Dividir en módulos especializados:
  - `document_service.rs`: `DocumentManager` maneja tabs, dirty flags, mtime reload y line endings.
  - `lsp_service.rs`: encapsula la conexión a `rust-analyzer`, diagnósticos e inlay hints.
  - `cargo_service.rs`: maneja el subproceso `cargo` y streaming de logs/mensajes del compilador.
  - `dap_service.rs`: maneja la sesión con `lldb-dap`, breakpoints y lista de variables.
  - `ui.rs` y `editor_widget.rs`: se enfocan pura y exclusivamente en presentación.

---

## Tareas

| ID | Spec | Descripción | Estado |
| :--- | :--- | :--- | :--- |
| **T1** | S1 | Implementar `LineLayoutCache` en `syntax.rs` para reutilizar layouts entre frames | ✅ Hecho — `feat(perf): memoize line layout galleys to kill per-frame lexing` (v0.2.10) |
| **T2** | S2 | Configurar repintado reactivo (`request_repaint_after`) para reposo a 0% CPU | ✅ Hecho — `feat(perf): reactive caret blink and event-driven repaint (v0.2.11)` |
| **T3** | S3 | Worker en background para parseo incremental de Tree-sitter AST | Pendiente |
| **T4** | S4 | Indexación y filtrado fuzzy en background para Quick Open (`Ctrl+P`) | Pendiente |
| **T5** | S5 | Desacoplar `EditorApp` en servicios dedicados (`DocumentManager`, `LspService`, etc.) | Pendiente |

---

## Log

- **L1** (2026-10-30): Solicitud original del usuario: "empieza a hacer las tareas" con el blueprint de las S1-S5 dejado por el agente anterior. El agente anterior dejó el roadmap en Engram (`architecture/robustness-and-performance-roadmap`) y este documento.
- **L2** (2026-10-30, T1): Implementado `LineLayoutCache<V>` genérico en `karkinos/src/syntax.rs` (LRU con capacidad 2048 según el S1, poda del cuarto más viejo, `sync_ast` para invalidación por generación del AST). Clave = hash de (bytes del fragmento, lenguaje, tamaño de fuente) + `ThemeMode` en la primera componente del HashMap según el S1. En producción `V = Arc<egui::Galley>` (el galley ya viene tipografiado; reusarlo salta también el shaper de egui, más que el `Arc<LayoutJob>` del blueprint: mismo alta de cache, menos trabajo por miss).
- **L3** (2026-10-30, T1): Cambios accesorios: `ThemeMode` ahora deriva `Hash` (+`Ord` NO — la poda ordena por tick); `EditorState` ganó `layout_cache` y `ast_rev` (viaja clonado por pestaña); `update_ast()` incrementa `ast_rev` (ambas ramas); el render converso tokeniza las líneas PEREZOSAMENTE (antes `tokenize_line_semantic` re-recorría `highlight_spans()` del árbol entero por línea visible por frame). 6 tests nuevos del cache (hit, por-tema, invalidación AST, claves, LRU). `cargo test --workspace`: 86+52 OK, 0 warnings. Compromis explicado: el tokenize queda en el hilo de UI hasta T3 (worker en background).
- **L4** (2026-10-30, T2): Auditoría previa: la app NO pedía repintado continuo (el único `request_repaint` incondicional es el de la barra de guardado, acotado a su ciclo); el hueco real era que el caret NO parpadeaba (trazo estático) y faltaba la cadencia de wake. Implementado parpadeo por fase pura: `caret_blink_visible`/`caret_blink_next_wake` (período 0.5s, sólido tras interacción), `EditorState.last_activity_time` reinicia la fase con Text/KeyPress/PointerButton/MouseWheel, y `request_repaint_after(próximo cruce)` SOLO con foco. 3 tests nuevos. `cargo test --workspace`: 89+52 OK, 0 warnings. En fase idle el wake es ~2 Hz (blink), nunca 144fps.
