# Feature: Karkinos Modern Aesthetic UI Redesign

**Goal**: Transform Karkinos IDE into a visually stunning, modern, and ergonomic native code editor (drawing aesthetic benchmarks from Zed, Linear, Raycast, and VS Code) by establishing a cohesive design token system, frameless floating modal overlays, integrated headers, and refined micro-interactions in `egui`.
**Branch**: `main` (atomic commits per work unit).
**Versioning**: Bump minor/patch digits (`0.X.Y`), strictly preserving the first digit (`0`).
**Quality Gate**: Zero warnings under `cargo check --workspace` and 100% pass on all existing automated tests (`cargo test --workspace`).

---

## Executive Summary & Design System Architecture

The current visual experience suffers from three distinct architectural design flaws:
1. **Vertical Space Fragmentation**: Three separate stacked top panels (`custom_title_bar` 32px + `toolbar` 34px + `tab_bar` 28px = 94px) consume valuable vertical code editing real estate.
2. **Flat Contrast & Missing Elevation**: Themes use raw stark blacks (`#0a0a0a`, `#121212`) without border strokes or surface elevation layers, causing visual harshness.
3. **OS-Style Window Popups**: Quick Open (`Ctrl+P`), Code Actions (`Ctrl+.`), and Rename (`F2`) render as standard window frames with title bars instead of frameless, floating command palettes.

```
+-----------------------------------------------------------------------------------+
|  [Logo] Karkinos  |  [ 🔍 Search files or commands (Ctrl+P) ]  | [▶ Run] [🐞] [— □ ✕] |  <- Tier 1: 34px Unified Bar
+-----------------------------------------------------------------------------------+
|  [x] main.rs •  |  [x] theme.rs  |  [+]          |  Breadcrumb: karkinos > src     |  <- Tier 2: 30px Tabs & Breadcrumbs
+-----+-----------------------------------------------------------------------------+
|  01 | fn main() {                                                                 |
|  02 |     // Smooth active line highlight with left indicator                     |
| >03 |     let app = EditorApp::new();                                             |
|  04 | }                                                                           |
|     |                                                                             |
|     |         +---------------------------------------------------------+         |
|     |         | 🔍 [ Search files...                       ] [ESC] [↵] |         |  <- Floating Frameless Overlay
|     |         |---------------------------------------------------------|         |     (Backdrop veil dimmed)
|     |         | 📄 main.rs                          karkinos/src/main.rs|         |
|     |         | 📄 theme.rs                        karkinos/src/theme.rs|         |
|     |         +---------------------------------------------------------+         |
+-----+-----------------------------------------------------------------------------+
|  [⚡ Terminal] [🔍 Diagnostics (0)] [🐞 Debugger]                    [Collapse ▾] |  <- Segmented Bottom Dock
|  cargo check: Finished `dev` profile [optimized + debuginfo] in 0.12s             |
+-----------------------------------------------------------------------------------+
|  [Saved] [Ln 3, Col 12]  |  [🦀 Rust 2021]  [● LSP Ready]  [UTF-8]  [LF]          |  <- Status Bar: Pill Badges (22px)
+-----------------------------------------------------------------------------------+
```

---

## Implementation Work Units (Specifications UI-1 to UI-5)

### UI-1: Design System, Elevation Hierarchy & Theme Tokens
- **Target Files**:
  - `karkinos/src/theme.rs`
  - `karkinos/src/main.rs` (global `egui::Visuals` initialization)
- **Problem**:
  `Theme` currently only has flat background, text, and syntax colors. No elevation semantics (surface, elevated cards, border separation, badge fills).
