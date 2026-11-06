# Feature: Dock inferior (colapso y altura compartida) y menú File en la barra superior

**Objetivo**: arreglar el botón de colapso y el redimensionado del dock inferior (terminal / diagnostics / debugger) y limpiar la barra superior agrupando las acciones de archivo, búsqueda y temas en un menú desplegable.
**Branch**: `main` (commits convencionales, SIN líneas de atribución AI / Co-Authored-By).
**Runner de tests**: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` (hoy ambos en verde: 250 + 2 ignored y 149 tests).
**Versionado**: cada tarea cerrada sube el patch (`0.2.77` → `0.2.78` …) en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml` y `scripts/PKGBUILD`.

## Reglas para el implementador (leer antes de cada tarea)

1. Hacé UNA tarea por vez, en el orden de la tabla. Un commit por tarea.
2. Leé completos los archivos y rangos que nombra la tarea antes de tocar código. Los números de línea son de v0.2.77 y pueden correrse: buscá el símbolo nombrado.
3. Test primero: escribí el test pedido, verificá que FALLA (RED), implementá, verificá que PASA (GREEN). Si el test pedido no falla en RED, NO inventes el arreglo: anotá en el Log qué observaste.
4. Reutilizá los helpers que se nombran. No reimplementes la misma lógica con otra forma.
5. No cambies comportamiento que la tarea no pide. No reformatees archivos enteros. No agregues dependencias.
6. Comentarios en español, explicativos, como el código existente. Textos de UI en inglés.
7. Al cerrar cada tarea: tests y clippy en verde, versión subida, tarea marcada `hecha` en la tabla, entrada nueva en `## Log` con la evidencia (RED/GREEN y números), commit con el mensaje sugerido. NO hagas push.
8. Tests sin efectos fuera de un directorio temporal único, y NUNCA arranques la terminal PTY real en un test.

## Specs

- **S1 — Ícono de colapso.** El usuario pide: "arregla el icono del collapse de la barra de abajo donde esta la terminal por el triangulo y saca la palabra collapse y deja el icono". El botón `"▾ Collapse"` del dock inferior pasa a ser solo un triángulo vectorial que apunta hacia abajo (sin texto, sin glifo de fuente), con el mismo hover animado que `icon_close_button`, y con el tooltip actual `"Close Console (Ctrl+`)"`.
- **S2 — Altura compartida del dock.** El usuario reporta: "cuando queres adaptar el tamaño de la barra de abajo con la conola anda bien pero cuando lo haces con la parte de debbug no anda y vuelve al tamaño default siempre, deja que el tamaño de esa parte sea igual para todo(bash, diagnostic, debugger)". La altura que el usuario elige arrastrando el borde del dock se mantiene igual en las tres pestañas (terminal, Diagnostics, Debugger) y no vuelve a otro valor al cambiar de pestaña ni al soltar el mouse.
- **S3 — Sin botón de minimap en la barra superior.** El usuario pide: "sacar el boton de toggle minimap". Se elimina el botón `🗺` de la barra de título. Se conservan el atajo `Ctrl+Shift+M`, el comando de la paleta y el chip `MAP` de la barra de estado (supuesto: el pedido se refiere al botón de la barra superior).
- **S4 — Menú File.** El usuario pide: "en la parte donde esta open que eso sea un menu desplegable donde tenga todo lo default como abrir, abrir como, guardar, guardar como y asi y el buscador de files ponelo ahi dentro para limpiar un poco los botones, los temas tambien metelos ahi". Los botones `"📂 Open…"` y `"💾 Save (Ctrl+S)"` de la franja de pestañas se reemplazan por un único botón desplegable `"📂 File"` con, en este orden: New Tab (Ctrl+T), Open File…, Open Folder…, separador, Save (Ctrl+S), Save As…, separador, Search Files… (Ctrl+P), Search in Workspace… (Ctrl+Shift+F), separador, submenú Theme (los temas de `ThemeMode::ALL`, con el actual marcado), separador, Close Tab (Ctrl+W), Exit. La pill `"🔍 Search files... (Ctrl+P)"`, el botón `🔎` y el `ComboBox` de temas desaparecen de la barra de título. Supuesto: "abrir como" se interpreta como Open Folder….
- **S5 — Botones de Cargo arriba a la derecha.** El usuario pide: "mueve los botones de cargo a la derecha arriba". Run, Debug, Check, Test y Clippy pasan al lado derecho de la barra de título, a la izquierda de los controles de ventana, en ese orden visual (Run primero, Clippy último), con las mismas acciones y tooltips.

