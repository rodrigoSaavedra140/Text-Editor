# Feature: Ventana de guardado sin nombre y sintaxis extendida

**Objetivo**:
1. Ventana modal al guardar archivo sin nombre pidiendo un nombre personalizado; si se deja en blanco o se cancela/confirma vacío, se asigna `nuevo.txt` (o correlativo sin colisiones).
2. Resaltado de sintaxis extendido para los lenguajes más usados: C, C++, Java, JavaScript/TypeScript, SQL, HTML y CSS (junto a Rust, Markdown, JSON existentes).

**Branch**: `main` (commits convencionales directos sin atribuciones AI).
**Runner de tests**: `cargo test` dentro de `rope_editor_app_GUI`.

## Specs

- **S1** — Ventana modal de guardado para archivos sin nombre:
  - Aceptación: al accionar Guardar (Ctrl+S o botón de toolbar) sobre un buffer sin ruta ni nombre asignado, se abre un diálogo modal solicitando el nombre. Si el usuario ingresa un nombre, se guarda con ese nombre; si lo deja en blanco o confirma vacío, se guarda automáticamente como `nuevo.txt` (o `nuevo (N).txt` si ya existe).
- **S2** — Resaltado de sintaxis extendido:
  - Aceptación: soporte para C (`.c`, `.h`), C++ (`.cpp`, `.cc`, `.hpp`, etc.), Java (`.java`), JavaScript/TypeScript (`.js`, `.jsx`, `.ts`, `.tsx`), SQL (`.sql`), HTML (`.html`, `.htm`) y CSS (`.css`, `.scss`) con tokenización eficiente $O(L)$ y renderizado con `LayoutJob` usando los colores del tema activo.

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | pendiente | - |
| T2 | S2 | inline | pendiente | - |

## Log

- **L1** (2026-10-30): Planificación de la ventana de guardado para archivos sin nombre y extensión de sintaxis a C, C++, Java, JS/TS, SQL, HTML, CSS.