- **Exact Implementation Orders**:
  1. In `karkinos/src/theme.rs`, extend the `Theme` struct:
     ```rust
     #[derive(Debug, Clone, Copy)]
     pub struct Theme {
         pub mode: ThemeMode,
         pub bg: egui::Color32,            // Canvas background (deep graphite)
         pub surface_bg: egui::Color32,    // Panels, toolbars, sidebars
         pub elevated_bg: egui::Color32,   // Floating modals, dropdowns, menus
         pub border_subtle: egui::Color32, // 1px panel boundaries
         pub border_focus: egui::Color32,  // Focused inputs and active tab stroke
         pub badge_bg: egui::Color32,      // Status bar chips and keyboard hints
         pub text: egui::Color32,          // Primary high-contrast text
         pub text_muted: egui::Color32,    // Secondary metadata, line numbers
         pub accent: egui::Color32,        // Brand color (Rust crab orange #de6432)
         pub gutter_bg: egui::Color32,
         pub gutter_text: egui::Color32,
         pub selection: egui::Color32,
         pub active_line: egui::Color32,
         pub caret: egui::Color32,
         pub title_bar_bg: egui::Color32,
         pub panel_bg: egui::Color32,
         // Syntax tokens...
         pub syntax_keyword: egui::Color32,
         pub syntax_type: egui::Color32,
         pub syntax_string: egui::Color32,
         pub syntax_comment: egui::Color32,
         pub syntax_number: egui::Color32,
     }
     ```
  2. Implement refined palette values in `ThemeMode::theme()`:
     - **Dark (Graphite Modern)**:
       - `bg`: `Color32::from_rgb(14, 17, 23)` (`#0e1117`)
       - `surface_bg`: `Color32::from_rgb(22, 27, 34)` (`#161b22`)
       - `elevated_bg`: `Color32::from_rgb(31, 36, 44)` (`#1f242c`)
       - `border_subtle`: `Color32::from_rgba_unmultiplied(255, 255, 255, 18)` (`#30363d`)
       - `border_focus`: `Color32::from_rgb(222, 100, 50)` (`#de6432`)
       - `badge_bg`: `Color32::from_rgba_unmultiplied(255, 255, 255, 14)`
       - `text`: `Color32::from_rgb(230, 237, 243)`
       - `text_muted`: `Color32::from_rgb(139, 148, 158)`
       - `accent`: `Color32::from_rgb(222, 100, 50)`
       - `active_line`: `Color32::from_rgba_unmultiplied(255, 255, 255, 6)`
     - **Monokai & Dracula**: Apply corresponding matching layered tones.
  3. Configure global `egui::Visuals` in `karkinos/src/main.rs`:
     - `visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0)`
     - `visuals.widgets.inactive.rounding = egui::Rounding::same(6.0)`
     - `visuals.widgets.hovered.rounding = egui::Rounding::same(6.0)`
     - `visuals.widgets.active.rounding = egui::Rounding::same(6.0)`
     - `visuals.window_rounding = egui::Rounding::same(10.0)`
     - `visuals.window_shadow = egui::epaint::Shadow { extrusion: 20.0, color: egui::Color32::from_black_alpha(150) }`

---

### UI-2: Unified Header & Streamlined Tab Strip
- **Target Files**:
  - `karkinos/src/ui.rs` (lines 1648–1879)
  - `karkinos/src/title_bar.rs`
- **Problem**:
  Currently, 3 distinct bars (`custom_title_bar`, `toolbar`, `tab_bar`) stack vertically, wasting 94px of editor height.
- **Exact Implementation Orders**:
  1. **Consolidate Top Panels into 2 Sleek Tiers**:
     - **Tier 1 (Title Bar & Global Actions — 36px)**:
       - Window dragging capability preserved across the entire bar rect.
       - Left: Logo image (18x18) + Bold "Karkinos".
       - Center: Clickable Search Pill Button:
         ```rust
         let search_pill = ui.add_sized(
             [260.0, 26.0],
             egui::Button::new(
                 egui::RichText::new("🔍 Search files... (Ctrl+P)")
                     .color(theme.text_muted)
                     .size(12.0),
             )
             .fill(theme.surface_bg)
             .rounding(egui::Rounding::same(6.0))
             .stroke(egui::Stroke::new(1.0, theme.border_subtle)),
         );
         if search_pill.clicked() {
             self.quick_open.request_open(&self.doc.browse_dir, &mut self.quick_open_services);
         }
         ```
       - Center-Right: Compact action buttons with subtle icons: `▶ Run (F5)`, `🐞 Debug`, `Check (F7)`.
       - Right: Theme dropdown + Extensions menu + native-styled minimize/maximize/close window controls from `title_bar.rs`.
     - **Tier 2 (Modern Tab Strip & Breadcrumb — 30px)**:
       - Background: `theme.surface_bg` with a bottom 1px separator `theme.border_subtle`.
       - Tab layout:
         - Active tab: rendered with `theme.bg`, rounded top corners (`Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 }`), 2px bottom accent highlight line (`theme.accent`), active file name in bold, dirty dot `●` in orange, and a subtle `×` close button that turns red on hover.
         - Inactive tabs: rendered transparently with `theme.text_muted`, hover background highlight.
         - New Tab button (`+`): round subtle button.
         - Breadcrumbs: On the right side of the tab strip, render breadcrumb tokens (`folder > subfolder > file.rs`) using `theme.text_muted`.

