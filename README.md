# Rope Editor

Este repositorio contiene un editor de texto basado en la estructura de datos *Rope*, desarrollado en Rust. El proyecto está diseñado nativamente para entornos Linux y está dividido en dos aplicaciones independientes (cada una con su propio `Cargo.toml`).

## 📦 Estructura del Repositorio

El repositorio se divide en dos directorios principales:

### 1. `rope_editor_app/` (Versión de Terminal)
Es la versión estándar para consola. Su estructura es la siguiente:
```text
rope_editor_app/
├── Cargo.toml
├── src/
│   ├── backend.rs
│   ├── buffer.rs
│   ├── clipboard.rs
│   ├── config.rs
│   ├── editor.rs
│   ├── input.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── renderer.rs
│   ├── rope.rs
│   ├── style.rs
│   ├── undo.rs
│   └── viewport.rs
└── tests/
    └── rope_tests.rs
```

### 2. `rope_editor_gui/` (Versión Gráfica)
Es la versión con interfaz gráfica de usuario (GUI). Reutiliza la lógica central (*Rope* y *Undo/Redo*).
```text
rope_editor_gui/
├── Cargo.toml
└── src/
    ├── main.rs   # Lógica principal de la app (ventana, edición, abrir/guardar)
    ├── rope.rs   # Estructura Rope persistente
    └── undo.rs   # Sistema de UndoStack/Edit
```

## ⚙️ Requisitos Previos

