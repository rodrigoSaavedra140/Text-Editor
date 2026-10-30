# Karkinos Text Editor

An ultra-lightweight, high-performance modern text editor written in Rust with `eframe`/`egui`.
Engineered to start in under 50 ms with minimal system resource consumption, native multi-tab support, balanced B-Tree Rope text buffers (`ropey`), fuzzy file navigation, lightweight multi-language syntax highlighting, and modern editing ergonomics. Runs natively on Linux (Wayland & X11) and under Windows WSLg.

## 📦 Project Structure

```text
karkinos/
├── Cargo.toml           # Package configuration + [profile.release] (LTO thin, strip)
├── assets/
│   └── logo.png         # Embedded logo with adaptive transparency cutout
└── src/
    ├── main.rs          # Entry point: custom frameless window and decoration setup
    ├── app.rs           # EditorApp: buffers, tabs, async save, font zoom, dialogs
    ├── buffer.rs        # Balanced B-Tree Rope text buffer engine with O(1) COW snapshots
    ├── editor_widget.rs # Band-rendered wrap editor widget, keyboard input, gutter, autoscroll
    ├── ui.rs            # egui layout: custom title bar, tabs, toolbar, modal dialogs
    ├── title_bar.rs     # Frameless title bar with vector icons and window controls
    ├── syntax.rs        # Syntax tokenizers and LayoutJob colorizer (10+ languages)
    ├── quick_open.rs    # Quick Open file search dialog (Ctrl+P) with fuzzy matching
    ├── theme.rs         # Theme palettes (Dark, Light, Monokai, Dracula)
    ├── util.rs          # Byte formatters, goto calculations, and WSL environment detection
    └── undo.rs          # UndoStack with idle chunking and O(1) Copy-On-Write Rope snapshots
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
- `Ctrl+S` — Save active document
- `Ctrl+Shift+S` — Save As...
- `Ctrl+Q` — Quit (with multi-tab unsaved changes protection)

### Editing and Navigation
- `Ctrl+Z` / `Ctrl+Y` — Undo / Redo
- `Ctrl+F` — Find & Replace bar
- `Ctrl+G` — Go to line number
- `Ctrl+/` — Toggle line or block comment
- `Ctrl+D` — Duplicate line or selection downward
- `Tab` / `Shift+Tab` — Indent / dedent selected block or line
- `Ctrl+=` / `Ctrl+-` or `Ctrl+Wheel` — Editor font size zoom
- **Gutter click (line numbers)** — Selects the entire clicked line

## 📄 License

This project is distributed under the terms of the [Business Source License 1.1 (BSL 1.1)](LICENSE).
It grants free use, copying, modification, and compilation for personal, educational, development, and non-commercial evaluation purposes. For any commercial production use or distribution, a commercial license issued by the copyright holder is required.
