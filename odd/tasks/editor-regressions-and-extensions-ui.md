# Feature: Regresiones del editor (cursor y lag) y panel de extensiones moderno

**Objetivo**: corregir el cursor desplazado una columna a la izquierda, recuperar la fluidez perdida al teclear después de T7 (Rope como única fuente) y rediseñar el panel de extensiones con un aspecto moderno.
**Branch**: `main` (commits convencionales, SIN líneas de atribución AI / Co-Authored-By).
**Runner de tests**: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` (hoy ambos en verde: 236 + 149 tests).
**Versionado**: cada tarea cerrada sube el patch (`0.2.72` → `0.2.73` …) en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml` y `scripts/PKGBUILD`.

## Reglas para el implementador (leer antes de cada tarea)

1. Hacé UNA tarea por vez, en el orden de la tabla. Un commit por tarea.
2. Leé completos los archivos y rangos que nombra la tarea antes de tocar código. Los números de línea son de v0.2.72 y pueden correrse: buscá el símbolo nombrado.
3. Test primero: escribí el test pedido, verificá que FALLA (RED), implementá, verificá que PASA (GREEN). Si el test pedido no falla en RED, NO inventes el arreglo: anotá en el Log qué observaste y pasá a la siguiente hipótesis de la tarea.
4. Reutilizá los helpers que se nombran. No reimplementes la misma lógica con otra forma.
5. No cambies comportamiento que la tarea no pide. No reformatees archivos enteros. No agregues dependencias.
6. Comentarios en español, explicativos, como el código existente. Textos de UI en inglés.
7. Al cerrar cada tarea: tests y clippy en verde, versión subida, tarea marcada `hecha` en la tabla, entrada nueva en `## Log` con la evidencia (RED/GREEN y números), commit con el mensaje sugerido.
8. Tests con directorios temporales únicos y borrados al final, como los existentes.

## Specs

- **S1 — Lag al teclear.** El usuario reporta: "tiene menos rendimiento, parece lagueado". Una tecla en un documento grande no debe reconstruir el texto completo ni el `Rope` más de lo estrictamente necesario. En particular, `sync_active_to_tab` no puede reconstruir el `Rope` de la pestaña desde un `String` en cada edición.
- **S2 — Cachés de filas sin asignaciones por línea.** Recalcular `row_counts` / `char_lens` (al apretar Enter, pegar o borrar líneas) no debe crear un `String` por cada línea del documento.
- **S3 — Cursor desplazado.** El usuario reporta: "el cursor se desplaza 1 a la izquierda". El caret (y la selección y el clic) tienen que quedar alineados exactamente con los glifos dibujados, en cualquier columna y en cualquier tamaño de zoom.
- **S4 — Panel de extensiones moderno.** El usuario pide: "mejorar el aspecto de la parte de extensiones por algo más moderno". Mismas funciones que hoy (Installed / Open VSX / Showcase, buscar, descargar, borrar), con un diseño de tarjetas, control segmentado, buscador con estilo propio, estados vacíos y de carga, y animaciones de hover iguales a las del explorador.

## Tasks

| ID | Specs | Riesgo | Estado | Commit sugerido |
|----|-------|--------|--------|-----------------|
| T1 | S1 | Medio | hecha | `perf(tabs): share the rope when syncing the active tab` |
| T2 | S1 | Medio | hecha | `perf(editor): debounce full-text consumers after each keystroke` |
| T3 | S2 | Medio | hecha | `perf(editor): rebuild row caches without per-line allocations` |
| T4 | S3 | Medio | hecha | `fix(editor): align caret, selection and clicks with rendered glyphs` |
| T5 | S4 | Bajo | hecha | `feat(extensions): modern card-based extensions panel` |

---

### T1 — Compartir el `Rope` al sincronizar la pestaña activa (S1)

**Causa encontrada.** `karkinos/src/ui.rs` (~3640), dentro de `if response.changed`, llama a `self.sync_active_to_tab()` en CADA edición. `karkinos/src/app.rs`, `sync_active_to_tab` (~1107):
```rust
let text = self.doc.text();          // Rope -> String completo (O(n))
...
tab.set_text(&text);                 // String -> Rope nuevo (O(n), más caro)
```
Antes de T7 esto era `tab.buffer = self.doc.buffer.clone()`, que en ropey es O(1) (el `Rope` comparte nodos con `Arc`). T7 lo volvió dos copias completas por tecla. Además clona `undo_stack` completo en cada tecla.

