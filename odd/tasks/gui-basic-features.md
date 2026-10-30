# Feature: Funcionalidades básicas faltantes del editor GUI

**Objetivo**: implementar las 10 funcionalidades básicas seleccionadas por el
usuario para `rope_editor_app_GUI` (eframe/egui 0.27), sin romper el
comportamiento observable existente.
**Branch**: `feat/gui-basic-features` (cortada de `main` @ cac1ded).
**Runner de tests**: `cargo test` dentro de `rope_editor_app_GUI`.
**Estrategia de entrega**: decidida por el usuario (2026-10-29) — **commits
directos a `main`** (el branch se descartó). TUI `rope_editor_app` eliminado
del repo por pedido explícito (commit b5e1928); queda solo el GUI.

## Specs

Fragments citados verbatim de la selección del usuario (question tool,
orden del prompt). El resto de la aceptación se marca como criterio, no como
 requisito nuevo del usuario.

- **S1** — Verbatim: "Aviso por cerrar sin guardar / recarga externa"
  - Aceptación: cerrar la ventana (X, Ctrl+Q) con `dirty == true` muestra
    un confirm con Guardar / Descartar / Cancelar; NADA se mata mientras
    el diálogo esté abierto (no correr el force-kill de 1s). Si el archivo
    en disco cambió de mtime desde la última lectura y el buffer está sin
    cambios sin guardar, avisar y ofrecer recargar. (Asumido: sin
    detección de conflicto de edición concurrente.)

- **S2** — Verbatim: "Buscar y reemplazar (Ctrl+F)"
  - Aceptación: barra de búsqueda apenas abajo del editor (next/prev,
    contador de matches; Replace + Replace All opcional). Esensible a
    mayúsculas con toggle. Navegar resultados mueve el cursor y hace
    scroll visible. Ctrl+F abre, Esc cierra. Sin resaltado de todos los
    matches (solo el actual) — decisión de alcance.

- **S3** — Verbatim: "Undo granular (por sesión de tipeo)"
  - Aceptación: Ctrl+Z ya no vuelve al último guardado entero; las
    ediciones se agrupan en chunks (timeout de inactividad ~700ms y/o
    corte por Enter/nueva línea/paste), con un test de la política de
    chunking. El tope de 200 se conserva.

- **S4** — Verbatim: "Números de línea + Ctrl+G"
  - Aceptación: gutter a la izquierda del área de texto con números
    alineados a la PRIMERA fila visual de cada línea lógica; ancho del
    gutter en función de dígitos del total. Ctrl+G abre input "ir a
    línea N" y posiciona el cursor + autoscroll. (Asumido: gutter fijo,
    sin números por sub-fila de línea envuelta.)

- **S5** — Verbatim: "PageUp/PageDown + Ctrl+←/→ + doble/triple click"
  - Aceptación: PageUp/PageDown mueven la vista y el cursor por página;
    Ctrl+←/→ saltan palabra (prev/next, con límites de palabra en ASCII+
    unicode alfanumérico, test puro); doble click selecciona palabra, triple
    click selecciona línea lógica.

- **S6** — Verbatim: "Auto-indent + Tab en el editor"
  - Aceptación: Enter hereda el prefijo de whitespace de la línea anterior
    (helper puro testado). Tab dentro de editor con foco inserta un tab;
    NO roba el foco hacia el siguiente widget. (Asumido: sin Shift+Tab
    dedent en este feature.)

- **S7** — Verbatim: "Ln,Col en la barra de estado"
  - Aceptación: "Ln X, Col Y" en la barra de status del GUI, 1-based,
    actualizado cuando se mueve el cursor. Col = índice de carácter dentro
    de la línea lógica (coherente con cursor_col existente).

- **S8** — Verbatim: "Botón Nuevo documento"
  - Aceptación: botón Nuevo en la toolbar: buffer vacío, dirty=false,
    current_file=None, título path vacío. Con dirty=true pide confirmación
    (mismo dialogo de S1). (Asumido: no abre ventanas nuevas — un solo
    documento.)

- **S9** — Verbatim: "Preservar saltos CRLF/originales"
  - Aceptación: al abrir, detectar el estilo del archivo (CRLF/LF mixto→LF)
    y guardarlo tal cual al guardar SIN \r dentro del buffer de edición
    (normalizar el buffer a \n y re-introducir \r\n al serializar).
    Files solo-LF quedan LF; CRLF quedan CRLF. Helper puro testado.

