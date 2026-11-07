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

## 🐧 Install on Arch Linux (AUR)

Karkinos is packaged on the AUR as `karkinos-bin`: it installs the prebuilt Linux binary from the latest GitHub release, so a system update also updates Karkinos.

```bash
yay -S karkinos-bin      # or: paru -S karkinos-bin
```

The package `provides` and `conflicts` with `karkinos`, so it replaces a previous source build instead of living next to it. Once installed, `yay -Syu` / `paru -Syu` keeps it up to date with the rest of the system. When Karkinos was installed this way, its in-app update button does not replace `/usr/bin/karkinos`; it runs `yay -S karkinos-bin` (or `paru -S karkinos-bin`) in the integrated terminal.

### Maintaining `karkinos-bin`

Publishing is automatic. The `aur` job in `.github/workflows/release.yml` (`scripts/release-workflow.yml` is the same file) takes `packaging/aur/karkinos-bin/PKGBUILD`, sets `pkgver` from the tag, replaces `sha256sums` with the SHA-256 of the four release files, regenerates `.SRCINFO` with `makepkg --printsrcinfo` and pushes both to `ssh://aur@aur.archlinux.org/karkinos-bin.git`. It only runs when the `AUR_SSH_PRIVATE_KEY` secret exists; without it the job is skipped and the release still publishes.

In the repository the PKGBUILD is a template: `pkgver` is a local reference and `sha256sums` is `('SKIP' 'SKIP' 'SKIP' 'SKIP')` because the checksums can only be computed from the published assets. Never publish that `SKIP` version; the workflow always fills it in. To inspect the current metadata locally: `cd packaging/aur/karkinos-bin && makepkg --printsrcinfo`.

One-time setup, for the maintainer:

1. **Create the AUR account** at <https://aur.archlinux.org/register/>.
2. **Create a key pair used only by CI** (no passphrase, because the runner cannot type one):
   ```bash
   ssh-keygen -t ed25519 -C "karkinos release" -f ~/.ssh/karkinos_aur -N ""
   ```
3. **Register the public key**: AUR → *My Account* → *SSH Public Key*, paste the whole content of `~/.ssh/karkinos_aur.pub` and save. (An account can hold several keys, so this does not break your personal one.)
4. **Create the `karkinos-bin` package** once, if it does not exist yet: either push the first `PKGBUILD` + `.SRCINFO` yourself to `ssh://aur@aur.archlinux.org/karkinos-bin.git` (the `aur` job does exactly that on the first release), or upload a `PKGBUILD` at <https://aur.archlinux.org/submit/>. After the first push the workflow keeps it updated.
5. **Add the repository secret**: GitHub → *Settings* → *Secrets and variables* → *Actions* → *New repository secret*, name `AUR_SSH_PRIVATE_KEY`, value = the complete content of the private key file `~/.ssh/karkinos_aur`, including the `-----BEGIN` and `-----END` lines.
6. **Tag a release** (`git tag vX.Y.Z && git push origin vX.Y.Z`) and check that the *Publish karkinos-bin to the AUR* job is green.

Never commit the private key. The workflow passes it through an environment variable (never interpolated into a script, so it cannot leak into the log), writes it to a `0600` file inside the ephemeral runner and deletes that file when the step ends.

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