**Cómo hacerlo.**
1. En `karkinos/src/document_service.rs`:
   - En `DocumentService` agregá `pub fn buffer(&self) -> &TextBuffer { &self.buffer }`.
   - En `TabDocument` agregá `pub fn set_buffer(&mut self, buffer: TextBuffer) { self.buffer = buffer; self.recalc_view_bytes(); }` (si `recalc_view_bytes` ya se llama aparte, no lo dupliques).
   - Verificá que `TextBuffer` implemente `Clone` (está en `crates/karkinos-core/src/buffer.rs`); si no, derivalo: clonar un `Rope` es O(1).
2. En `sync_active_to_tab` reemplazá `let text = self.doc.text();` + `tab.set_text(&text)` por `tab.set_buffer(self.doc.buffer().clone())`.
3. Hacé lo mismo en el camino inverso, donde la pestaña se carga en el documento activo (buscá en `app.rs` las llamadas `self.doc.set_text(&...tab...text())` o `tabs[idx].text()` ~1163). Agregá `DocumentService::set_buffer(&mut self, TextBuffer)` si hace falta.
4. `undo_stack` NO se toca en esta tarea (anotalo en el Log como posible mejora futura).

**Tests (en `karkinos/src/app_tests.rs`).**
- `sync_active_to_tab_conserva_el_texto_exacto`: `set_text` con un texto con tildes, emojis y CRLF normalizado; `sync_active_to_tab()`; el `text()` de la pestaña activa es igual al del documento.
- `cambiar_de_pestana_y_volver_conserva_texto_y_cursor`: dos pestañas, editar la 1, cambiar a la 2 y volver: el texto de la 1 está intacto.
- Test de rendimiento marcado `#[ignore]` (para correr a mano con `cargo test -- --ignored`): documento de 200 000 líneas; 200 llamadas a `sync_active_to_tab()` deben tardar menos de 50 ms en total en release. Anotá el tiempo antes/después en el Log.

---

### T2 — No recalcular el texto completo en cada tecla (S1) — después de T1

**Causa encontrada.** En el mismo bloque `if response.changed` de `ui.rs` (~3626) se llaman, en cada tecla:
- `self.update_ast()` → `app.rs` `update_ast` (~1305) hace `self.doc.current_text()` (Rope → String completo) y además `current_language()` (~1098), que con documentos de hasta 100 líneas también materializa el texto entero.
- `self.update_lsp()` → `lsp_service.rs` (~47) hace `doc.current_text()` y `client.did_change(...)` con el documento completo.

**Cómo hacerlo.**
1. Agregá a `EditorApp` un campo `pending_full_text_sync: Option<std::time::Instant>`.
2. En `ui.rs`, dentro de `if response.changed`, reemplazá las llamadas directas a `update_ast()` y `update_lsp()` por `self.pending_full_text_sync = Some(Instant::now());`.
3. En el bucle por frame de `ui.rs` (donde está `self.doc.chunk_tracker.check_idle_timeout(now)`, ~2838), agregá: si `pending_full_text_sync` lleva más de **120 ms**, llamá `update_ast()` y `update_lsp()` una sola vez y poné `None`. Mientras esté pendiente, pedí `ctx.request_repaint_after(restante)` para que se dispare aunque el usuario no mueva nada (mismo patrón que `chunk_tracker.remaining_time`).
4. Antes de guardar, cambiar de pestaña, ir a definición, hover o formatear, si hay una sincronización pendiente, hacela en el momento (llamá a un helper `flush_full_text_sync(&mut self)` que haga lo del paso 3 sin esperar). Buscá los puntos de entrada: `save`, `switch_tab`/activación de pestaña, `request_hover`, `goto_definition`, `format_document`.
5. `current_language()`: cachealo. Guardá `(Option<PathBuf>, Language)` y recalculalo solo cuando cambia `current_file`/`file_path_input` o al abrir un archivo. Para la detección por contenido usá solo las primeras 100 líneas con `self.doc.line(i)`, NUNCA `self.doc.text()`.

**Tests.**
- `editar_no_dispara_ast_ni_lsp_hasta_el_debounce`: simulá una edición (`response.changed`) y verificá que `pending_full_text_sync` es `Some` y que el worker de AST no recibió pedido (si no hay forma de observarlo, exponé un contador `ast_requests` solo en `#[cfg(test)]`).
- `flush_full_text_sync_antes_de_guardar`: editar y guardar inmediatamente; después de `save`, `pending_full_text_sync` es `None`.
- Tests existentes de `current_language` (`app_tests.rs` ~979) siguen pasando.

---