---

### UI-3: Frameless Floating Command Palette & Modal Overlays
- **Target Files**:
  - `karkinos/src/ui.rs`:
    - `show_quick_open` (lines 900–980)
    - `show_code_actions_modal` (lines 384–435)
    - `show_rename_dialog` (lines 443–485)
- **Problem**:
  Modals use standard `egui::Window` title chrome, making them look like OS utility popups instead of an integrated modern command palette (like Zed or Raycast).
- **Exact Implementation Orders**:
  1. **Fullscreen Backdrop Dimmer**:
     - Whenever a modal is active (`quick_open.open`, `code_actions_menu_open`, or `rename_dialog_open`), paint a soft dark veil over the entire screen:
       ```rust
       let screen_rect = ctx.screen_rect();
       ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("modal_backdrop")))
           .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(130));
       ```
  2. **Frameless Floating Card**:
     - Replace `egui::Window::new(...)` with a custom frameless window:
       ```rust
       egui::Window::new("quick_open_modal")
           .title_bar(false)
           .collapsible(false)
           .resizable(false)
           .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
           .fixed_width(560.0)
           .frame(
               egui::Frame::none()
                   .fill(theme.elevated_bg)
                   .rounding(egui::Rounding::same(10.0))
                   .stroke(egui::Stroke::new(1.0, theme.border_subtle))
                   .inner_margin(egui::Margin::same(12.0))
                   .shadow(egui::epaint::Shadow {
                       extrusion: 24.0,
                       color: egui::Color32::from_black_alpha(160),
                   }),
           )
           .show(ctx, |ui| { ... });
       ```
  3. **Command Palette Input Bar**:
     - Render a 36px input box with:
       - Prefix search glyph `🔍`
       - Single-line search text with placeholder
       - Right-aligned keyboard hint badge: `[ESC to close]` and `[↵ to select]` using `theme.badge_bg`
  4. **Item Results List**:
     - Row height: 32px with comfortable inner padding.
     - Hover/Selection state: rounded card background (`theme.surface_bg` or `theme.border_subtle` fill).
     - File name in high-contrast text (`theme.text`), parent directory path in muted secondary text (`theme.text_muted`).

---

### UI-4: Segmented Bottom Dock & Pill-Badged Status Bar
- **Target Files**:
  - `karkinos/src/ui.rs`:
    - `show_cargo_panel` (lines 595–650)
    - Status bar panel (lines 1883–1921)
- **Problem**:
  The Cargo panel is an unstyled dump of text, and the status bar is a plain string divided by vertical pipes.
- **Exact Implementation Orders**:
  1. **Segmented Bottom Dock Header**:
     - Replace flat buttons with a modern segmented pill switch:
       `[ ⚡ Terminal Output ]  [ 🔍 Diagnostics (N) ]  [ 🐞 Debugger ]`
     - Active segment: filled with `theme.surface_bg` with a 2px bottom accent bar (`theme.accent`).
     - Right side of panel header: "Clear Output" button (`🗑`), "Auto-scroll" toggle, and "Collapse" chevron button (`▾`).
     - Output log area: dark terminal background (`#080a0f`), monospace font, colored error/warning badges (`[ERROR]` in `#f85149`, `[WARN]` in `#e3b341`, `[SUCCESS]` in `#3fb950`).
  2. **Refined 22px Status Bar**:
     - Background: `theme.surface_bg` with a top 1px border `theme.border_subtle`.
     - Layout using pill-shaped micro-containers (`theme.badge_bg`, `Rounding::same(4.0)`):
       - Left:
         - Dirty indicator: `[Saved]` in muted green or `[Modified ●]` in warm orange.
         - Position pill: `[Ln 42, Col 18]`.
         - Size pill: `[12.4 KB]`.
       - Right:
         - LSP Status pill: `[● LSP Ready]` (green circle `#3fb950` when idle, yellow pulse `#e3b341` when indexing).
         - Language pill: `[🦀 Rust 2021]`.
         - Format pills: `[UTF-8]` and `[LF]`.

