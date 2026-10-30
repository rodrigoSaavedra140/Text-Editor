# Karkinos Text Editor

Editor de texto ultra liviano y de alto rendimiento escrito en Rust con `eframe`/`egui`.
Diseñado para iniciar en menos de 50 ms con un consumo de recursos mínimo, soporte nativo de multi-pestañas, búsqueda difusa de archivos, resaltado de sintaxis ligero y ergonomía de edición moderna. Corre en Linux nativo (Wayland y X11) y bajo WSLg.

## 📦 Estructura del proyecto

```text
karkinos/
├── Cargo.toml           # Dependencias + [profile.release] (LTO thin, strip)
├── assets/
│   └── logo.png         # Logo embebido con recorte de transparencia adaptativo
└── src/
    ├── main.rs          # Entrada: inicialización de ventana custom y decoraciones
    ├── app.rs           # EditorApp: buffers, pestañas, guardado async, zoom, confirmaciones
    ├── buffer.rs        # Motor de buffer de texto Rope (ropey) O(log N) y snapshots COW O(1)
    ├── editor_widget.rs # Widget de edición por franja (wrap), teclado, gutter, autoscroll
    ├── ui.rs            # Layout egui: title bar personalizada, tabs, toolbar, diálogos modales
    ├── title_bar.rs     # Barra de título sin bordes nativos con íconos vectoriales
    ├── syntax.rs        # Tokenizador de sintaxis y coloreado LayoutJob (10 lenguajes)
    ├── quick_open.rs    # Diálogo de búsqueda rápida de archivos (Ctrl+P) con fuzzy match
    ├── theme.rs         # Paletas de temas (Dark, Light, Monokai, Dracula)
    ├── util.rs          # Formateo de tamaños, cálculo de saltos y detección WSL
    └── undo.rs          # UndoStack con agrupación por inactividad y punteros Arc compartidos
```

## ⚙️ Requisitos

- [Rust y Cargo](https://rustup.rs/) (edición 2021).
- **Linux nativo** (Wayland o X11) o **Windows con WSL / WSLg**.

## 🚀 Compilación y ejecución

```bash
cd karkinos

# Modo desarrollo
cargo run

# Abrir un archivo directamente
cargo run -- mi_archivo.rs

# Compilar binario de producción optimizado
cargo build --release
```

El ejecutable resultante se ubicará en `target/release/karkinos`.

## ⌨️ Atajos de teclado principales

### Archivos y pestañas
- `Ctrl+N` — Nuevo documento (con confirmación de cambios)
- `Ctrl+T` — Nueva pestaña
- `Ctrl+W` — Cerrar pestaña activa
- `Ctrl+Tab` / `Ctrl+Shift+Tab` — Ciclar entre pestañas
- `Ctrl+P` — Búsqueda rápida de archivos en el proyecto (*Quick Open*)
- `Ctrl+S` — Guardar archivo
- `Ctrl+Shift+S` — Guardar como...
- `Ctrl+Q` — Salir (con resguardo multi-pestaña ante cambios sin guardar)

### Edición y navegación
- `Ctrl+Z` / `Ctrl+Y` — Deshacer / Rehacer
- `Ctrl+F` — Barra de búsqueda y reemplazo
- `Ctrl+G` — Ir a número de línea
- `Ctrl+/` — Comentar / descomentar línea o bloque seleccionado
- `Ctrl+D` — Duplicar línea o bloque hacia abajo
- `Tab` / `Shift+Tab` — Indentar / desindentar bloque o línea
- `Ctrl+=` / `Ctrl+-` o `Ctrl+Rueda` — Zoom de tamaño de fuente
- **Clic en el gutter (números de línea)** — Selecciona la línea completa

## 📄 Licencia

Este proyecto se distribuye bajo los términos de la [Business Source License 1.1 (BSL 1.1)](LICENSE).
Permite el uso, copia, modificación y compilación gratuita para fines personales, educativos, de desarrollo y evaluación no comercial. Para cualquier uso o distribución comercial se requiere una licencia comercial emitida por el titular de los derechos.
