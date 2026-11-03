# Karkinos Text Editor

An ultra-lightweight, high-performance modern text editor written in Rust with `eframe`/`egui`.
Engineered to start in under 50 ms with minimal system resource consumption, native multi-tab support, balanced B-Tree Rope text buffers (`ropey`), fuzzy file navigation, lightweight multi-language syntax highlighting, and modern editing ergonomics. Runs natively on Linux (Wayland & X11) and under Windows WSLg.

## 📦 Project Structure

```text
karkinos/                  # GUI crate (eframe/egui)
├── Cargo.toml
├── assets/
│   └── logo.png           # Embedded logo with adaptive transparency cutout
└── src/
    ├── main.rs            # Entry point, native window setup, persisted storage
    ├── app.rs             # EditorApp: state, tabs, commands, palette, watchers
    ├── ui.rs              # Layout: title bar, sidebar, editor, dock, modals
    ├── editor_widget.rs   # Editor rendering, input, gutter, folding, minimap
    ├── title_bar.rs       # Frameless title bar with vector icons
    ├── syntax.rs          # Tokenizers, layout cache, colorizer (10+ languages)
    ├── theme.rs           # Theme palettes (Dark, Monokai, Dracula) and tokens
    ├── quick_open.rs      # Async fuzzy file finder (Ctrl+P)
    ├── document_service.rs# Tabs, buffers, save/close orchestration
    ├── lsp_service.rs     # LSP bridge: hover, code actions, rename, diagnostics
    ├── cargo_service.rs   # Cargo pipeline panel
    ├── dap_service.rs     # Debug Adapter Protocol panel
    ├── terminal_service.rs# Multi-tab PTY dock
    ├── ast_worker.rs      # Background tree-sitter parser
    ├── git_status_worker.rs
    ├── git_diff_worker.rs
    └── util.rs            # Byte formatters, goto calculations, WSL detection

crates/karkinos-core/      # UI-agnostic engine
└── src/
    ├── buffer.rs          # Rope text buffer with O(1) COW snapshots
    ├── undo.rs            # Chunked undo stack
    ├── document.rs        # Document model
    ├── text_ops.rs        # Editing primitives
    ├── ending.rs          # Line-ending detection and serialization
    ├── syntax_ast.rs      # Tree-sitter highlighting, scopes, symbols
    ├── workspace_search.rs# Workspace search, filters, and multi-file replace
    ├── git_status.rs      # Git status, commit/stage, unified diffs
    ├── git_diff.rs        # Myers line diff for gutter marks
    ├── lsp.rs             # LSP JSON-RPC types and client
    ├── dap.rs             # DAP client
    ├── cargo.rs           # Cargo runner
    ├── terminal.rs        # PTY terminal emulator
    ├── plugin.rs          # Plugin trait and manager
    ├── open_vsx.rs        # Open VSX registry client
    └── platform.rs        # Platform helpers
```

## ⚙️ Requirements

- [Rust and Cargo](https://rustup.rs/) (2021 edition).
- **Native Linux** (Wayland or X11) or **Windows with WSL / WSLg**.

## 🚀 Building and Running

```bash
cd karkinos

# Development mode
cargo run

# Open a file directly
cargo run -- src/main.rs

# Build optimized production binary
cargo build --release
```

The resulting executable will be located at `target/release/karkinos`.

## ⌨️ Main Keyboard Shortcuts

### Files and Tabs
- `Ctrl+N` — New document (prompts to save unsaved changes)
- `Ctrl+T` — New tab
- `Ctrl+W` — Close active tab
- `Ctrl+Tab` / `Ctrl+Shift+Tab` — Cycle through open tabs
- `Ctrl+P` — Quick Open project file finder
- `Ctrl+Shift+P` / `F1` — Command Palette (fuzzy search over every registered action)
- `Ctrl+Shift+F` — Search across every file in the workspace (case sensitive, whole word, regex)
- `Ctrl+Shift+H` — Workspace search with the multi-file replace field expanded
- `Ctrl+Shift+G` — Git Source Control tab (status, stage all, commit, per-file diff)
- `Ctrl+Shift+O` — Jump to a symbol of the current document (functions, structs, enums, traits, methods, constants)
- `Ctrl+Shift+M` — Toggle the right-side minimap (off by default so the editor uses the full width)
- `Ctrl+S` — Save active document
- `Ctrl+Shift+S` — Save As...
- `Ctrl+Q` — Quit (with multi-tab unsaved changes protection)

### Editing and Navigation
- `Ctrl+Z` / `Ctrl+Y` — Undo / Redo
- `Ctrl+F` — Find & Replace bar
- `Ctrl+G` — Go to line number
- `Ctrl+/` — Toggle line or block comment
- `Ctrl+D` — Select the word under the caret; press again to add the next occurrence as an extra cursor
- `Alt+Click` — Add an extra cursor at the clicked position
- `Esc` — Collapse all extra cursors back to the primary one
- `Ctrl+Shift+D` — Duplicate line or selection downward
- `Tab` / `Shift+Tab` — Indent / dedent selected block or line
- `Ctrl+=` / `Ctrl+-` or `Ctrl+Wheel` — Editor font size zoom
- **Gutter click (line numbers)** — Selects the entire clicked line

### Layout and Panels
- `Ctrl+\` — Split the editor vertically (side by side); press again to close
- `Ctrl+Shift+\` — Split the editor horizontally (top / bottom)
- Drag the 6px divider to change the ratio
- Each pane keeps its own cursor and scroll over the same buffer
- `Ctrl+\`` — Toggle the Cargo console (moved from `Ctrl+\`)
- Terminal dock tabs: `＋` opens a new PTY, `✕` closes the active one; each tab keeps its own shell

## 📄 License

This project is distributed under the terms of the [Business Source License 1.1 (BSL 1.1)](LICENSE).
It grants free use, copying, modification, and compilation for personal, educational, development, and non-commercial evaluation purposes. For any commercial production use or distribution, a commercial license issued by the copyright holder is required.