### T3 — Cachés de filas sin un `String` por línea (S2)

**Causa encontrada.** `karkinos/src/editor_widget.rs`: `rebuild_row_counts` y `rebuild_char_lens` recorren TODAS las líneas con `lines.get_line(i)`, y `LineBuffer::get_line` para `TextBuffer` (~171) hace `line_without_newline` → `slice.to_string()`: una asignación por línea. Se ejecutan cada vez que cambia la cantidad de líneas (cada Enter, pegar, borrar líneas). Con `Vec<String>` (antes de T7) era una lectura directa.

**Cómo hacerlo.**
1. En el trait `LineBuffer` (`editor_widget.rs` ~128) agregá:
   ```rust
   /// Cantidad de caracteres de la línea `i` sin el salto final,
   /// sin crear un `String`.
   fn line_char_len(&self, i: usize) -> usize;
   ```
   - Para `TextBuffer`: delegá en `TextBuffer::line_char_count` (`crates/karkinos-core/src/buffer.rs` ~85), que ya existe y no asigna.
   - Para `Vec<String>` (tests): `self.get(i).map(|l| l.chars().count()).unwrap_or(0)`.
2. `rebuild_char_lens` usa `line_char_len`. `rebuild_row_counts`: si `row_count_for` solo necesita la cantidad de caracteres (y el ancho visual por tabs), creá `row_count_for_len(len, wrap_chars)` para las líneas SIN tabs; para detectar tabs sin asignar, agregá al trait `fn line_has_tab(&self, i: usize) -> bool` (en `TextBuffer`: `slice.chars().any(|c| c == '\t')` sobre `self.line(i)`). Solo las líneas con tab usan el camino lento con `get_line`.
3. Reemplazá también `line_char_count(&lines.get_line(x))` por `lines.line_char_len(x)` en los manejadores de teclas (Home/End, flechas, clic: ~1418, ~1447, ~1499, ~1911, ~1924, ~1983, ~1993) y en el minimap (~2817, ~2841). Es un cambio mecánico: mismo resultado, sin asignar.

**Tests (en `editor_widget.rs`, módulo de tests).**
- `line_char_len_coincide_con_get_line`: para `TextBuffer` y para `Vec<String>`, con líneas vacías, con tildes/emoji, la última línea sin `\n` y un buffer vacío: `line_char_len(i) == get_line(i).chars().count()` para todo `i`.
- `rebuild_row_counts_igual_que_antes`: un documento con tabs, líneas largas que se envuelven y líneas cortas: el resultado es idéntico al de la implementación anterior (copiá la función vieja dentro del test como referencia).
- Perf `#[ignore]`: 200 000 líneas, `rebuild_row_counts` + `rebuild_char_lens` < 30 ms en release. Anotá antes/después en el Log.

---

### T4 — Caret, selección y clic alineados con los glifos (S3)

**Hipótesis principal (verificar primero).** El caret se posiciona con aritmética de ancho fijo y el texto se dibuja con el layout real de egui:
- `editor_widget.rs` ~1306: `let char_width = ui.fonts(|f| f.glyph_width(&font_id, ' '));`
- ~2648: `caret_doc_x = gutter_w + PAD_X + (vcol - vstart) as f32 * char_width;`
- ~1426 (clic): `target_vcol = (rel_x / char_width).round()`
- El texto, en cambio, se pinta con galleys (`layout_no_wrap` / `layout_job`, ~2497), y egui redondea la posición de cada glifo a píxeles. Con Geist Mono (agregada en v0.2.60) el avance real por carácter puede diferir de `glyph_width(' ')` en fracciones de píxel; el error se acumula con la columna y el caret queda corrido. Mismo problema para el rectángulo de selección.

**Paso 1 — Diagnóstico (obligatorio, con evidencia en el Log).**
Escribí un test `caret_x_coincide_con_el_galley` en `editor_widget.rs`:
- Creá un `egui::Context`, instalá las mismas fuentes que `main.rs` (si la carga de Geist Mono está inline en `main.rs`, extraela a una función `pub fn install_fonts(ctx: &egui::Context)` en un módulo `fonts.rs` y usala desde `main.rs` y desde el test).
- Para tamaños 12, 14, 16, 20 y la línea `"let mut resultado = calcular(a, b) + 42; // comentario"`, compará `col as f32 * glyph_width(' ')` con la x real del glifo en el galley (`galley.pos_from_cursor(&egui::text::CCursor::new(col)).min.x`, o la API equivalente de la versión de egui del proyecto) para cada `col`.
- El test pasa si la diferencia máxima es < 0.5 px. Si FALLA (RED), la hipótesis queda confirmada: seguí al paso 2. Si PASA, anotá los números en el Log y probá las hipótesis secundarias de abajo.

