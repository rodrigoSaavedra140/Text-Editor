# Feature: Extensible Plugin & Scripting Engine (Phase 5)

**Objective**:
Equip Karkinos with an extensible plugin architecture in `karkinos-core` that allows community extensions, custom commands, and lifecycle hooks:
1. Decoupled `Plugin` trait and `PluginContext` for document lifecycle events (`on_load`, `on_document_open`, `on_document_save`, `on_text_change`, `on_command`).
2. `PluginManager` registry that coordinates active plugins, executes commands, and isolates plugin execution.
3. Built-in extensible plugins showcasing capability (`WordCountPlugin`, `CaseTransformPlugin`, `HeaderBannerPlugin`).
4. Integration with `EditorApp` and command dispatching.
5. Unit tests verifying plugin lifecycle, event propagation, and custom command execution.

**Branch**: `main`
**Test runner**: `cargo test --workspace`

## Specs

- **S1** — `Plugin` Trait & `PluginContext` in `karkinos-core`:
  - Definition of plugin hooks, capability flags, and mutable text context.
- **S2** — `PluginManager` Registry & Dispatcher:
  - Plugin registration, priority ordering, and error-tolerant event broadcasting.
  - Command palette registry for registered plugin actions.
- **S3** — Standard Plugin Implementations:
  - Implementations for text transformation, stats inspection, and formatting.
- **S4** — Editor & GUI Integration:
  - Expose plugin commands to `EditorApp` and status bar reporting.
- **S5** — Test verification & stability:
  - Unit tests verifying event propagation, command dispatch, and document manipulation.
  - Zero warnings and 100% test pass across workspace (103 tests passing).

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | completado | |
| T2 | S2 | inline | completado | |
| T3 | S3 | inline | completado | |
| T4 | S4 | inline | completado | |
| T5 | S5 | inline | completado | |

## Log

- **L1** (2026-10-30): Initialized Phase 5 Plugin Engine task.
- **L2** (2026-10-30): Implemented `Plugin` trait, `PluginContext`, and `PluginCommand` in `crates/karkinos-core/src/plugin.rs`.
- **L3** (2026-10-30): Implemented `PluginManager` registry and builtin plugins (`WordCountPlugin`, `CaseTransformPlugin`, `HeaderBannerPlugin`).
- **L4** (2026-10-30): Integrated `PluginManager` and "🔌 Plugins" dropdown menu into `karkinos` GUI.
- **L5** (2026-10-30): Verified workspace with 103 unit tests and 0 compiler warnings.
