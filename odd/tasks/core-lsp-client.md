# Feature: Headless LSP Client & Live Diagnostics (Phase 4)

**Objective**:
Integrate an asynchronous Language Server Protocol (LSP) client into `karkinos-core` and connect it with the `karkinos` GUI:
1. JSON-RPC 2.0 communication engine running over asynchronous background channels without blocking UI frames.
2. Server process lifecycle management (`initialize`, `initialized`, `shutdown`, `exit`).
3. Document sync: `didOpen`, `didChange`, `didClose`.
4. Live diagnostics: parse `textDocument/publishDiagnostics` into ranges with severities (Error, Warning, Info, Hint) and render visual indicators in the editor gutter.
5. Interactive LSP capabilities: Hover tooltip/info (`textDocument/hover`) and autocompletion (`textDocument/completion`).
6. Zero UI lag or blocking, graceful degradation if no language server is installed on the host.

**Branch**: `main`
**Test runner**: `cargo test --workspace`

## Specs

- **S1** — LSP JSON-RPC & Protocol types in `karkinos-core`:
  - Request, Response, Notification serialization/deserialization.
  - Core structures: `Position`, `Range`, `Diagnostic`, `DiagnosticSeverity`, `CompletionItem`, `Hover`.
- **S2** — Asynchronous `LspClient` Engine:
  - Spawn background server process (e.g. `rust-analyzer`), communicate via stdin/stdout.
  - Thread-safe channels for non-blocking outbound requests and inbound notification dispatch.
- **S3** — Document & Editor Integration:
  - Sync editor document events (`didOpen`, `didChange`) with the LSP client.
  - Store active diagnostics in `EditorState` and render markers in the gutter.
- **S4** — Hover & Completion UI:
  - Query hover information at cursor point and display details.
  - Completion suggestions overlay or triggerable suggestions list.
- **S5** — Test verification & stability:
  - Unit tests verifying JSON-RPC parsing, diagnostic mapping, mock server round-trips.
  - Zero warnings and 100% test pass across workspace (98 tests passing).

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | completado | |
| T2 | S2 | inline | completado | |
| T3 | S3 | inline | completado | |
| T4 | S4 | inline | completado | |
| T5 | S5 | inline | completado | |

## Log

- **L1** (2026-10-30): Initialized Phase 4 LSP Client task.
- **L2** (2026-10-30): Implemented LSP JSON-RPC 2.0 types and framing in `crates/karkinos-core/src/lsp.rs`.
- **L3** (2026-10-30): Built background reader/writer threads and asynchronous dispatch in `LspClient`.
- **L4** (2026-10-30): Integrated live diagnostics, hover display, and completion overlay into GUI and `EditorApp`.
- **L5** (2026-10-30): Verified workspace with 98 unit tests and 0 compiler warnings.
