# Feature: Correcciones de auditoría, optimizaciones y mejoras del editor GUI

**Objetivo**:
1. Resolver bugs de integridad de datos al cerrar ventana con múltiples pestañas y en confirmaciones de guardado.
2. Optimizar el bucle de renderizado eliminando la clonación masiva de líneas e historial en cada frame.
3. Incorporar "Guardar y cerrar" en pestañas, atajos de ciclado `Ctrl+Tab` / `Ctrl+Shift+Tab` y "Guardar como..." (`Ctrl+Shift+S`).
4. Añadir ergonomía de edición moderna: comentar/descomentar (`Ctrl+/`), duplicar línea (`Ctrl+D`) y selección por clic en el gutter.

**Branch**: `main` (commits convencionales directos sin atribuciones AI).
**Runner de tests**: `cargo test` dentro de `rope_editor_app_GUI`.

## Specs

- **S1** — Cierre seguro de ventana multi-pestaña:
  - Aceptación: `try_close` detecta si la pestaña activa o cualquiera de las pestañas en segundo plano tiene cambios sin guardar (`dirty`). El diálogo advierte si hay múltiples pestañas con cambios y previene la pérdida de datos.
- **S2** — Optimización de sincronización de pestañas por frame:
  - Aceptación: `self.sync_active_to_tab()` no clona vectores de líneas ni stacks de undo en cada frame ocioso; solo sincroniza estado completo en cambio de pestaña o mutación real.
- **S3** — Guardar y cerrar pestaña:
  - Aceptación: Diálogo de cierre de pestaña (`show_tab_close_confirm`) incluye acción "Guardar y cerrar" que guarda el documento antes de cerrarlo.
- **S4** — Navegación de pestañas por atajos:
  - Aceptación: `Ctrl+Tab` avanza a la siguiente pestaña y `Ctrl+Shift+Tab` retrocede a la anterior (cíclico).
- **S5** — "Guardar como..." (`Ctrl+Shift+S`):
  - Aceptación: Atajo `Ctrl+Shift+S` y botón en la toolbar para guardar el documento actual con un nuevo nombre o ruta.
- **S6** — Comentar / descomentar líneas (`Ctrl+/`):
  - Aceptación: Alterna comentarios de línea según el lenguaje detectado (`//` para Rust/C/C++/Java/JS, `--` para SQL, etc.).
- **S7** — Duplicar línea o bloque (`Ctrl+D`):
  - Aceptación: `Ctrl+D` duplica la línea actual bajo el cursor o el bloque seleccionado manteniendo la consistencia de undo.
- **S8** — Selección de línea completa desde el gutter:
  - Aceptación: Al hacer clic sobre el número de línea en el gutter se selecciona la línea completa.

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | listo | 91b75a8 |
| T2 | S2 | inline | listo | 91b75a8 |
| T3 | S3 | inline | listo | 91b75a8 |
| T4 | S4 | inline | listo | 91b75a8 |
| T5 | S5 | inline | listo | 91b75a8 |
| T6 | S6 | inline | listo | 91b75a8 |
| T7 | S7 | inline | listo | 91b75a8 |
| T8 | S8 | inline | listo | 91b75a8 |

## Log

- **L1** (2026-10-30): Planificación de las 8 tareas de correcciones y mejoras del editor GUI.
- **L2** (2026-10-30): T1-T8 resueltas en su totalidad (suite 73/73 pasando, 0 warnings).
  1. Cierre seguro multi-pestaña: `first_dirty_tab_index` y `dirty_tabs_count` evitan pérdida de trabajo al cerrar ventana o descartar pestañas.
  2. Optimización de frame: `sync_active_metadata_to_tab` elimina clonación de líneas y stacks de undo en frames ociosos.
  3. Pestañas: acción "Guardar y cerrar" en confirmación de cierre y ciclado cíclico `Ctrl+Tab` / `Ctrl+Shift+Tab`.
  4. Guardado: "Guardar como..." (`Ctrl+Shift+S`) y botón en barra de herramientas con reinicio de ruta anterior.
  5. Edición: comentar/descomentar con `Ctrl+/` según prefijo del lenguaje, duplicar con `Ctrl+D` y selección completa por clic en el gutter.
