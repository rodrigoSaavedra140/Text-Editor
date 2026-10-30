# Feature: Core Architecture Decoupling (Phase 2)

**Objective**:
Decouple the text editing engine into a standalone, zero-GUI crate `karkinos-core`, establishing a clean separation of concerns between headless business logic and the graphical interface:
1. Extract buffer management, undo/redo COW snapshots, line ending detection/serialization, text manipulation algorithms, and document lifecycle into `karkinos-core`.
2. Make `karkinos-core` 100% headless and fast to test without UI or windowing dependencies.
3. Refactor the `karkinos` GUI crate to consume `karkinos-core`, focusing solely on rendering, widgets, themes, and window management.
4. Maintain 100% regression test coverage.

**Branch**: `main`
**Test runner**: `cargo test --workspace`

## Specs

- **S1** — Workspace setup and `karkinos-core` crate scaffolding:
  - Root `Cargo.toml` workspace referencing `crates/karkinos-core` and `karkinos`.
  - `karkinos-core` depends only on `ropey` and standard library.
- **S2** — Extraction of core engine modules:
  - `buffer`: `TextBuffer` with balanced Rope operations and $O(1)$ snapshots.
  - `undo`: `UndoStack`, `ChunkTracker`, `EditAction`, `Edit`.
  - `text_ops`: Indentation, bracket matching, comment toggling, word navigation, char-byte conversions.
  - `ending`: `LineEnding`, detection, serialization, and default file naming.
  - `document`: Headless `Document` model tracking buffer, undo, disk mtime, and dirty state.
- **S3** — Migration of `karkinos` GUI to consume `karkinos-core`:
  - `karkinos/Cargo.toml` imports `karkinos-core`.
  - `app.rs`, `editor_widget.rs`, `ui.rs` use types from `karkinos_core`.
- **S4** — Verification and test suite preservation:
  - All unit tests pass across the workspace (`cargo test --workspace`).
  - Zero compiler warnings (`cargo check --workspace`).

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | listo | pending |
| T2 | S2 | inline | listo | pending |
| T3 | S3 | inline | listo | pending |
| T4 | S4 | inline | listo | pending |

## Log

- **L1** (2026-10-30): Initialized Phase 2 architectural task for Core vs GUI decoupling.
- **L2** (2026-10-30): Created root Cargo workspace and `crates/karkinos-core` standalone crate.
  Extracted `buffer`, `undo`, `ending`, `text_ops`, and `document` modules into `karkinos-core`.
  Updated `karkinos` GUI crate to consume `karkinos-core`.
  All 86 tests (66 in GUI + 20 in Core) passing with 0 warnings.
