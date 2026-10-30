# Feature: Optimizaciones de rendimiento y eficiencia del editor GUI

**Objetivo**: eliminar cuellos de botella de asignaciones en caliente en el render loop (60/144 FPS), acelerar indexación en líneas largas (O(1) fast-path) y reducir la huella de memoria del stack de deshacer, sin alterar el comportamiento funcional ni romper tests existentes.
**Branch**: `main` (commits directos convencionales).
**Runner de tests**: `cargo test` dentro de `rope_editor_app_GUI`.
**Estrategia de entrega**: commits directos a `main` por unidad de trabajo, suite completa pasando y 0 warnings en cada paso.

## Specs

- **S1** — Fast-path O(1) ASCII en `char_to_byte` y `line_char_count`
  - Aceptación: cuando una línea no tiene caracteres multi-byte (`line.len() == char_len`), `char_to_byte` resuelve en O(1) mediante clamp directo en vez de recorrer con `char_indices().nth()`. Preservar comportamiento idéntico con caracteres multi-byte UTF-8 (acentos, ñ, emojis). Tests puros de equivalencia.

- **S2** — Eliminación de asignaciones y clones en caliente en el render loop
  - Aceptación: `editor_widget.rs::show()` no debe alocar innecesariamente en cada frame. El gutter formatea los números de línea sin heap allocation descartable por frame; `FontId` no se clona en cada sub-fila; los eventos de entrada de egui solo se clonan si `!events.is_empty()`. Cero regresiones visuales.

- **S3** — Reducción de huella de memoria en `UndoStack`
  - Aceptación: compartir memoria inmutable (`Arc<str>` o deduplicación entre `before` y `after` contiguos) para que el estado $N$ no se duplique entre el `after` del edit anterior y el `before` del edit nuevo, reduciendo a la mitad el consumo de RAM del historial. Todos los tests de undo/redo deben pasar sin cambios de semántica.

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | listo | e4eb95c |
| T2 | S2 | inline | listo | 9403330 |
| T3 | S3 | inline | listo | fd29c45 |

## Log

- **L1** (2026-10-29): Auditoría de rendimiento del codebase GUI. Identificados 3 cuellos de botella:
  1. `char_to_byte` recorriendo caracteres en sub-filas de líneas largas.
  2. Asignaciones en el heap por frame en `show()` (números de gutter, `font_id.clone()`, clonado incondicional de eventos).
  3. Duplicación de buffers completos en `UndoStack`.
  Plan aprobado por el usuario e inicio de ejecución.
- **L2** (2026-10-29): T1 terminado (commit e4eb95c, suite 48/48 pasando, 0 warnings).
  `char_to_byte` ahora ejecuta en O(1) vía clamp directo cuando la línea es ASCII (`s.is_ascii()`).
  En el render loop de `show()`, si `line.len() == char_len`, la conversión de sub-fila a byte offsets
  se resuelve en O(1) directo sin recorrer caracteres. Test exhaustivo de equivalencia ASCII/UTF-8 agregado.
- **L3** (2026-10-29): T2 terminado (commit 9403330, suite 48/48 pasando, 0 warnings).
  En `show()`, el vector de eventos de entrada `ui.input(|i| i.events)` ahora solo se clona si
  `!events.is_empty()`, eliminando alocaciones y copias en el heap en frames ociosos con foco activo.
- **L4** (2026-10-29): T3 terminado (commit fd29c45, suite 49/49 pasando, 0 warnings).
  `Edit` y `EditorApp.last_snapshot` ahora usan `Arc<str>`. El estado $N$ se comparte inmutablemente
  entre el `after` del paso anterior y el `before` del paso nuevo (`Arc::clone` O(1) en vez de alocar y
  copiar todo el texto). Las operaciones `undo()` y `redo()` ahora transfieren estados entre pilas
  con cero copias de buffers en el heap. Test con `Arc::ptr_eq` incorporado.
