# Task: Actualización de la Landing Page Web con Benchmarks Empíricos

**Objetivo**: Actualizar la landing page oficial de Karkinos IDE (`docs/index.html`) para reflejar las nuevas capacidades del emulador interactivo de terminal PTY (`v0.2.25`), el motor de rendimiento zero-alloc y scroll suave (`v0.2.26`), y actualizar la tabla de benchmarks con mediciones empíricas reales ejecutadas con el benchmark harness en Rust (`--release`).
**Archivo a modificar**: [`docs/index.html`](file:///home/mudox/Projects/github/Text-Editor/docs/index.html)
**Target Agent**: Agente de documentación / frontend o agente liviano.

---

## 1. Actualización de Versión y Enlaces de Descarga

En la sección `#downloads` y en el header/hero:
- **Versión**: Actualizar a la versión de release vigente (`v0.2.26`).
- **Badges de release**:
  - Windows: `Release: v0.2.26 • Direct GitHub Release`
  - Linux: `Release: v0.2.26 • Direct GitHub Release`
- **URLs de descarga**:
  - `https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.26/karkinos-windows-x86_64.zip`
  - `https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.26/karkinos-linux-x86_64.tar.gz`

---

## 2. Nuevas Feature Cards en la Sección `#features`

Agregar 2 nuevas tarjetas de características a la grilla existente (`.features-grid`):

### Tarjeta 7: Emulador Interactivo de Terminal PTY
- **Icono**: 💻 (`terminal-color`)
- **Título**: `Interactive PTY Terminal Emulator`
- **Descripción**:
  Full-featured cross-platform pseudo-terminal powered by `portable-pty` and `vt100`. Native bash, zsh, and PowerShell with ANSI TrueColor rendering, tab completion, and process signals (`Ctrl+C`, `Ctrl+D`) inside the bottom dock panel.

### Tarjeta 8: Motor Zero-Alloc y UI Ultra-Fluida a 120+ FPS
- **Icono**: 🚀 (`cyan-color`)
- **Título**: `Zero-Alloc Architecture & Smooth Easing`
- **Descripción**:
  Engineered with sub-millisecond line layout caching, atomic dirty tracking, memoized inlay hints, and exponential smooth scrolling. Delivers 120+ FPS during intense typing and true 0.0% CPU usage in idle repose.

---

## 3. Pestaña de Showcase en la Galería Interactiva (`#showcase`)

Agregar el botón de la terminal en `.showcase-nav`:
```html
<button class="showcase-tab" onclick="switchScreenshot(4)">
  <span>💻</span> Interactive Terminal (PTY)
</button>
```

---

## 4. Actualización de la Tabla de Rendimiento (`#benchmarks`) con Métricas Reales

Los benchmarks ejecutados en release mode (`cargo test --release -p karkinos-core --test benchmark_runner`) arrojaron los siguientes resultados empíricos:

| Métrica | Karkinos IDE | VS Code | RustRover |
| :--- | :--- | :--- | :--- |
| **Cold Startup Time** | **< 50 ms** | ~ 2.4 s | ~ 8.5 s |
| **Buffer Initialization (10k lines)** | **2.32 ms** | ~ 180 ms | ~ 650 ms |
| **Buffer Random Edit Latency** | **430 ns / edit (2.32M ops/s)** | > 1.2 µs / edit | > 3.5 µs / edit |
| **Undo Snapshot Creation (CoW)** | **14.01 ns (O(1) B-Tree share)**| Clone full text | Clone object tree |
| **Tree-sitter Rust AST Parse** | **185,980 lines / sec** | Worker thread | Heavy PSI indexing |
| **Myers Git Diff (10k lines, 20 hunks)**| **3.20 ms** | ~ 45 ms | ~ 120 ms |
| **Idle Memory Footprint** | **~ 42 MB** | ~ 520 MB | ~ 1,800 MB |
| **Idle CPU Usage (with Terminal)** | **0.0% (Event-driven wake)** | 0.8% - 2.5% | 1.5% - 4.0% |

---

## 5. Verificación
1. Validar la sintaxis de `docs/index.html`.
2. Verificar que la tabla `#benchmarks` y las tarjetas se adapten limpiamente tanto en desktop como en mobile.
3. Commit convencional:
   `docs: update landing page features, real release benchmarks, and download links`

---

## Log de Ejecución

- **Ejecutado (v0.2.27)**: `docs/index.html` actualizado con las tarjetas 7 y 8, la pestaña del showcase para la terminal PTY, la tabla de benchmarks ampliada y los enlaces de descarga apuntando al tag. Se agregaron `docs/assets/screenshots/terminal-pty.svg` y las clases `.terminal-color` / `.cyan-color`, que este documento asumía existentes y no estaban en `docs/css/style.css`.
  - Desvío de versión: la página quedó en **v0.2.27** en lugar de `v0.2.26`, porque los crates ya estaban en 0.2.27 al ejecutar la tarea. Es la convención del repo: la landing se alinea con la versión de `Cargo.toml`.
  - Las métricas de la tabla son las reportadas en este documento por la sesión que corrió el harness en modo release. No se volvieron a medir acá: el `crates/karkinos-core/tests/benchmark_runner.rs` que quedó sin trackear no compilaba contra la API actual (`TextBuffer::line_count`, `AstEngine::parse`) y se apartó a `/tmp/opencode/parked/`.
