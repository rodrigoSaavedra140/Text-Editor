# Rope Editor (GUI)

Editor de texto con interfaz gráfica (Rust + `eframe`/`egui`), modelo de
buffer por líneas (`Vec<String>`) con wrap, portapapeles del sistema y
deshacer/rehacer basado en snapshots del documento. Corre en Linux
nativo (Wayland y X11) y bajo WSLg.

## 📦 Estructura

```text
rope_editor_app_GUI/
├── Cargo.toml           # dependencias + [profile.release] (LTO, strip)
├── assets/
│   └── logo.png         # Logo embebido en el binario
└── src/
    ├── main.rs          # Punto de entrada: detecta WSL, configura la ventana
    ├── app.rs           # EditorApp: abrir, guardar async con progreso, undo/redo
    ├── editor_widget.rs # El editor: dibujado por franja (wrap), teclado, portapapeles
    ├── ui.rs            # Layout egui: fondo, toolbar, navegador, barras de estado
    ├── title_bar.rs     # Barra de título propia + bordes para redimensionar
    ├── util.rs          # Helpers: tamaño legible, cierre prolijo, detección de WSL
    └── undo.rs          # UndoStack: cada Edit guarda el documento antes/después
```

## ⚙️ Requisitos

- [Rust y Cargo](https://rustup.rs/).
- **Linux nativo** (Wayland o X11) o **WSL con WSLg** (Windows 11+): en
  WSL se fuerza el render por software automáticamente
  (`LIBGL_ALWAYS_SOFTWARE=1`), porque el GL de WSLg a veces falla en
  silencio.

## 🚀 Compilación y uso

```bash
cd rope_editor_app_GUI
cargo run                 # modo debug
cargo run -- archivo.txt  # abrir un archivo directamente
cargo build --release     # binario optimizado en target/release/
```

## ⌨️ Atajos

### Globales

- `Ctrl+S` — Guardar · `Ctrl+Z` — Deshacer · `Ctrl+Y` — Rehacer · `Ctrl+Q` — Cerrar

### En el editor

- `Ctrl+C / Ctrl+V / Ctrl+X` — Copiar / Pegar / Cortar (portapapeles del sistema)
- `Ctrl+A` — Seleccionar todo
- Flechas, `Home`, `End` — Navegar; `Shift` + movimiento para seleccionar
- Click y arrastre con el mouse — selección por tramo

## 🏗️ Arquitectura

Diagrama de clases del diseño GUI (render generado con PlantUML):

![Diagrama de clases — GUI](https://www.plantuml.com/plantuml/dpng/hLRDRjj64BxpAGOkq5A98gKz4geHDrLReu4Js4alHI6CoA6qo7A7tLqgTee2VOY-HawzzUetwPDqTqYjVkoSR9sMCz_Ep-mtCthXF5hVDJfpxvLfqM83fKRdphotQjbvEY_PNAcF1C_t85ePyZSj0Lk_vYpx2hvzm2zSXxEsXH_VdKDETN10euUQp2GBc5VAiut6PxFUSb64hzDJ-2C307W6dgvz0LVUAhCQHE6cNpY3hLlpiQup5I-Sn_9z0U_2-IeU1rsjD2rQzEk5CcrtVBFih2NZ5n5LmElMApQpDm7zNLUV3fXAMNzJm99P3nBdqNVkmDHJwDeAFUMblnv3Nwu2DgoguSusz8Mg5-KQpOggF25I5aSYItT4t98v4ZdSK1BzcMM6FO5LgxK7hdVL5dIiB903qh3iwfeid6pKXayAq0mKe2WmMdMgADu6SBmUIq2bnuQCfp5uAdKFQzdsNnEuv9RWuZMqQBqgLOj01WOOLK5o-u-9ekWD_llhRt2i6Hn1vp1Uwf6U6mmXrUpPdK1-Jz56Wlkz8q2jOa1eOZ9hvJnRXHegaafECZ8Ln58yHYTStFvRQk8-o_m7BB4YU0elkMdPAKyZIKcjEjGIc-GE6x9EyWvWJxPH1lKZzP04CflraUnrZkNO7PVbx8A3VA2ceGzvtr9VEcz7XTmUTBgdK-VKnwt8yy89iwFYWECbcn9zpdvDDXYBPWwjkLOhlmYGbbrlD0BoY1Y3m0UWCexiFcmCdukm8asUSXcA2Koc6iw7uC0I6_qhKz7r3Z5qTjHkYs9fYHfDIVbHGPIXOmj71IexwrZSS5qxyZlKIz749YJJDH0U8RvIeWbCOHxawRrU4TRv_h3hotDkF5c3Elz0iOjJqffgUTonTBSSHz6NK6BmdPXm4DeWQ_3wC9E6DxI8PdCpAD9esUd5sMS0afVY7OVk3g1Ksxa1SXu1stOiBS4LzfApD-So3mJMpmGZh1t1pJuT8T-G5L2QQaLntv2O2AYdwtHBocbFkcbFg2dvSh8T9XKQSCe4npASPB1HE9CMn1U_ejRy8Qy8gqN95Or-wqVGoj8A6tQFEsr-EXvcqxW87cbof0MPc39ipb-edCModkujdyKzEddspgrp2e82elYG4xB8KjCTtBDqLvKOBAP2kpXlkzAd9daAxvMfUaS_AtFCl4D2BwbcImSDZRKdEqY6naos1XVd5rVpoxTo_7x-QlvsBiVB-PjNPo_dzw_KEuoMAC4fQbfBGRnl0cvIk0dnF1AjfobM25iMWwMWr_WHWRJCH1OGnPd0Op0SyAtlB49dCArMmRITyN43bTpmtHKK8MaDGHxvwsu_WSFbxISCM0bnPuokiVG85VNnIU-8ThfaQx7YqUEI_f8qbeeDkGSJFGq7UFBi6PnyVR97-YAcIRI7cKnEXHn1LLhQlOJBidIjD_JafATeiVkx8NQIczxCGC9xLKB6B9Cl7bIo9GfPtP37aIu2qJwVJBv9W3YdOIeqx85YHkBlP_-GGVQ2JDKr-dy0)

## 📄 Licencia

Este proyecto está bajo la Licencia Pública General de GNU (GNU GPL).
