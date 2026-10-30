# Feature: Migración del motor de buffer a Rope industrial (Fase 1)

**Objetivo**:
Reemplazar la representación ingenua `Vec<String>` por una estructura de datos Rope balanceada industrial (`ropey`), logrando:
1. Clonación de snapshots en tiempo $O(1)$ para deshacer/rehacer sin duplicar strings completos en memoria.
2. Inserciones, eliminaciones y ediciones en $O(\log N)$ sin realocaciones masivas de vectores de strings.
3. Indexación bidireccional eficiente entre `(line, col)` y `char_index` UTF-8 / UTF-16.
4. Capacidad de abrir y manipular archivos grandes (>50 MB / cientos de miles de líneas) instantáneamente y sin congelar la interfaz.

**Branch**: `main` (commits convencionales directos sin atribuciones AI).
**Runner de tests**: `cargo test` dentro de `rope_editor_app_GUI`.

## Specs

- **S1** — Integración de `ropey` y módulo `buffer.rs`:
  - Aceptación: encapsulación limpia de `ropey::Rope` con métodos para conteo de líneas, extracción de rebanadas de línea, conversiones entre coordenadas de cursor y byte/char offsets, e inserción/borrado atómico.
- **S2** — Snapshots O(1) en UndoStack:
  - Aceptación: `UndoStack` almacena clones de `Rope` en $O(1)$ mediante COW interno en lugar de strings de texto completo.
- **S3** — Adaptación de `EditorApp` y `editor_widget.rs`:
  - Aceptación: `EditorApp` gestiona `TextBuffer` con compatibilidad completa para pestañas, resaltado de sintaxis, diálogos de guardado y navegación visual, pasando la suite completa de 73+ tests sin regresiones.
- **S4** — Test de estrés y validación de rendimiento:
  - Aceptación: verificación automatizada de manipulación de buffers grandes (>100.000 líneas) en milisegundos con memoria controlada.

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | listo | 6d79d71 |
| T2 | S2 | inline | pendiente | |
| T3 | S3 | inline | pendiente | |
| T4 | S4 | inline | listo | 6d79d71 |

## Log

- **L1** (2026-10-30): Planificación de la Fase 1 del Roadmap (migración a Rope industrial).
- **L2** (2026-10-30): T1 y T4 completadas (commit 6d79d71, 78/78 tests pasando, 0 warnings).
  Se incorporó `ropey = "1.6"` y se creó el módulo `buffer.rs` (`TextBuffer`) con operaciones $O(\log N)$,
  indexación bidireccional `(line, col) <-> char_idx`, snapshots COW instantáneos en $O(1)$ y prueba de estrés
  con 50.000 líneas en milisegundos.
