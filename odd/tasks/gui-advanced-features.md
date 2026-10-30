# Feature: Funcionalidades avanzadas del editor GUI (Etapa 2)

**Objetivo**: implementar las 8 funcionalidades avanzadas seleccionadas por el usuario para `rope_editor_app_GUI` (eframe/egui 0.27):
1. Resaltado de la línea activa
2. Resaltado global de búsqueda (todos los matches de Ctrl+F)
3. Matching de delimitadores `()`, `[]`, `{}`
4. Indentación y desindentación de bloques (Tab / Shift+Tab multilínea)
5. Sistema de temas visuales (Dark, Light, Monokai, Dracula)
6. Resaltado de sintaxis ligero
7. Búsqueda rápida de archivos (Ctrl+P / Quick Open)
8. Pestañas / Múltiples documentos (Tabs)

**Branch**: `main` (commits directos convencionales).
**Runner de tests**: `cargo test` dentro de `rope_editor_app_GUI`.
**Estrategia de entrega**: implementación incremental en orden de dependencias arquitectónicas, suites pasando y 0 warnings en cada paso.

## Specs

- **S1** — Resaltado de la línea activa
  - Aceptación: dibuja una franja de fondo sutil sobre la línea lógica que contiene el cursor (abarcando todas sus sub-filas visuales si está envuelta). Visible solo cuando el editor tiene foco.

- **S2** — Resaltado global de todas las coincidencias de búsqueda
  - Aceptación: cuando la barra de búsqueda está activa y hay query, se pintan rectángulos de fondo en TODAS las coincidencias visibles en pantalla (color ámbar/amarillo translúcido), diferenciando la coincidencia actual en foco (naranja/azul más intenso).

- **S3** — Matching de delimitadores `()`, `[]`, `{}`
  - Aceptación: si el cursor está inmediatamente adyacente a un delimitador de apertura o cierre, busca y resalta visualmente el delimitador correspondiente (considerando anidación). Si no hay match (desbalance), indica visualmente el error.

- **S4** — Indentación de bloques (Tab / Shift+Tab multilínea)
  - Aceptación: con selección multilínea activa, Tab agrega sangría al inicio de cada línea seleccionada. Shift+Tab quita un nivel de sangría (tabulación o hasta 4 espacios). Preserva la selección multilínea expandida.

- **S5** — Temas visuales (Paletas de color)
  - Aceptación: definición de paleta tipada (fondo de texto, gutter, selección, cursor, línea activa, matches). Temas incluidos: Dark (default), Light, Monokai, Dracula. Selector en la barra y persistencia en Storage.

- **S6** — Resaltado de sintaxis ligero
  - Aceptación: lexer o tokenizer por regex/reglas para palabras clave, cadenas, números y comentarios (Rust, Markdown, JSON, etc.) renderizado con `LayoutJob` de egui respetando la paleta del tema activo.

- **S7** — Búsqueda rápida de archivos (Ctrl+P / Quick Open)
  - Aceptación: ventana modal con campo de texto que lista archivos del proyecto, filtra en tiempo real por coincidencia difusa (fuzzy) y permite abrir el archivo seleccionado con Enter o navegación con flechas.

- **S8** — Pestañas / Múltiples documentos (Tabs)
  - Aceptación: barra de pestañas sobre el editor que permite tener múltiples archivos abiertos simultáneamente. Cada pestaña mantiene su propio buffer, ruta, estado de cursor/scroll, historial de deshacer y bandera dirty. Botón para cerrar pestaña (con aviso si dirty).

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | listo | 9b1f340 |
| T2 | S2 | inline | listo | 2be1a15 |
| T3 | S3 | inline | listo | 9f14c29 |
| T4 | S4 | inline | listo | 86f018c |
| T5 | S5 | inline | pendiente | - |
| T6 | S6 | inline | pendiente | - |
| T7 | S7 | inline | pendiente | - |
| T8 | S8 | inline | pendiente | - |

## Log

- **L1** (2026-10-29): Selección de las 8 funcionalidades por el usuario vía question tool.
  Diseño de arquitectura y ordenamiento de tareas por dependencias (ergonomía de render -> edición -> temas -> sintaxis -> navegación -> arquitectura multi-tab).
- **L2** (2026-10-29): T1 terminado (commit 9b1f340, suite 49/49 pasando, 0 warnings).
  Franja de fondo sutil (`Color32::from_white_alpha(12)`) sobre la línea lógica activa bajo el cursor
  (abarcando todas sus sub-filas visuales si hay wrap), activa únicamente con foco del widget.
- **L3** (2026-10-29): T2 terminado (commit 2be1a15, suite 49/49 pasando, 0 warnings).
  Resaltado de todas las coincidencias visibles de búsqueda en el viewport (ámbar translúcido para
  matches secundarios y naranja intenso para el match actualmente enfocado). Búsqueda binaria O(log M)
  por línea en los matches ordenados y cero alocaciones (referencias prestadas en `SearchHighlight`).
- **L4** (2026-10-29): T3 terminado (commit 9f14c29, suite 50/50 pasando, 0 warnings).
  Matching de delimitadores `()`, `[]`, `{}`: helper puro `find_matching_bracket` con soporte de
  anidación, búsqueda bidireccional y detección de desbalance (cap defensivo 2000 líneas).
  Visualización con recuadro azul/cyan para pares coincidentes y alerta roja tenue para delimitadores
  huérfanos. Suite con test multilínea, anidado y desbalanceado.
- **L5** (2026-10-29): T4 terminado (commit 86f018c, suite 51/51 pasando, 0 warnings).
  Indentación de bloques: Tab en selección multilínea indenta todas las líneas seleccionadas y expande
  la selección para preservarla. Shift+Tab quita sangría (\t o hasta 4 espacios) tanto en bloque
  multilínea como en línea simple. Helpers puros `indent_line` y `dedent_line` con test unitario.