## Tasks

| ID | Specs | Riesgo | Estado | Commit sugerido |
|----|-------|--------|--------|-----------------|
| T1 | S1, S2 | Bajo | hecha | `fix(dock): shared resizable height and triangle collapse icon` |
| T2 | S3, S4, S5 | Bajo | hecha | `feat(ui): file menu and right-aligned cargo actions` |

---

### T1 — Dock inferior: ícono triángulo y altura compartida (S1, S2)

**Causa encontrada (S2).** `karkinos/src/ui.rs`, `show_cargo_panel` (~1929), usa `egui::TopBottomPanel::bottom("cargo_panel").resizable(true).default_height(200.0).min_height(100.0).max_height(450.0)`. En egui 0.27.2 (`egui-0.27.2/src/containers/panel.rs`, `show_inside_dyn` ~684-760) el panel guarda como altura persistida `PanelState { rect }` donde `rect = inner_response.response.rect`, es decir **el rectángulo del contenido**, no el del panel. La pestaña terminal (`show_interactive_terminal`) ocupa todo el alto disponible, así que el alto arrastrado se conserva. Diagnostics y Debugger tienen contenido de alto fijo (un label, o `ui.columns` con `ScrollArea::max_height(140.0)`), así que en el frame siguiente el panel se encoge (o crece) a la altura del contenido: parece que "vuelve al tamaño default".