- **S10** — Verbatim: "Zoom de fuente (Ctrl+Scroll / +/-)"
  - Aceptación: Ctrl+Scroll sobre el editor y botones +/- en la barra
    cambian el tamaño de punto de la fuente del editor (clamp 8..32 px);
    persiste entre sesiones si egui persistence está disponible
    (Storage). Re-ajusta wrap_chars y el cálculo de scroll con el tamaño
    cambiado. (Asumido: solo la fuente del editor, no toda la UI.)

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1, S9 | inline | listo | 331a023 |
| T2 | S7, S6 | inline | listo | 3b03c37 |
| T3 | S4 | inline | listo | 0e9a8d4 |
| T4 | S5 | inline | listo | 307b523 |
| T5 | S3 | inline | listo | 468ca3a |
| T6 | S2 | inline | listo | 74a4b92 |
| T7 | S8 | inline | listo | 478c1d4 |
| T8 | S10 | inline | listo | dad8209 |

Nota de ruta: los 10 features comparten edit surfaces (editor_widget.rs y
ui.rs), así que NO hay writers paralelos — el padre escribe inline en serie.
Route exception registrada por el Writer trigger (superficies compartidas ≠
disjuntas).

## Log

- **L1** (verbatim): "quiero que leas todo el proyecto y lo optimices" —
  sesión previa (optimización, commiteada en main hasta cac1ded).
- **L2** (2026-10-29, verbatim): "ahora quiero que investigues que
  funcionalidades basicas le faltan a mi editor (solo trabaja en el gui) y
  me las menciones para aver cuales implementar"
- **L3** (2026-10-29): el usuario respondió al menú seleccionando LAS DIEZ
  opciones: "Aviso por cerrar sin guardar / recarga externa, Buscar y
  reemplazar (Ctrl+F), Undo granular (por sesión de tipeo), Números de
  línea + Ctrl+G, PageUp/PageDown + Ctrl+←/→ + doble/triple click,
  Auto-indent + Tab en el editor, Ln,Col en la barra de estado, Botón
  Nuevo documento, Preservar saltos CRLF/originales, Zoom de fuente
  (Ctrl+Scroll / +/-)".
- **L4** (2026-10-29, verbatim): "directo al main y borra todo lo que tenga
  que ver con la otra version la que es solo en terminal" — entrega:
  commits directos a main; borrado del crate `rope_editor_app` completo
  (src/, tests/, Cargo.toml/lock, textos sueltos) + README reescrito
  solo-GUI. Commit b5e1928.
- **L5** (2026-10-29): T1 terminado (commit 331a023, RED E0425 observado y
  luego GREEN; suite 22/22, warnings 0). Hallazgo de diseño: el caso
  "archivo borrado del disco" es `(disk=None, memo=Some)` en
  mtime_is_stale; (Some, None) significa "sin memo todavía" y por
  seguridad NOT notifica stale. El guardado también refresca el memo
  post-write para que NO aparezca el banner Recargar por un save propio.
  Cancelación del cierre del WM vía ViewportCommand::CancelClose
  (verificado presente en egui 0.27.2 y procesado por egui-winit).
  Pendiente: verificación interactiva del diálogo por el usuario (GUI).
- **L6** (2026-10-29): T2 terminado (commit 3b03c37, suite 24/24, warnings 0).
  Ln,Col 1-based en status bar; auto-indent en Enter hereda prefijo de
  whitespace; Tab inserta caracter gracias a Memory::set_focus_lock_filter
  desactivando el secuestro de foco de egui.
- **L7** (2026-10-29): T3 terminado (commit 0e9a8d4, suite 26/26, warnings 0).
  Gutter con números de línea alineados a primera fila visual, ancho dinámico
  por dígitos; diálogo Ctrl+G con prellenado y clamp; needs_autoscroll flag.
- **L8** (2026-10-29): T4 terminado (commit 307b523, suite 31/31, warnings 0).
  Funciones puras is_word_char, next_word_pos, prev_word_pos, word_range_at y
  page_target_row; navegación Ctrl+←/→ por palabra con soporte Shift; PageUp y
  PageDown con salto por páginas de filas visuales; doble click selecciona
  palabra y triple click selecciona línea lógica completa.
- **L9** (2026-10-29): T5 terminado (commit 468ca3a, suite 34/34, warnings 0).
  Undo granular: ChunkTracker con timeout de inactividad de 700ms repintando con
  request_repaint_after; pre_break_snapshot en Enter y Paste para aislar el tipeo
  previo de la mutación estructural; tope de 200 edits preservado; suite con 3
  tests puros de la política de chunking.
- **L10** (2026-10-29): T6 terminado (commit 74a4b92, suite 37/37, warnings 0).
  Buscar y reemplazar (Ctrl+F): barra inferior arriba del status bar con contador
  de matches (X/Y), navegación prev/next con autoscroll al match seleccionado,
  toggle sensible a mayúsculas (Aa), reemplazo individual y reemplazo global con
  soporte de deshacer (Ctrl+Z), y cierre con Esc o botón ✕. Suite con 3 tests.