- [Rust y Cargo](https://rustup.rs/) instalados.
- Un entorno **Linux**.
  - **Nota sobre WSL (Windows Subsystem for Linux):** El proyecto es perfectamente compatible con WSL. La versión de terminal (`rope_editor_app`) funcionará nativamente sin ajustes. Sin embargo, para ejecutar la versión gráfica (`rope_editor_gui`) en WSL, necesitarás usar **WSLg** (presente en las versiones modernas de Windows 11) o configurar un servidor X11/Wayland en Windows.

## 🚀 Compilación y Uso

Como se trata de dos proyectos de Cargo separados, debes navegar al directorio correspondiente antes de compilar o ejecutar.

### Ejecutar la versión de Terminal

Abre una terminal, ingresa a la carpeta y ejecuta:

```bash
cd rope_editor_app
cargo run
```

### Ejecutar la versión Gráfica (GUI)

Abre una terminal, ingresa a la carpeta de la GUI y ejecuta:

```bash
cd rope_editor_app_GUI
cargo run
```

### Compilación para Producción (Release)

Si deseas obtener los binarios optimizados, usa el flag `--release` dentro del directorio que desees compilar:

```bash
# Ejemplo para la versión de terminal
cd rope_editor_app
cargo build --release
```
Los ejecutables generados se encontrarán en la carpeta `target/release/` dentro de su respectivo directorio.

## ⌨️ Atajos de Teclado (Versión de Terminal)

La versión de terminal (`rope_editor_app`) utiliza un sistema modal simplificado, inspirado en Vim, para una edición rápida y eficiente.

### Modo Normal
El editor inicia en este modo por defecto. Utilízalo para navegar y manipular texto.
- `i` — Cambia al modo **Insert**.
- `x` — Borra el carácter bajo el cursor.
- `y` — Copia (yank) la línea actual.
- `p` — Pega (paste) el contenido previamente copiado.
- `u` — Deshace (undo) la última acción.
- **Flechas direccionales** — Mueven el cursor por el documento.

### Modo Insert
Modo utilizado para ingresar texto al documento.
- `Esc` — Vuelve al modo **Normal**.
- **Teclas alfanuméricas** — Escribes el texto normalmente.
- `Backspace` — Borra el carácter a la izquierda del cursor.

### Atajos Globales (En cualquier modo)
Estas combinaciones de teclas funcionan sin importar en qué modo te encuentres.
- `Ctrl + S` — Guarda los cambios en el archivo.
- `Ctrl + Q` — Sale del editor.
- `Ctrl + R` — Rehace (redo) una acción previamente deshecha.

## 🏗️ Arquitectura 

###Versión de Terminal

A continuación se muestra el diagrama UML con el diseño interno y las interacciones de los módulos en la versión de consola:

![Diagrama de Arquitectura del Editor](https://www.plantuml.com/plantuml/dpng/hLZDRYEv4RxhAGeJODFQzTXP1CX1yHYhaJixnlZ7aUGP91U1QfOapb1a3ycMHvC4o4Fa1NBSGmv1RhdwJV8a8Qj9qMfhWWKS7onsLR6wM3zV5Vi7wvXnpLhsx4UXQcRO6Yh9h1qwPyIySNXHQJKHNn1-zKX42eLkMoDeurQwr_ibl7h6dzyEvMGwlXjDxyR32PoNS3wStaoWk7xuToLHmrZNs8V__FLlm8N5XtzggD5OOHqgXr2Cg_vpJUYnkNM6LQuzFbnhZl2d7lY_Ny0zYkNA3Q2ntX54-oxIYlu-LTYPHBO8vBdMCb8bgYpubrvFQOTWmdxG2tfL48mguHL8hPR2DHngesl101Kq2Ywzujy6mSpdAC4syuU_cxcM8BvyQWGQhkE62-NGA2QFQTDKSstzLWlhXvySFLYjWAF_aHeaLgunRD2-W5b0BYgX5NenX1MhKC7cuH-COW751WsmEVkW-ySzL1p2aNezyXqP6dtt4Xn-zfwRE2FKqfyzYYJJTc8I5rviN9q6Rvn58haeK_DsiY5jDLftjtu72k-BzjNVM6Vw0zgIWgGi6bVKsiQO7C7NH3bAT5XGbUo4KV6utXUkLipCc2k4ulWvIzpKJcXr6hZ9kaghoWjgjqBZN-jrT5JPMWetOsHarbG4YICWyIXeSCuaKnKMMK4tvN9kuhfssqvoEZsp59r0Zt4Y3bLvzpmTdLLZhAP3wyN2eijI8-8yoVPiussku44CoKdhxfFMFIV92IJESTEWtBIPJV6_lYlVtS0MAhskcV6A20XYWTXS5sX1QLXhBXQYObpROoXbjaDxqqH8nKyDSWP2ELOv3KKbsGOXu9r_8p-Gv_68kppC7iZ-MUiDpa9k5ogxPgC5Vymdttn5eDPMEA7tyUFvS3kwkRgzkIwlfsM0sqiepo-cD-FdAev-ARbmZ_moRnQBaEQlwJSIDmBlQnsAwbrSHOP1nT44-N5SHOPGTUDcAwQu3DoByFY6dg94234Eujkl_3gIAodgkMQ6-wHCoyZoWGtPpwgF0xZpwqbOfcrQBSJIxw7V60vTevejXCIYPcxbI_VM_uJAGzj8Tne2TmIbCNfNwfPjiDtnFyNQSyteaCL7t0xWBMxB3IgtbmzaSR5FCfX96I9RRpy2oHeJG10Xc3IZGoI02aVnknBuSc7SDY85KM898P-nWEXRwt0z5UkVYQfatiz6LHBEA5GG5AbcdIMU8bMb6tMe8IUp4-ubJF8Fpkbr0AMSclj811oJ8wLaQwP0j8rhZVwNvDk6E1Zixzl2k2olRY-7VnnUWqMvE3R9zRKsW99sWMfmeoEq1SnPDuvLuk5V2XWeiK7oV3RYA8yzbM19nlD9sm64IhMLkx6kPy5PoKcv-NhtoNtglU1kjKzQ7HY4R6Mqb0L7wTW0nDC06WoquduNwTN1E6_yrhb4CizIiJ2rn5EIpAr_XmePNdp9KkwyrfzF-LR1wvOGAoi2If5Gw9laoen0toGtTRBKiFi03gwnipap1HAmUF6I5eygZf9YmIfCxuRJqntjx2oOcZM6eYX2E_t-DqUmZR-vKex0kgr4d-3XPz-YIY8pdUfVoCQkEZJojoSLfDl_wsV_JT6iGxjBbYOtMiTr4-gQVhCycU7FOvrfAlVeC0l2HgbDV9vtdxNakU3G5s_BZWh6vQISqkhrvN3qDjBEwVT7NppNjFf3UNbvypwAtPMqUF_cObe-Ej0ArtaOLDgicOpUQsbnIDcdvRWcUZ8ozu-ezFfcV3MyfEN5zQGSjrQ_kvZS4PMQwTNm-ZoRzBWtxVnbaF6PH_QYdx4zijQi9Wu6sXxoZ_HwxVLarObtA2ALvaYxHtuHQH7yZ1t0LLgceoRHg63qPZWkmfJQfySm78pRrMJKHi37QN8x7BLX68x7D--BSs6m2kqZfrX-qK4ZdP5xyoUJRk_PhiJ2TH9dzvQevkwMVivlthTfSbd-hetA-EB7DzEqBNalRheWX1xv0nRL0F91pmco0vqUh_G6syckuC8CyhkFO3UYaU24RP0Mlsy4HP96YDOCZ8l9lN3LAYHJqPrEiiFow96hiPr9tc5r6dR4daojVjRbfKanu01w0htayDWGM_tUFeDFMTcqy7WeZ1-5uksBtWhL7LRt6yGS5zhWtks4BHoQIEdU44Xf2mCDCvm1fm2qBNVNEPb_i2SSRTqy_6H9l9Wpmvo6fM6LO1QOXDRineJbYeM1WPfbrJ-Y7U5AYry2gGhtxD2N2OartIaajWjDibfoWkuJXIs6vswRWOES4NsKiI9CsEMqZ11tw-4kmrSUIUCs6ZmTAjSPSRo5s-9h0rNDh2F43woiknrTS_Ddyrc8IwVvnz5gZiRkZNik0Ek0yFN_TEiOb_xi5_xUCNdsJOE-pvp--ULB-jgG7tTV5eAzuofT3WNNyEeCsDBWaYQiKCsQkDeA8lJIJj1UpOllNm0f7T1qz5KcPMsFKhBBqJeoZe_Fqbrn09L1rgEgIl8aCuXbSf15vH8ET6jqBP0p3lYvaZwA6mQ5qY6Zm_7sZXFdrc-pgbQvphHMf4jDqr2NdYQlBZrtkoUyZ6mTHaxx3YFcRAA-V7cM8JEu0grZN0EhN1DYd6nA7ai3M2_--fmuFkxCF9df0nFdcr0IhDU-EvaICv_-jxHVluYpGk_ncOEQt9S7K1lDcmgVogHsb4NeQ5qcVi2gSQoNL2PxKhj-oiYTFOe4ZRbnzNgvmT2MRr-GK-droSK4NL_zM5Fka3IWPVpUE1UA6Q5987w5ObmVmuXnbh_mBGd5IGWbr6Wr58S_O0QP_16pxnKH2dyGd0N9V3sZ13z1AK9KwS6tBwzq2QgVKRbxoMkTJLfkcJD-5t1C7vG7H8_nqEPJ4nBjA5t-V6JmGpf3oa6UkiLHlfHOZ8sbOcP9lf8CQeCMLRhPRKbYBpcSOI4TWEO_RKc0XMhsV6sXk7et0YkyB-8Nh4U7_C6lchNyBm00)

###version grafica

Diagrama UML con el diseño interno y las interacciones de los módulos en la versión GUI:

![Diagrama de clases — GUI](https://www.plantuml.com/plantuml/dpng/hLRDRjj64BxpAGOkq5A98gKz4geHDrLReu4Js4alHI6CoA6qo7A7tLqgTee2VOY-HawzzUetwPDqTqYjVkoSR9sMCz_Ep-mtCthXF5hVDJfpxvLfqM83fKRdphotQjbvEY_PNAcF1C_t85ePyZSj0Lk_vYpx2hvzm2zSXxEsXH_VdKDETN10euUQp2GBc5VAiut6PxFUSb64hzDJ-2C307W6dgvz0LVUAhCQHE6cNpY3hLlpiQup5I-Sn_9z0U_2-IeU1rsjD2rQzEk5CcrtVBFih2NZ5n5LmElMApQpDm7zNLUV3fXAMNzJm99P3nBdqNVkmDHJwDeAFUMblnv3Nwu2DgoguSusz8Mg5-KQpOggF25I5aSYItT4t98v4ZdSK1BzcMM6FO5LgxK7hdVL5dIiB903qh3iwfeid6pKXayAq0mKe2WmMdMgADu6SBmUIq2bnuQCfp5uAdKFQzdsNnEuv9RWuZMqQBqgLOj01WOOLK5o-u-9ekWD_llhRt2i6Hn1vp1Uwf6U6mmXrUpPdK1-Jz56Wlkz8q2jOa1eOZ9hvJnRXHegaafECZ8Ln58yHYTStFvRQk8-o_m7BB4YU0elkMdPAKyZIKcjEjGIc-GE6x9EyWvWJxPH1lKZzP04CflraUnrZkNO7PVbx8A3VA2ceGzvtr9VEcz7XTmUTBgdK-VKnwt8yy89iwFYWECbcn9zpdvDDXYBPWwjkLOhlmYGbbrlD0BoY1Y3m0UWCexiFcmCdukm8asUSXcA2Koc6iw7uC0I6_qhKz7r3Z5qTjHkYs9fYHfDIVbHGPIXOmj71IexwrZSS5qxyZlKIz749YJJDH0U8RvIeWbCOHxawRrU4TRv_h3hotDkF5c3Elz0iOjJqffgUTonTBSSHz6NK6BmdPXm4DeWQ_3wC9E6DxI8PdCpAD9esUd5sMS0afVY7OVk3g1Ksxa1SXu1stOiBS4LzfApD-So3mJMpmGZh1t1pJuT8T-G5L2QQaLntv2O2AYdwtHBocbFkcbFg2dvSh8T9XKQSCe4npASPB1HE9CMn1U_ejRy8Qy8gqN95Or-wqVGoj8A6tQFEsr-EXvcqxW87cbof0MPc39ipb-edCModkujdyKzEddspgrp2e82elYG4xB8KjCTtBDqLvKOBAP2kpXlkzAd9daAxvMfUaS_AtFCl4D2BwbcImSDZRKdEqY6naos1XVd5rVpoxTo_7x-QlvsBiVB-PjNPo_dzw_KEuoMAC4fQbfBGRnl0cvIk0dnF1AjfobM25iMWwMWr_WHWRJCH1OGnPd0Op0SyAtlB49dCArMmRITyN43bTpmtHKK8MaDGHxvwsu_WSFbxISCM0bnPuokiVG85VNnIU-8ThfaQx7YqUEI_f8qbeeDkGSJFGq7UFBi6PnyVR97-YAcIRI7cKnEXHn1LLhQlOJBidIjD_JafATeiVkx8NQIczxCGC9xLKB6B9Cl7bIo9GfPtP37aIu2qJwVJBv9W3YdOIeqx85YHkBlP_-GGVQ2JDKr-dy0)

## 📄 Licencia

Este proyecto está bajo la Licencia Pública General de GNU (GNU GPL).