---

### UI-5: Editor Canvas & Gutter Micro-Interactions
- **Target Files**:
  - `karkinos/src/editor_widget.rs`
  - `karkinos/src/ui.rs` (`show_find_bar`)
- **Problem**:
  Active line highlighting is a flat alpha fill across the whole row; gutter breakpoints are text emojis (`🔴`); find bar is an intrusive bottom panel instead of a floating top-right widget.
- **Exact Implementation Orders**:
  1. **Active Line Highlight**:
     - In `editor_widget.rs`:
       - Background fill: Soft band (`Color32::from_rgba_unmultiplied(255, 255, 255, 8)`).
       - Left indicator: In the gutter, paint a vertical 2px colored accent bar (`theme.accent`) directly next to the active line number.
  2. **Geometric Breakpoints & Execution Pointer**:
     - Replace emoji breakpoint text with anti-aliased geometric primitives:
       ```rust
       // Breakpoint active: crisp red circle with subtle ring
       let center = egui::pos2(gutter_rect.left() + 8.0, row_y + row_height * 0.5);
       painter.circle_filled(center, 4.5, egui::Color32::from_rgb(220, 50, 50));
       painter.circle_stroke(center, 4.5, egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 100, 100)));

       // Gutter hover preview (ghost breakpoint):
       if is_gutter_hovered && !has_breakpoint {
           painter.circle_filled(center, 4.0, egui::Color32::from_rgba_unmultiplied(220, 50, 50, 80));
       }

       // Stopped line (debugger execution pointer):
       // Draw an anti-aliased golden triangle arrow (▶) pointing right
       ```
  3. **Floating Top-Right Find Widget**:
     - Convert `show_find_bar` from an `egui::TopBottomPanel::bottom` into a floating widget anchored at `RIGHT_TOP` inside the editor area (`x: -20.0, y: 10.0`), with rounded corners (8.0px), subtle shadow, and match counter chip (`3 of 15`).

---

## Task Matrix & Execution Plan for Peer Agent

| ID | Spec | Description | Target Files | Acceptance Criteria | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **T-UI1** | UI-1 | Implement Design Tokens, Elevations & Extended Theme Struct | `karkinos/src/theme.rs`, `main.rs` | All 3 themes expose surface/elevated/border colors; `egui::Visuals` rounding configured. Tests pass. | ✅ v0.2.19 |
| **T-UI2** | UI-2 | Consolidate Top Bar into 2-Tier Header with Search Pill & Modern Tabs | `karkinos/src/ui.rs`, `title_bar.rs` | Top panels merged from 94px to 66px total; active tab has bottom accent stroke; search pill opens QuickOpen. | ✅ v0.2.20 |
| **T-UI3** | UI-3 | Convert QuickOpen, CodeActions & Rename into Frameless Floating Modals | `karkinos/src/ui.rs` | Modals have dimmed screen backdrops, no window title bars, rounded corners (10px), shadow elevation, and keyboard badges. | ⬜ pending |
| **T-UI4** | UI-4 | Modernize Bottom Dock (Segmented Tabs) and Status Bar (Pill Chips) | `karkinos/src/ui.rs` | Bottom dock uses segmented tabs with colored log badges; status bar displays clean pill badges. | ⬜ pending |
| **T-UI5** | UI-5 | Polish Active Line Gutter Accent, Geometric Breakpoints & Floating Find | `karkinos/src/editor_widget.rs`, `ui.rs` | Active line has gutter accent; breakpoints drawn as anti-aliased vector circles; find widget is floating top-right. | ⬜ pending |
| **T-UI6** | UI-1 (review follow-ups) | Address the native review findings on T-UI1: Monokai/Dracula elevation order, muted-text contrast ≥ 4.5:1, named design constants and all five widget states rounded, a pure visuals helper with a headless test, pinned Dark gutter/title tokens, token role docs, spec translation notes | `karkinos/src/theme.rs`, `main.rs`, `odd/tasks/gui-modern-aesthetic-ui.md` | Monokai and Dracula keep canvas < surface < elevated; text and text_muted reach ≥ 4.5:1 on the surfaces they sit on; the visuals configuration is asserted by a test; zero warnings. | ⬜ pending |