**Paso 2 — Arreglo.**
1. Creá en `editor_widget.rs` un único helper para mapear columna ↔ x usando el layout real:
   ```rust
   /// x (relativa al inicio del texto) del borde izquierdo de la columna
   /// `col` según el galley que realmente se dibuja.
   fn x_for_col(galley: &egui::Galley, col: usize) -> f32
   /// Columna más cercana a `x` según el galley (para clics).
   fn col_for_x(galley: &egui::Galley, x: f32) -> usize
   ```
   Para obtener el galley de una línea usá el MISMO `state.layout_cache` + `line_layout_key` que usa el render (~2480), para no hacer un layout extra por frame.
2. Usá `x_for_col` en: posición del caret/smear (~2648), rectángulos de selección y de cursores múltiples, y subrayados de diagnósticos si usan `char_width`.
3. Usá `col_for_x` en el clic y el arrastre (~1426).
4. `char_width` sigue usándose SOLO para calcular `wrap_chars` y el ancho del gutter.
5. Respetá tabs: hoy existe `visual_col_at(&line, col, DEFAULT_TAB_SIZE)`; si el galley ya contiene los espacios del tab expandido, la columna del galley es la visual, no la lógica. Verificalo con un test que tenga un `\t` al inicio.

**Hipótesis secundarias (solo si el Paso 1 no falla).**
- **Clic redondeado.** `.round()` en ~1426 hace que un clic en la mitad derecha de un carácter caiga después de él; probá un test de clic en el 30% izquierdo de la columna 5 → debe quedar en la columna 5.
- **Desfase de texto vs caret por `text_left`.** Compará la x donde se pinta el galley (`painter.galley(egui::pos2(text_left, y), ...)`, ~2505) con la base del caret (`gutter_w + PAD_X` menos el scroll horizontal). Tienen que ser la misma expresión; si no, unificá en una variable.
- **Saltos de línea extra de ropey.** Ropey (feature por defecto `unicode_lines`) considera salto de línea también `\r` solo, `\u{0085}`, `\u{2028}` y `\u{2029}`. Test: `TextBuffer` con `"ab\u{2028}cd"` → si `len_lines()` es 2, el cursor se desfasa en archivos con esos caracteres. Arreglo: en `crates/karkinos-core/Cargo.toml` usar `ropey = { version = "1.6", default-features = false, features = ["simd", "cr_lines"] }` (y lo mismo en `karkinos/Cargo.toml`), o solo `["simd"]` si el archivo se normaliza a `\n` al abrir. Corré toda la suite.

**Tests de cierre.**
- `caret_x_coincide_con_el_galley` (paso 1) en verde.
- `clic_en_columna_n_deja_el_cursor_en_n`: para cols 0, 1, 10, 40 de la línea de prueba, un clic en el centro del glifo deja `cursor_col == col`.
- `caret_alineado_con_tab_inicial`.

---

### T5 — Panel de extensiones moderno (S4)

**Estado actual.** `karkinos/src/ui_extensions.rs` (`render_extensions_tab`): título "EXTENSIONS", sub-pestañas con `selectable_label` (Installed / Open VSX / Showcase), y listas hechas con `ui.group` + `horizontal_wrapped` + `small_button` con emojis. Funcional pero se ve como un formulario por defecto de egui.

**Principios de diseño (seguir al pie de la letra).**
- Solo tokens del tema: `theme.bg`, `theme.surface_bg`, `theme.border_subtle`, `theme.border_focus`, `theme.text`, `theme.text_muted`, `theme.accent`, `theme.selection` (`karkinos/src/theme.rs` ~166). Cero colores hardcodeados (el `Color32::from_rgb(80, 200, 120)` del "● Downloaded" se reemplaza por un token o por `theme.accent`).
- Hover animado con el mismo patrón del explorador: `ui.ctx().animate_bool_with_time(id, response.hovered(), 0.12)` + `crate::theme::lerp_color(a, b, t)` (ver `ui_explorer.rs` ~320 y `title_bar.rs` ~76).
- Radio de esquinas 8 px en tarjetas, 6 px en botones y buscador; espaciado vertical 8 px entre tarjetas; padding interno 10 px.
- Sin dependencias nuevas y sin imágenes de red (Open VSX trae íconos, pero no se descargan: usamos avatares generados).
- Todo debe funcionar en los 5 temas (Dark, Monokai, Dracula, One Dark y los que existan en `ThemeMode::ALL`).