- **L11** (2026-10-29): T7 terminado (commit 478c1d4, suite 39/39, warnings 0).
  Botón Nuevo documento: botón en toolbar y atajo global Ctrl+N. Si dirty == true
  abre diálogo de confirmación (Guardar / Descartar / Cancelar) antes de limpiar
  el buffer; buffer vacío, current_file = None, file_path_input vacío, dirty = false
  y reset de pila de undo. Suite con 2 tests.
- **L12** (2026-10-29): T8 terminado, suite 42/42 (RED observado: E0425 en
  los 3 tests nuevos de la política de zoom; luego GREEN), warnings 0.
  Zoom de fuente (S10): helper puro apply_zoom (paso entero, clamp 8..32),
  EditorState.zoom_in/zoom_out reutilizados; el widget ahora usa
  state.font_size en el FontId (antes 14.0 hardcodeado), así wrap_chars,
  gutter y autoscroll se re-ajustan solos al cambiar el tamaño. Ctrl+Scroll
  sobre el editor zoomea un paso por notch (umbral de 2.0 del delta);
  Ctrl+= / Ctrl+- global; botones − / + en la toolbar. Persistencia: solo
  el zoom via eframe::App::save → Storage ("editor_font_size"), carga en
  el closure de main con clamp defensivo. Sin `Risk: none` directo — el
  touched compartido con T1–T7 es el hovered-scroll del widget: sigue
  scrolleando normal sin Ctrl.
- **L13** (2026-10-29) bugfix: usuario reportó (verbatim) "cuando haago
  undo vuelve al principio de la pagina y no se quda en el lugar". Root
  cause: undo()/redo() restauraban el texto vía set_text(), que resetea
  cursor (0,0) y scroll (0) — correcto para abrir archivo / nuevo
  documento, regresivo para deshacer. Fix: helper
  restore_text_keeping_view (commit bf27ad9) — conserva scroll tal cual
  (la caché del widget lo recorta al nuevo máx) y cursor clampeado; si
  la línea vieja ya no existe, aterriza al FIN de la última línea nueva.
  Suite 44/44 (RED observado: 2 tests del failure, luego GREEN),
  warnings 0. open_path/new_document conservan el reset (comportamiento
  deseado).
- **L14** (2026-10-29) bugfix: usuario reportó (verbatim) "cuando en el
  abrir el buscador de archivos cada vez que se cambiia de carpeta se
  cierra la lista y no se actualiza ahi y hay que abrirlo cada vez que
  se cambia". Root cause: egui::ComboBox cierra el popup con CUALQUIER
  click que no sea el botón (popup.rs: widget_response.clicked_elsewhere
  en popup_above_or_below_widget) — navegar una carpeta ES un click
  adentro. Fix (commit 4bd2161): picker propio (botón "Abrir…" +
  egui::Area Foreground bajo el botón), estado picker_open en EditorApp;
  navegación de carpetas NO lo cierra, solo Esc / click completo afuera
  (helper puro picker_should_stay_open, testado) / elegir un archivo.
  El listado siempre re-dibuja con cached_list_dir de la carpeta
  actual. Suite 45/45, warnings 0.
- **L15** (2026-10-29) bugfix: usuario pidió verbatim "cuando guarda un
  archivo nuevo, no le pone nombre... que cada vez que se intente
  guardar un archivo nuevo que no tenga nombre, lo guarde como
  nuevo.txt". Fix (commit 632b526): save() con current_file None y
  file_path_input vacío elige "nuevo.txt" en browse_dir (helper puro
  default_new_file_path con exists inyectado); si existe, NO pisa
  datos: propone "nuevo (1).txt", (2), … El nombre vuela al input de
  la toolbar y el path queda ABSOLUTO (una recursión via
  file_path_input perdía el dir — reestructurado a una sola pasada).
  El test del diálogo S1 cambió de semántica: ahora el diálogo SÍ
  puede prometer el cierre porque el save arranca sobre un destino
  concreto (antes abortaba y el diálogo quedaba esperando un
  guardado imposible). Suite 47/47, warnings 0.
- **L16** (2026-10-29) bugfix: usuario reportó "el zoom no funciona" y
  aclaró "solo el ctrl++ pero el boton de la ui no funciona". Root cause:
  doble sync en el frame — el mirror state→app (post-editor, del
  Ctrl+Scroll) corría DESPUÉS del click del botón y REVERTÍA el
  app.font_size nuevo al valor viejo del state; los atajos se salvaban
  porque su sync state←app corría antes del editor. Fix (dd13d06): el
  sync state←app se movió a dentro de CentralPanel, justo antes de
  editor_widget::show() — atajos Y botones del mismo frame aplican en
  el mismo frame; el mirror post-editor queda solo para Ctrl+Scroll
  (cambia dentro de show()). No testable headless (orden de frames del
  UI) — verificación interactiva pendiente del usuario. Suite 47/47,
  warnings 0.