**Cómo hacerlo.**
1. En `show_cargo_panel`, al principio del closure de `.show(ctx, |ui| { ... })`, forzá que el contenido ocupe todo el alto del panel, para las tres pestañas: `ui.set_min_height(ui.available_height());`. Comentá por qué (el párrafo de la causa, resumido).
2. En la pestaña Diagnostics, la `ScrollArea::vertical()` debe usar `.auto_shrink([false, false])` para llenar el alto. En la pestaña Debugger, reemplazá los `.max_height(140.0)` de las tres `ScrollArea` (`dap_stack_scroll`, `dap_vars_scroll`, `dap_bps_scroll`) por el alto disponible de su columna (`.max_height(cols[i].available_height())` o equivalente calculado antes), con `.auto_shrink([false, false])`, para que el contenido acompañe la altura elegida.
3. No cambies `default_height`, `min_height` ni `max_height`, ni el id `"cargo_panel"` (las tres pestañas ya comparten el mismo panel y, por lo tanto, la misma altura persistida).
4. (S1) En `karkinos/src/title_bar.rs` agregá `pub fn icon_triangle_down_button(ui, hover_text, theme) -> egui::Response`, con la misma estructura que `icon_close_button` (18×18, hover `animate_bool_with_time(..., 0.12)`, fondo `theme.badge_bg`, color `lerp_color(theme.text_muted, theme.text, hover_t)`) y que dibuje solo un triángulo relleno apuntando hacia abajo con `egui::Shape::convex_polygon` (podés copiar el bloque del triángulo de `icon_collapse_button`, SIN la raya horizontal). En `show_cargo_panel` reemplazá `ui.small_button("▾ Collapse").on_hover_text("Close Console (Ctrl+`)")` por `icon_triangle_down_button(ui, "Close Console (Ctrl+`)", theme)`.

**Tests (en `karkinos/src/app_tests.rs` o en el `mod tests` de `ui.rs`, donde ya haya tests de render con `egui::Context`).**
- `dock_conserva_altura_default_en_diagnostics_y_debugger`: `EditorApp` de test (como los existentes), `cargo.panel_open = true`. Para `active_tab` en `[1, 2]` (NO uses 0: arrancaría la PTY), con un `egui::Context::default()` y un `RawInput` con `screen_rect` de 1200×800, corré 3 frames llamando a `show_cargo_panel(ctx, &theme)` y después leé `egui::containers::panel::PanelState::load(&ctx, egui::Id::new("cargo_panel"))`: su `rect.height()` debe ser `200.0` (± 1.0). RED esperado antes del arreglo: la altura cae al alto del contenido (≈100 en Diagnostics vacío). Anotá los números observados en el Log.
- `dock_misma_altura_al_cambiar_de_pestana`: mismo armado; 3 frames con `active_tab = 1`, cambiar a `active_tab = 2`, 3 frames más: la altura persistida es igual antes y después.
- Si `show_cargo_panel` es privado, hacelo `pub(crate)` solo para el test.

### T2 — Menú File, barra de título limpia y Cargo a la derecha (S3, S4, S5)

**Dónde.** `karkinos/src/ui.rs`, `update` (~2745): panel `"custom_title_bar"` (~3103-3213) y panel `"tab_bar"` (~3221-3394).

**Cómo hacerlo.**
1. (S3) Borrá el bloque `// UI-7 (v0.2.38): toggle del minimap.` del título (botón `🗺`). NO toques `toggle_minimap`, el chip `MAP` de la barra de estado, el atajo ni la paleta.
2. (S4) En la barra de título borrá la pill de Quick Open (`search_pill`), el botón `🔎` y el `ComboBox` `"theme_picker"`.
3. (S4) En `"tab_bar"`, reemplazá los botones `"📂 Open…"` y `"💾 Save (Ctrl+S)"` por `egui::menu::menu_button(ui, "📂 File", |ui| { ... })`. Guardá la respuesta del botón (`menu_button` devuelve `InnerResponse`, su `.response` es el botón) y seguí asignando `self.picker_button_rect = Some(resp.response.interact_rect)`, porque `show_picker_popup` se ancla a ese rectángulo. Ítems, en este orden (usá `ui.add(egui::Button::new("…").shortcut_text("…"))` para mostrar el atajo; cada clic llama a `ui.close_menu()`):
   - `New Tab` (`Ctrl+T`) → `self.new_tab()`
   - `Open File…` → `self.picker_open = true` (mismo selector que hoy abría el botón Open)
   - `Open Folder…` → `self.prompt_open_folder()`
   - separador
   - `Save` (`Ctrl+S`) → `self.save_document()`
   - `Save As…` → `self.save_document_as()`
   - separador
   - `Search Files…` (`Ctrl+P`) → `self.quick_open.request_open(&self.doc.browse_dir, &mut self.quick_open_services)` (exactamente lo que hacía la pill)
   - `Search in Workspace…` (`Ctrl+Shift+F`) → `self.open_workspace_search()`
   - separador
   - `ui.menu_button("🎨 Theme", |ui| { for mode in crate::theme::ThemeMode::ALL { if ui.radio(self.theme_mode == mode, mode.name()).clicked() { self.theme_mode = mode; ui.close_menu(); } } })`
   - separador
   - `Close Tab` (`Ctrl+W`) → `self.try_close_tab(self.doc.active_tab)`
   - `Exit` → `self.try_close(ctx)`
   Las acciones que necesitan `&mut self` dentro del closure: si el borrow checker se queja, juntá la acción elegida en una variable local (`enum` o `Option<FileMenuAction>`) y ejecutala después del closure, como hacen `switch_to`/`close_target` más abajo.
4. (S5) Mové los cinco botones de Cargo (`▶ Run`, `🐞 Debug`, `Check`, `Test`, `Clippy`, mismas acciones y tooltips) al closure `right_to_left` de la barra de título, después de `ui.add_space(6.0);` de los controles de ventana. Como el layout es de derecha a izquierda, agregalos en orden inverso (Clippy, Test, Check, Debug, Run) para que se vean Run … Clippy de izquierda a derecha. Actualizá el comentario `UI-2 (v0.2.20)` del panel para que describa la nueva distribución.
5. Actualizá la documentación de usuario donde se mencione la pill de búsqueda, el selector de temas o el botón de minimap de la barra de título (`grep -rn "Search files\|theme picker\|🗺" README* docs/ 2>/dev/null`), si existe.

**Tests.**
- `file_menu_acciones_basicas` (estado puro, sin egui): si extraés las acciones a `enum FileMenuAction` con `fn apply_file_menu_action(&mut self, action, ctx: Option<&egui::Context>)` (o similar sin `ctx` para todo salvo Exit), testeá que `OpenFile` deja `picker_open == true`, `OpenFolder` deja `open_folder_modal.is_some()`, `NewTab` suma una pestaña y `SearchFiles` deja `quick_open.open == true`. Si no extraés el enum, anotá en el Log por qué no hay test conductual y dejá el test de render de abajo.
- `title_bar_y_tab_bar_renderizan_sin_panico`: no es obligatorio si `update` exige un `eframe::Frame` real; en ese caso anotalo en el Log.

## Fuera de alcance

- Cambiar los atajos de teclado o la paleta de comandos.
- Rediseñar las pestañas del dock o el contenido del Debugger más allá de que acompañe la altura.

## Log

- **L1 (2026-11-06)** — Pedido original: "arregla el icono del collapse de la barra de abajo donde esta la terminal por el triangulo y saca la palabra collapse y deja el icono y cuando queres adaptar el tamaño de la barra de abajo con la conola anda bien pero cuando lo haces con la parte de debbug no anda y vuelve al tamaño default siempre, deja que el tamaño de esa parte sea igual para todo(bash, diagnostic, debugger) y sacar el boton de toggle minimap y en la parte donde esta open que eso sea un menu desplegable donde tenga todo lo default como abrir, abrir como, guardar, guardar como y asi y el buscador de files ponelo ahi dentro para limpiar un poco los botones, los temas tambien metelos ahi y mueve los botones de cargo a la derecha arriba". Después: "implementa con deepseek de opencode".
- **L2 (2026-11-06)** — Análisis previo (v0.2.77): la causa de S2 está confirmada leyendo `egui-0.27.2/src/containers/panel.rs` (`PanelState` se guarda con el rect del contenido). Supuestos: "abrir como" = Open Folder…; "el botón de toggle minimap" = el `🗺` de la barra de título (el chip `MAP` de la barra de estado se conserva).
- **L3 (2026-11-06)** — T1 implementada (S1, S2). RED: `dock_conserva_altura_default_en_diagnostics_y_debugger` falló antes del arreglo con altura persistida **100 px** en Diagnostics (el panel se encogía al rect del contenido); `dock_misma_altura_al_cambiar_de_pestana` ya pasaba porque ambas pestañas colapsaban a 100 px (no era discriminante, se mantiene como invariante). GREEN: ambos tests pasan con **200 px** (± 1). Arreglo: `ui.set_min_height(ui.available_height())` al inicio del closure de `show_cargo_panel`; `ScrollArea` de Diagnostics con `.auto_shrink([false, false])`; las tres `ScrollArea` del Debugger usan `.max_height(cols[i].available_height())` + `.auto_shrink([false, false])`. S1: nuevo `icon_triangle_down_button` en `title_bar.rs` (triángulo relleno, hover animado, sin texto ni raya) reemplaza `"▾ Collapse"` conservando el tooltip. Versión **0.2.78** en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml` y `scripts/PKGBUILD`. Evidencia: `cargo test --workspace` → 252 passed, 2 ignored + 149 passed; `cargo clippy --workspace --all-targets -- -D warnings` → verde. Commit: `fix(dock): shared resizable height and triangle collapse icon`.
- **L4 (2026-11-06)** — T2 implementada (S3, S4, S5). RED: `file_menu_acciones_basicas` no compilaba (`E0433: cannot find type FileMenuAction`, `E0599: method apply_file_menu_action not found`), porque el enum y el método no existían; al agregarlos pasó a GREEN (`test app::tests::file_menu_acciones_basicas ... ok`). Arreglo: `FileMenuAction` (9 variantes) + `apply_file_menu_action(action, ctx)` en `app.rs` (el `ctx` solo se usa para `Exit` → `try_close`, que pasó a `pub(crate)`); el menú `egui::menu::menu_button("📂 File", …)` en la franja de pestañas acumula la acción elegida y la aplica después del closure, conservando `picker_button_rect` anclado al botón. Ítems en el orden pedido con separadores y atajos: New Tab (Ctrl+T), Open File…, Open Folder…, Save (Ctrl+S), Save As…, Search Files… (Ctrl+P), Search in Workspace… (Ctrl+Shift+F), submenú 🎨 Theme (`ThemeMode::ALL`, `ui.radio` con el actual marcado), Close Tab (Ctrl+W), Exit. S3: eliminado el botón `🗺` de la barra de título (se conservan atajo, comando de paleta y chip `MAP`). S4: eliminados de la barra de título la pill de búsqueda, el botón `🔎` y el `ComboBox` `theme_picker`. S5: Run, Debug, Check, Test y Clippy pasaron al closure `right_to_left` (agregados al revés: Clippy … Run) a la izquierda de los controles de ventana; comentario UI-2 actualizado. Test de render `title_bar_y_tab_bar_renderizan_sin_panico` omitido: `update` exige un `&mut eframe::Frame` real, no construible en un test unitario. Docs: `docs/assets/screenshots/terminal-pty.svg` actualizado (pill → `📂 File` en la franja, Cargo a la derecha); README solo menciona atajos que se conservan. Versión **0.2.79** en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml` y `scripts/PKGBUILD`. Evidencia: `cargo test --workspace` → 253 passed, 2 ignored + 149 passed; `cargo clippy --workspace --all-targets -- -D warnings` → verde. Commit: `feat(ui): file menu and right-aligned cargo actions`.
