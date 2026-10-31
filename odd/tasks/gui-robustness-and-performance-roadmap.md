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
| **T3** | S3 | Worker en background para parseo incremental de Tree-sitter AST | ✅ Hecho — `feat(ast): background tree-sitter worker with revision coalescing (v0.2.12)` |
| **T4** | S4 | Indexación y filtrado fuzzy en background para Quick Open (`Ctrl+P`) | ✅ Hecho — `feat(quick-open): async quick open index and fuzzy scoring (v0.2.13)` |
| **T5** | S5 | Desacoplar `EditorApp` en servicios dedicados (`DocumentManager`, `LspService`, etc.) | Pendiente |

---

## Log

- **L1** (2026-10-30): Solicitud original del usuario: "empieza a hacer las tareas" con el blueprint de las S1-S5 dejado por el agente anterior. El agente anterior dejó el roadmap en Engram (`architecture/robustness-and-performance-roadmap`) y este documento.
- **L2** (2026-10-30, T1): Implementado `LineLayoutCache<V>` genérico en `karkinos/src/syntax.rs` (LRU con capacidad 2048 según el S1, poda del cuarto más viejo, `sync_ast` para invalidación por generación del AST). Clave = hash de (bytes del fragmento, lenguaje, tamaño de fuente) + `ThemeMode` en la primera componente del HashMap según el S1. En producción `V = Arc<egui::Galley>` (el galley ya viene tipografiado; reusarlo salta también el shaper de egui, más que el `Arc<LayoutJob>` del blueprint: mismo alta de cache, menos trabajo por miss).
- **L3** (2026-10-30, T1): Cambios accesorios: `ThemeMode` ahora deriva `Hash` (+`Ord` NO — la poda ordena por tick); `EditorState` ganó `layout_cache` y `ast_rev` (viaja clonado por pestaña); `update_ast()` incrementa `ast_rev` (ambas ramas); el render converso tokeniza las líneas PEREZOSAMENTE (antes `tokenize_line_semantic` re-recorría `highlight_spans()` del árbol entero por línea visible por frame). 6 tests nuevos del cache (hit, por-tema, invalidación AST, claves, LRU). `cargo test --workspace`: 86+52 OK, 0 warnings. Compromis explicado: el tokenize queda en el hilo de UI hasta T3 (worker en background).
- **L4** (2026-10-30, T2): Auditoría previa: la app NO pedía repintado continuo (el único `request_repaint` incondicional es el de la barra de guardado, acotado a su ciclo); el hueco real era que el caret NO parpadeaba (trazo estático) y faltaba la cadencia de wake. Implementado parpadeo por fase pura: `caret_blink_visible`/`caret_blink_next_wake` (período 0.5s, sólido tras interacción), `EditorState.last_activity_time` reinicia la fase con Text/KeyPress/PointerButton/MouseWheel, y `request_repaint_after(próximo cruce)` SOLO con foco. 3 tests nuevos. `cargo test --workspace`: 89+52 OK, 0 warnings. En fase idle el wake es ~2 Hz (blink), nunca 144fps.
- **L5** (2026-10-31, T3): Nuevo módulo `karkinos/src/ast_worker.rs`: hilo dedicado dueño del `AstEngine` (no necesita ser `Send`), canal de pedidos `mpsc` (drenado al último pedido = coalescing), canal de resultados `sync_channel(1)` con backpressure natural. La UI envía `<snapsho>_text` por edición y hace `try_recv_result()` por frame vía `poll_ast_results()` — aplican solo resultados con `revision >= ast_applied_rev`. `update_ast()` mutateo dispatcher del worker (mismo nombre, call-sites intactos); `load_tab` SIEMPRE re-pide el parseo al cambiar de tab. `EditorState` ya no tiene `ast_engine`: consume `ast_spans: Vec<AstHighlightSpan>` (spans planos, `tokenize_line_semantic` cambió firma). 3 tests del worker + 2 adaptados en app. FIX importante: el primer Drop causaba deadlock (join mientras el Sender seguía vivo → el worker nunca salía de recv); resuelto soltando canales (result_rx → request_tx) antes del join. `cargo test --workspace`: 92+52 OK, 0 warnings. Compromiso documentado: el snapshot enviado es `String` (copia O(n) por edición, ~100KB ≈ tens de µs) dado que ropey no alimenta tree-sitter directamente; el O(1) del blueprint no es posible con esta combo de librerías.
- **L6** (2026-10-31, T4): Quick Open async. `BackgroundWorker<J,R>` genérico en `karkinos/src/quick_open.rs` (patrón del worker de T3 con el fix de drop-order: `take()` de `result_rx`/`job_tx` antes de `join`). `QuickOpenServices` es de `EditorApp` (indexador + scorer, hilos reales en prod, `noop()` solo tests bajo `#[cfg(test)]`); `QuickOpenState` delega: `open()` → `request_open(&root, &mut services)` (cache caliente por `indexed_root` en reabrir la misma carpeta), `set_query(query, services)` con scoring inline bajo `INLINE_SCORE_THRESHOLD = 1024` y en worker arriba; `poll(services)` por frame en `ui.rs::update()` (repinto suave 16ms). Generaciones separadas con wrapping (`open_generation` por escaneo, `score_generation` por query; stale (`gen vieja`) no contamina). Modal muestra "Scanning…"/"Scoring…" en vez del "0 files found" mentiroso. 7 tests (fuzzy boundary, index en fondo con salteo de ocultos, scoring grande→worker, resultado viejo ignorado, reabrir caché, navegación, exactidad). `cargo test --workspace`: 96+52 OK, 0 warnings.
- **L7** (2026-10-31, T5 plan): Mapa de fronteras de `app.rs` (3.062 líneas, HEAD `fd73bcf`) hecho por explorer. Hallazgos clave: ui.rs tiene dos bloques `impl EditorApp` (~300 accesos directos a campos — es la superficie real del split); `LineEnding`/`detect_line_ending`/`serialize_line_ending`/`default_new_file_path` en app.rs duplican `karkinos-core/src/ending.rs` (ya re-exportado en core lib.rs:27 → re-exportar, no copiar); `run_cargo` ya es delegado de 2 líneas a `karkinos-core/src/cargo.rs::CargoRunner`; quick_open.rs y main.rs casi no dependen de app.rs. Plan: 4 slices secuenciales inline (unidades del mismo archivo app.rs compartido, sin writers paralelos), un commit por slice con bump patch: T5a `document_service.rs` (`SaveProgress`, `TabDocument`, campos de doc/tabs/mtime/save/close-new, métodos 437–687 + snapshot/undo 1405–1451 + file I/O 1599–1836 + close/new 1854–1953; LineEnding→re-export core) — `refactor(document): extract document service from EditorApp (v0.2.14)`; T5b `lsp_service.rs` (campos 294–316, métodos 735–956 + apply ediciones 957–1068, v0.2.15); T5c `cargo_service.rs` (campos 309–311, run/jump, v0.2.16); T5d `dap_service.rs` (campos 318–321, métodos 1069–1243, v0.2.17). Stays (sin módulos nuevos): find/replace, goto, zoom, theme, picker, quick_open, plugin/extensions/open_vsx, ast_worker, chunk_tracker, editor_state (display). Cada slice: suite verde + 0 warnings antes de commit.
- **L8** (2026-10-31, T5a): `document_service.rs` creado (~560 líneas). `DocumentService` posee 29 campos doc/tab (buffer activo, undo/snapshot, save/mtimien, pestañas, `TabDocument`, dir_cache, etc.); EditorApp queda `pub doc: DocumentService` + 41 campos de servicios/UI. 23 métodos puros movidos (save try/nombre/cancel, fechados mtime, list_dir/cached_list_dir/read_file/is_probably_text estáticos, current_text/with_ending, snapshot_text/if_changed, is_buffer_empty/is_empty_buffer, recalc_view_bytes, first_dirty/dirty_count/all_dirty_is_empty, check_external_changes, note_save_done, cancel_close/confirm_close_*, confirm_new_doc_save). El duplicado `LineEnding`/`detect`/`serialize`/`default_new_file_path` de app.rs (T5a) eliminado y reexportado desde `karkinos-core/src/ending.rs` (uno solo). Los orquestadores que además tocan presentación/LSP/AST (load_tab, set_text, sync_active_*, switch/new/close_tab, undo/redo, open_path, new_document, reload, jump_to_cargo_diagnostic...) siguen en app.rs llamando al servicio. Datos reescritos por script: 216+150+4 reescrituras de prefijo en app.rs/ui.rs (ui.rs značení 94 → 103 + 4 encadenados). app.rs 3.062 -> ~2.480 líneas. 1 test nuevo (mtime_is_stale). `cargo test --workspace`: 97+52 OK, 0 warnings, v0.2.14.
- **L9** (2026-10-31, T5b): `lsp_service.rs` creado. `LspService` posee la conexión (`client`), el estado del menú de code actions y del diálogo de rename (5 campos antes en EditorApp). 6 métodos de protocolo movidos con parámetros (`&DocumentService`, `&EditorState`, `is_rust` resuelto por el orquestador): update_lsp, goto_definition, request_hover, request_code_actions, request_inlay_hints (#[allow(dead_code)] conservado), request_rename. Compromiso documentado: `poll_lsp_results`, `apply_code_action`, `apply_text_edits_to_active/tab` y `apply_workspace_edit` siguen en app.rs porque son el puente LSP→doc+AST+disco (open_path, update_ast hooks); el campo pasa a `self.lsp.*` (13 renames en ui.rs). 97+52 OK, 0 warnings, v0.2.15.
- **L10** (2026-10-31, T5c): `cargo_service.rs` creado. `CargoService` posee `runner` (el subproceso y el streaming ya viven en `karkinos-core/src/cargo.rs: CargoRunner`; el blueprint de S5 en este punto ya estaba hecho) + estado del panel (`panel_open`, `active_tab`). `CargoService::run(action, dir)` encapsula spawn; app.rs mantiene el orquestador `run_cargo` que resuelve el `dir` (archivo activo o `browse_dir`) y `jump_to_cargo_diagnostic` (salta vía open_path — puente doc+editor). 26 renames de campo en ui.rs. 97 OK, 0 warnings, v0.2.16.