---

## Log & Status Tracking
- **L1 (2026-10-31)**: Architecture audit completed. Specifications UI-1 through UI-5 drafted in detail for peer execution agent. Ready for execution without ambiguity.
- **L2 (2026-10-31, user, verbatim)**: "lee las instrucciones que dejo el otro agente y empieza a implementar sin pushear". Scope: implement UI-1..UI-5 locally on `main` as atomic work-unit commits, one version bump each (0.2.18 → 0.2.19 …). No push: publishing stays the user's decision. The landing page (`docs/index.html`) tracks published releases and is not bumped per work unit.
- **L3 (T-UI1, v0.2.19)**: Route: direct inline (one understood change; no parallel writers and no delegation trigger met). Done: `Theme` extended with the 7 elevation and accent tokens; Dark uses the exact Graphite Modern values (pinned by test); Monokai and Dracula get matching layered tones; global `Visuals` set in `main.rs` (widget rounding 6, window rounding 10, elevation shadow). Egui 0.27 translation: the spec's `Shadow { extrusion }` does not exist in this egui, so it becomes `Shadow { offset, blur, spread, color }`. The 7 new tokens have no reader until T-UI2..T-UI5; each carries a field-level `#[allow(dead_code)]` that its consuming unit removes. Test-first: not followed for this unit (tokens and palette were written before the tests), so no RED was observed; disclosed, not reconstructed. Evidence: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 99 passed, karkinos-core 52 passed. Differential check: N/A (no command or option touched). Risk: none (cosmetic token and style change; no data, security, contract, concurrency or delivery effect). Pixel-level appearance is not covered by tests; visual review is the user's.
- **L4 (T-UI1, native review)**: `review-2b2cef58fc657e86` on d6b390f. Assessed high by `process_boundary` (signal `shell_process`) from the pre-existing `std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1")` at `karkinos/src/main.rs:38`, not added by this unit; the classifier will flag any later change to `main.rs`. The user granted consent for this candidate. Lenses readability and reliability returned approved; the exact acknowledgement burned the authority. Its 12 findings are non-blocking and are tracked as T-UI6: two real design defects (Monokai and Dracula surfaces darker than the canvas; `text_muted` at about 3:1 contrast), plus naming, documentation and test-coverage gaps. Checked: no `set_visuals` or other `set_style` path exists in `karkinos/src`, so the global `Visuals` cannot be replaced at runtime.
- **L5 (T-UI2, v0.2.20)**: Route: direct inline (one understood change across `ui.rs` and `title_bar.rs`; every edit touches the same header code, so no parallel writers). Done: `custom_title_bar` is 36px (logo, name, search pill 260×26 calling `quick_open.request_open`, Run, Debug, Check, Test and Clippy, a right group with theme picker, Extensions, Console toggle and window controls; drag and double-click preserved). `tab_bar` is 30px (Open… and Save on the left, tabs with a 2px accent underline and an orange dirty dot, round `+`, breadcrumb on the right). The `toolbar` panel is removed. `TITLE_BAR_HEIGHT + TAB_STRIP_HEIGHT = 66`. `breadcrumb_segments` is pinned by two tests. Test-first: RED observed for the three `title_bar` tests (E0425, constants and helper missing), then GREEN. Layout and drawing code has no runnable deterministic RED; that is an exception, covered by compile checks and an independent verification. Deviations, all deliberate: Open… and Save move to the tab strip so no function is lost; Test, Clippy and Console stay; the decorative `🦀 Rust:` label is removed; Check follows Debug as in the spec; the search pill is placed by flow rather than absolutely centered, because a centered 260px pill overlaps the action cluster at 1280px. Bold: egui 0.27 has no weight support and the project ships no Bold face, so `faux_bold_label` draws the galley twice with a 0.6px offset. This is an approximation; a real bold face is an asset decision left open. Independent verification (`gentle-ai-verify`): VERDICT BLOCKED on the bold gap only. Fixed with the faux bold; spot-checked by grep and compile, not re-verified by a second verifier. Evidence: `cargo check --workspace` 0 warnings; `cargo test --workspace` karkinos 102 passed, karkinos-core 52 passed. Differential check: N/A (no command or option behavior changed). Risk: item 6 (interactive layout with no automated UI test; the visual result needs a manual look).