**Cómo hacerlo.**
1. **Componentes reutilizables** en un módulo nuevo `karkinos/src/ui_components.rs` (registralo en `main.rs`):
   - `segmented_control(ui, theme, selected: &mut usize, labels: &[&str]) -> bool`: una pastilla con fondo `surface_bg` y borde `border_subtle`; la opción activa con fondo `selection` y texto `text`; las otras con `text_muted` y hover animado. Devuelve `true` si cambió.
   - `search_field(ui, theme, text: &mut String, hint: &str) -> egui::Response`: `TextEdit::singleline` sin marco propio, dentro de un `Frame` redondeado con ícono 🔍 a la izquierda, borde `border_focus` cuando tiene foco y `border_subtle` si no, y botón ✕ para limpiar cuando hay texto.
   - `avatar(ui, theme, name: &str, size: f32)`: círculo con color derivado de un hash del nombre (mezclá `theme.accent` con un tono por hash usando `lerp_color`) y la inicial en mayúscula centrada.
   - `pill_button(ui, theme, label: &str, kind: PillKind) -> egui::Response` con `PillKind::{Primary, Secondary, Danger}`: Primary = fondo `accent`, Secondary = fondo transparente con borde, Danger = texto rojo del tema (si no hay token de error, usá el color de diagnósticos de error que ya use el editor) y hover animado.
   - `extension_card(ui, theme, card: &ExtensionCard) -> CardAction`: ver punto 3.
   - `empty_state(ui, theme, icon: &str, title: &str, subtitle: &str)` y `loading_skeleton(ui, theme, rows: usize)` (rectángulos `surface_bg` con un brillo que pulsa usando `ui.input(|i| i.time)`; pedí `request_repaint` solo mientras se muestra).
2. **Encabezado:** título "Extensions" (no en mayúsculas) con tamaño 15 y peso fuerte + un contador gris de resultados/instalados a la derecha. Debajo, el `segmented_control` con "Installed", "Marketplace" (antes "Open VSX") y "Showcase". Mantené los mismos índices de `extensions_tab` (0, 2, 1) mapeándolos dentro del control para no tocar `app.rs` ni los tests.
3. **Tarjeta de extensión** (`ExtensionCard { title, publisher, version, description, downloads: Option<u64>, state: CardState }`, `CardState::{Available, Downloading, Downloaded, Active}`):
   - Fila 1: `avatar` 36 px | columna con título (fuerte, `text`) y debajo `publisher · v1.2.3` (`text_muted`, pequeño) | a la derecha el botón según estado: Available → `pill_button("Download", Primary)`; Downloading → spinner (`egui::Spinner`) + "Downloading…"; Downloaded/Active → chip "Downloaded"/"Active" + `pill_button("Delete", Danger)` que aparece solo en hover de la tarjeta.
   - Fila 2: descripción en `text_muted`, máximo 2 líneas con elipsis (`egui::Label::new(...).truncate()` o un `LayoutJob` con `max_rows = 2`).
   - Fila 3: descargas formateadas (`1.2M`, `34.5K`, `812`) con ícono ⬇. Agregá `fn format_count(n: u64) -> String` en `ui_components.rs`.
   - Fondo `surface_bg`; en hover, `lerp_color(surface_bg, selection, 0.35 * t)` y borde `border_focus` con alpha según `t`. Toda la tarjeta es clickeable (`Sense::click`), y el clic NO dispara la acción del botón.
   - `CardAction::{None, Download, Delete}` se devuelve al caller, que reutiliza las funciones existentes: `self.extensions.download_open_vsx_extension(...)` y `self.extensions.uninstall_vsix(&mut self.doc, ...)`. NO reimplementes descarga/borrado.
4. **Pestaña Installed:** dos secciones con subtítulo en versalitas pequeñas (`text_muted`): "PLUGINS" (con `plugin_manager`, igual que hoy) y "DOWNLOADED PACKAGES" (lista de `.vsix` como tarjetas compactas con el tamaño en KB/MB). Si ambas están vacías: `empty_state("🧩", "No extensions yet", "Browse the Marketplace to download packages.")` con un `pill_button("Open Marketplace", Primary)` que cambia a la pestaña Marketplace.
5. **Pestaña Marketplace:** `search_field` (Enter o botón busca, igual que hoy con `do_search`) + debajo los resultados como tarjetas. Mientras `open_vsx_searching`: `loading_skeleton(…, 4)`. Error: `empty_state("⚠", "Couldn't reach Open VSX", <error>)` + botón "Retry". Sin resultados: `empty_state("🔍", "No results", "Try another search term.")`.
6. **Pestaña Showcase:** las mismas tarjetas, con el chip "Coming soon" en lugar del botón.
7. Hacé que el `ScrollArea` tenga `auto_shrink([false, false])` y que las tarjetas ocupen todo el ancho disponible del panel lateral.
8. Mantené el estado de "Downloading" por extensión: agregá a `ExtensionsService` un `downloading: Option<String>` (id de la extensión) que se setea al iniciar `download_open_vsx_extension` y se limpia en `poll_open_vsx` al recibir el resultado.

**Tests.**
- `format_count`: `0`→`"0"`, `812`→`"812"`, `1_250`→`"1.3K"`, `34_500`→`"34.5K"`, `1_200_000`→`"1.2M"`.
- `avatar_color_es_determinista`: el mismo nombre da el mismo color; dos nombres distintos dan colores distintos (al menos para `"rust-analyzer"` y `"prettier"`).
- `downloading_se_limpia_al_terminar`: simulá un resultado en `open_vsx_download_rx` y verificá que `downloading` vuelve a `None`.
- Los tests existentes de extensiones (`app_tests.rs`, búsqueda, `uninstall_vsix`, path traversal) siguen pasando.
- Render sin pánico: un test que crea un `egui::Context`, corre un frame (`ctx.run(...)`) con `render_extensions_tab` en cada una de las 3 pestañas y en cada tema de `ThemeMode::ALL`, con listas vacías y con 3 resultados de ejemplo.

## Fuera de alcance

- Cargar o ejecutar extensiones `.vsix`.
- Descargar los íconos reales de Open VSX.
- Cambiar los atajos o el resto de los paneles laterales.
- Optimizar `undo_stack.clone()` en `sync_active_to_tab` (anotarlo en el Log si se mide como problema).

## Log

- **L1 (2026-11-06)** — Pedido original: "ahora crea un roadmap para resolver estos errores, el cursor se desplazaa 1 a la izquierda, tiene menos rendiiento, parece lagueado y mejorar el aaspecto de laparte de extenciones por algo mas moderno tiene que pensar que lo va a implementar opencode".
- **L2 (2026-11-06)** — Análisis previo (v0.2.72): (a) `sync_active_to_tab` hace `doc.text()` + `tab.set_text()` en cada tecla, reconstruyendo el `Rope` desde un `String` (antes de T7 era un clon O(1)); (b) `update_ast` y `update_lsp` materializan el texto completo en cada tecla; (c) `rebuild_row_counts`/`rebuild_char_lens` crean un `String` por línea vía `get_line`; (d) el caret usa `col * glyph_width(' ')` mientras el texto se dibuja con galleys de egui (posible desfase acumulado con Geist Mono). La causa (d) NO está confirmada: T4 empieza por un test de diagnóstico.
- **L3 (2026-11-06) — T1 hecha (v0.2.73).** Tests nuevos en `app_tests.rs`: `sync_active_to_tab_conserva_el_texto_exacto` y `cambiar_de_pestana_y_volver_conserva_texto_y_cursor` (GREEN; preservan comportamiento) y el perf `#[ignore]` `sync_active_to_tab_es_barato_con_documento_grande` (200 000 líneas, 200 sincronizaciones). **RED medido antes del cambio** (release): `1.439211827s` (≈7,2 ms por tecla) → fallaba el umbral de 50 ms. **GREEN después** (release): `30.548µs` para las 200 llamadas. Cambios: `DocumentService::buffer()`/`set_buffer()` y `TabDocument::buffer()`/`set_buffer()` (con `recalc_view_bytes`); `sync_active_to_tab` y `load_tab` comparten `TextBuffer` clonado (clon de `Rope` O(1)) en lugar de `doc.text()` + `set_text(&text)`. Suite: `cargo test --workspace` → 238 passed + 1 ignored (karkinos) y 149 passed (core); `cargo clippy --workspace --all-targets -- -D warnings` en verde. Nota: `tab.undo_stack = self.doc.undo_stack.clone()` sigue clonando el stack completo en cada sincronización; queda como posible mejora futura (fuera de alcance por la tarea).
- **L4 (2026-11-06) — T2 hecha (v0.2.74).** El AST y el LSP ya no se recalculan en cada tecla: `ui.rs` marca `pending_full_text_sync = Some(Instant::now())` dentro de `if response.changed` y el bucle por frame llama `poll_full_text_sync(now)`, que a los 120 ms aplica `update_ast()` + `update_lsp()` una sola vez (y pide `request_repaint_after(restante)`, más un `request_repaint_after(FULL_TEXT_SYNC_DEBOUNCE)` al marcar, porque el chequeo ocurre antes del render). `flush_full_text_sync()` se ejecuta antes de guardar (`save_document`/`save_document_as`, reemplazan los `doc.try_save`/`doc.save_as` de la UI), cargar/activar pestaña (`load_tab`), ir a definición, hover y formatear. `current_language()` cachea `(ruta, Language)` (inválido al cambiar de ruta, vía `RefCell` para conservar `&self`); sin ruta no cachea porque la detección depende del contenido. La muestra de detección se arma con `self.doc.line(0..100)`, nunca con `doc.text()`. `set_text` limpia el pendiente porque ya aplicó el texto completo. **RED**: no hay RED conductual observable desde la API pública (simular `response.changed` exige un frame real de egui y el hook no existía en v0.2.73: el test no compila contra el código previo); la causa está explícita en el call-site viejo `ui.rs:3626-3627` (`update_ast(); update_lsp();` dentro de `if response.changed`), no es una hipótesis. Tests nuevos en `app_tests.rs`: `editar_no_dispara_ast_ni_lsp_hasta_el_debounce` (marca pendiente, `ast_request_rev` intacto; antes del umbral devuelve `Some`; a los 120 ms aplica y `rev` sube exactamente 1) y `flush_full_text_sync_antes_de_guardar` (marca, `save_document`, `pending` queda `None`, `rev` +1, y el archivo en disco tiene el texto). **GREEN**: `cargo test --workspace` → 240 passed + 1 ignored (karkinos, +2) y 149 passed (core); `cargo clippy --workspace --all-targets -- -D warnings` en verde. Nota: `update_lsp` sigue serializando el documento completo (`did_change`), pero ahora solo tras la pausa o en los flushes, no por tecla.
- **L5 (2026-11-06) — T3 hecha (v0.2.75).** Las cachés de filas y largos ya no crean un `String` por línea. `LineBuffer` suma `line_char_len`, `line_has_tab`, `rebuild_row_counts`, `rebuild_char_lens` y `rebuild_caches` (default sobre los métodos por línea); los manejadores de teclas, el clic, `clamp_col` y el minimap usan `line_char_len` en lugar de `line_char_count(&get_line(..))`. El camino de producción (`TextBuffer`) sobreescribe las tres reconstrucciones y recorre el Rope secuencialmente con el nuevo `TextBuffer::lines()`: medir mostró que el costo dominante no era la asignación por línea sino la navegación del árbol con `line(i)` en cada llamada, así que los call sites que reconstruyen las dos cachés (Enter, pegar, borrar líneas) usan una sola pasada compartida. Las líneas con tabs del camino default siguen usando `get_line` (expansión visual), y en `TextBuffer` se expanden sobre el slice sin asignar. **RED/GREEN medido** con el perf `#[ignore]` (200 000 líneas, release): referencia de la implementación vieja `191.8 ms`; primera versión sólo con `line_char_len`/`line_has_tab` `101.2 ms`; con recorrido secuencial por caché `51.2 ms`; con la pasada única final `26.3 ms` (< 30 ms). Tests nuevos: `line_char_len_coincide_con_get_line` (Vec y TextBuffer, vacías, tildes/emoji, CRLF y última sin salto), `rebuild_row_counts_igual_que_antes` (Vec y TextBuffer, tabs, líneas largas que envuelven, cortas y un bloque plegado) y el perf `rebuild_caches_es_barato_con_documento_grande`. Nota: los tests de la API nueva no compilan contra v0.2.74 (mismo caso que T2); la evidencia RED es la medición de la referencia vieja y los intentos intermedios. Suite: `cargo test --workspace` → 242 passed + 2 ignored (karkinos, +2 y +1 ignored) y 149 passed (core); `cargo clippy --workspace --all-targets -- -D warnings` en verde.
- **L6 (2026-11-06) — T4 hecha (v0.2.76).** Hipótesis confirmada: el caret se ubicaba con `col * glyph_width(' ')`, pero egui redondea la posición acumulada de cada glifo a píxeles físicos (`text_layout.rs` hace `round_to_pixel` tras cada avance), así que el desfase crecía con la columna. **RED medido** con el test de diagnóstico (Geist Mono, línea de 54 columnas): `size=12 char_width=7.3846 → 20.385 px`; `size=14 → 16.308 px`; `size=16 → 16.308 px`; `size=20 → 0 px` (el avance cae justo en 12 px enteros). El clic usaba `(rel_x / char_width).round()`, que con el mismo error caía una columna a la izquierda (el reporte del usuario). **GREEN**: `x_for_col`/`col_for_x` mapean contra el galley real y el desfase medido es `0.000 px` en los cuatro tamaños. Cambios: `main.rs` extrae la carga de fuentes a `fonts::install_fonts` (usada también por los tests); el galley de cada fila visual se obtiene ANTES de pintar fondos y alimenta caret/smear, selección, cursores extra, búsqueda, inlay hints, delimitadores, píldora de fold y blame; el clic/drag resuelve la columna con `col_for_x` sobre un layout del fragmento clicado; `char_width` queda para `wrap_chars`, gutter y fallbacks de líneas fuera de pantalla. Tabs: egui no expande `\t` (dibuja un glifo de reemplazo), así que `syntax::expand_tabs` los convierte a espacios antes de tipografiar (PlainText y `build_layout_job`), y la clave del layout incluye la fase `vcol % 4` (`line_layout_key_with_tab_phase`) para no compartir galley entre fragmentos que arrancan en otra parada; el caret de la columna lógica 1 tras un tab inicial cae exactamente sobre el glifo `l`. Tests nuevos: `caret_x_coincide_con_el_galley` (4 tamaños, < 0.5 px; imprime el desfase viejo como evidencia), `clic_en_columna_n_deja_el_cursor_en_n` (0, 1, 10, 40 al 40% izquierdo del glifo), `caret_alineado_con_tab_inicial` y `expand_tabs_respeta_la_fase_de_arranque`. Suite: `cargo test --workspace` → 246 passed + 2 ignored (karkinos, +4) y 149 passed (core); `cargo clippy --workspace --all-targets -- -D warnings` en verde.
- **L7 (2026-11-06) — T5 hecha (v0.2.77).** Panel de extensiones rediseñado con tarjetas. Módulo nuevo `karkinos/src/ui_components.rs` (registrado en `main.rs`): `segmented_control`, `search_field` (borde `border_focus` con foco, ✕ para limpiar), `avatar`/`avatar_color` (hash FNV-1a mezclado con `theme.accent`), `pill_button` (`Primary`/`Secondary`/`Danger`), `extension_card` + `CardAction`, `compact_card`, `empty_state`, `loading_skeleton` y `format_count`; todos los colores salen de `Theme` y el hover usa `animate_bool_with_time(..., 0.12)` + `lerp_color` como el explorador. El único literal es `ERROR_COLOR` (`from_rgb(255, 90, 90)`), el rojo de diagnósticos que ya usa el editor, porque la paleta no tiene token de error. `ui_extensions.rs` se reescribió sobre esos componentes: encabezado "Extensions" 15 px + contador gris, control segmentado Installed/Marketplace/Showcase mapeado a los índices 0/2/1 sin tocar `app.rs`, Installed con PLUGINS + DOWNLOADED PACKAGES y estado vacío con botón "Open Marketplace", Marketplace con buscador (Enter o botón), `loading_skeleton` mientras busca, `empty_state` de error con Retry y de "No results", y Showcase con chip "Coming soon". `ExtensionsService` suma `downloading: Option<String>`, se setea al iniciar la descarga y se limpia en `poll_open_vsx` al recibir el resultado. **RED**: con `format_count` devolviendo `""` y `avatar_color` devolviendo siempre `theme.accent`, los 2 tests de `ui_components` fallaron (`""` vs `"0"`; ambos nombres con `Color32([222, 100, 50, 255])`); y con `downloading` seteado pero sin limpiar, `downloading_se_limpia_al_terminar` falló (`Some("rust-lang.rust-analyzer")` vs `None`). **GREEN** tras implementar ambos helpers y limpiar el estado en `poll_open_vsx`. Test nuevo de render: `render_extensions_tab_sin_panico_en_los_3_tabs_y_temas` corre un frame real de egui en las 3 pestañas × los 4 temas de `ThemeMode::ALL` × listas vacías / 3 resultados, sin pánico. Nota: el documento hablaba de 5 temas, pero `ThemeMode::ALL` define 4 (Dark, Monokai, Dracula, One Dark); se cubrieron todos los existentes. Suite: `cargo test --workspace` → 250 passed + 2 ignored (karkinos, +4) y 149 passed (core); `cargo clippy --workspace --all-targets -- -D warnings` en verde.
