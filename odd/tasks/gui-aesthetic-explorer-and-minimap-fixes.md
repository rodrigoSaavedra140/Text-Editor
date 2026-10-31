# Roadmap: Mejoras de Estética, Árbol de Archivos Desplegable y Corrección de Minimap (v0.2.38)

**Objetivo**: Transformar la ergonomía visual de Karkinos IDE para alcanzar un acabado industrial pulido (nivel Zed / VS Code):
1. Convertir el explorador de archivos en un **árbol jerárquico interactivo con carpetas colapsables/desplegables**, iconos por tipo de archivo y acciones de colapsar todo.
2. **Eliminar la barra gris inútil a la derecha** del editor (generada por la reserva fija incondicional del minimap de 64px) y hacer el **Minimap toggleable (`Ctrl+Shift+M`)**, con borde divisorio sutil y ancho 0px cuando está inactivo.
3. **Elevar el diseño estético general**: indicador activo en la barra de actividad lateral (Activity Bar), tabs con acento visual, pills modernas en el status bar y jerarquía de contraste en 3 niveles.

**Branch**: `main` (conventional commits por unidad).
**Versión destino**: `0.2.38` (en `Cargo.toml`, `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml`).
**Calidad**: Cero compiler warnings (`cargo check --workspace`), 100% de tests pasando (`cargo test --workspace`).

---

## Diagnóstico Técnico Arquitectónico

### Problema 1: Explorador de Archivos Plano y Saturado
- **Causa raíz actual (`karkinos/src/ui.rs:673-690`)**:
  El panel `explorer_panel` itera directamente sobre `self.explorer.files: Vec<PathBuf>` e imprime una lista plana con la ruta relativa completa (`crates/karkinos-core/src/buffer.rs`).
- **Consecuencia**:
  No hay carpetas agrupadas ni jerarquía visual; todos los archivos aparecen abiertos y expandidos a la vez, haciendo imposible navegar proyectos grandes.
- **Solución**:
  Estructurar los archivos en un árbol jerárquico (`FileTreeNode`), con estado colapsable por carpeta (`collapsed_dirs: HashSet<PathBuf>`), chevrons interactivos (`▸`/`▾`), indentación proporcional al nivel de profundidad y solo nombres de archivo/carpeta legibles.

### Problema 2: "Barra gris que no hace nada" a la derecha del Editor
- **Causa raíz actual (`karkinos/src/editor_widget.rs:348, 929-933, 2145-2172`)**:
  Se definió `const MINIMAP_W: f32 = 64.0;` fijo e incondicional. En cada frame, el editor resta 64px a la derecha del canvas (`rect.right() - MINIMAP_W`), dibujando un rectángulo sólido del color de fondo (`theme.bg`) y un viewport tenue.
- **Consecuencia**:
  En archivos vacíos, medianos o cortos que no requieren scroll, aparece una franja gris vacía y muerta que roba espacio útil de pantalla y desorienta al usuario como si fuera un "gap roto".
- **Solución**:
  Hacer el minimap **toggleable** mediante `show_minimap: bool` en `EditorState`:
  - Si `show_minimap == false` (valor por defecto o al apagarlo): `minimap_w = 0.0`. El editor y la barra de scroll usan el 100% del ancho derecho, **eliminando por completo la barra gris**.
  - Si `show_minimap == true`: `minimap_w = 64.0`, dibuja una línea divisoria vertical sutil de 1px (`theme.border_subtle`) a la izquierda del minimap, un tooltip descriptivo y el viewport con esquinas redondeadas.
  - Atajo global: `Ctrl+Shift+M` para alternar el minimap al instante.

---

## Tareas Específicas de Implementación

```
odd/tasks/gui-aesthetic-explorer-and-minimap-fixes.md
├── Tarea UI-6: Árbol de Archivos Jerárquico Desplegable en el Explorer
├── Tarea UI-7: Eliminación del Gap Gris y Minimap Toggleable (Ctrl+Shift+M)
└── Tarea UI-8: Refinamiento Estético de Barra de Actividad, Tabs y Status Bar
```

---

### Tarea UI-6: Árbol de Archivos Jerárquico Desplegable en el Explorer

#### 1. Modelado de Datos (`karkinos/src/app.rs`)
Extender `ExplorerState` para gestionar carpetas colapsadas:
```rust
#[derive(Debug, Default)]
pub struct ExplorerState {
    pub open: bool,
    pub files: Vec<std::path::PathBuf>,
    pub loaded_for: Option<std::path::PathBuf>,
    /// Conjunto de rutas de carpetas que están actualmente cerradas (plegadas).
    pub collapsed_dirs: std::collections::HashSet<std::path::PathBuf>,
}
```

Definir un nodo de árbol para renderizar de forma limpia y recursiva o iterativa:
```rust
#[derive(Debug, Clone)]
pub enum FileTreeNode {
    Directory {
        name: String,
        path: std::path::PathBuf,
        children: Vec<FileTreeNode>,
    },
    File {
        name: String,
        path: std::path::PathBuf,
        extension: String,
    },
}

impl FileTreeNode {
    /// Construye un árbol jerárquico a partir de la lista plana de archivos y la raíz base.
    pub fn build_from_paths(root: &std::path::Path, paths: &[std::path::PathBuf]) -> Vec<FileTreeNode> {
        // Agrupa por componentes relativos de ruta.
        // Las carpetas van primero, ordenadas alfabéticamente; luego los archivos.
    }
}
```

#### 2. Comportamiento y Estado Inicial
- Al abrir un proyecto nuevo o refrescar el explorador:
  - Las carpetas de primer nivel pueden estar expandidas, pero subcarpetas profundas cerradas, O bien permitir al usuario abrir/cerrar cualquier carpeta haciendo clic sobre ella.
- **Click en una carpeta**: alterna su presencia en `collapsed_dirs` (si estaba colapsada, se expande; si estaba expandida, se colapsa).
- **Click en un archivo**: abre el archivo en una pestaña del editor (`self.open_path(path)`).

#### 3. Renderizado y Guía Visual (`karkinos/src/ui.rs`)
En `show_explorer_panel`:
- **Encabezado del Panel**:
  - Título sutil en mayúsculas: `EXPLORER` en tamaño 10pt negrita con color `theme.text_muted`.
  - Nombre de la carpeta raíz del proyecto (ej. `Text-Editor`) con ícono de carpeta abierta `📂`.
  - Botones de acción en la esquina superior derecha:
    - Botón `⟲` / `⟳`: Refrescar lista.
    - Botón `⊟` / `─`: Colapsar todas las carpetas ("Collapse All").
    - Botón `✕`: Cerrar panel.
- **Fila de Carpeta**:
  - Indentación: `depth as f32 * 14.0 + 4.0`.
  - Chevron indicador: `▾` si está desplegada, `▸` si está colapsada (en `theme.text_muted`).
  - Ícono: `📁` si está colapsada, `📂` si está abierta.
  - Nombre: solo el nombre de la carpeta (ej. `crates`, `src`), en `theme.text`.
  - Efecto hover: fondo sutil redondeado (`theme.surface_bg` o `theme.active_line`).
- **Fila de Archivo**:
  - Indentación: `(depth as f32 + 1.0) * 14.0 + 4.0`.
  - Ícono contextual según extensión:
    - `.rs` -> `🦀` (o glifo naranja)
    - `.toml` -> `⚙`
    - `.md` -> `📄`
    - `.json` -> `📦`
    - `.html` / `.css` / `.js` -> `🌐`
    - Otros / desconocido -> `•` o `📄`
  - Nombre: solo el nombre del archivo (ej. `editor_widget.rs`).
  - Resaltado si está activo: si coincide con la pestaña abierta actualmente en el editor, mostrar el texto en `theme.accent` o `theme.text.strong()` con un fondo sutil.

---

### Tarea UI-7: Eliminación del Gap Gris y Minimap Toggleable (`Ctrl+Shift+M`)

#### 1. Estado en `EditorState` (`karkinos/src/editor_widget.rs`)
Agregar el campo de visibilidad del minimap:
```rust
#[derive(Debug, Clone)]
pub struct EditorState {
    ...
    /// Si está activo, renderiza la tira del minimap a la derecha.
    /// Por defecto `false` para que archivos estándar tengan 100% del ancho libre sin franjas muertas.
    pub show_minimap: bool,
}
```
En `EditorState::default()` inicializar `show_minimap: false`.

#### 2. Geometría Dinámica sin Gaps (`karkinos/src/editor_widget.rs`)
En el método de dibujo del editor:
```rust
let minimap_w = if state.show_minimap { MINIMAP_W } else { 0.0 };

let scrollbar_track = egui::Rect::from_min_max(
    egui::pos2(rect.right() - SCROLLBAR_W - minimap_w, rect.top()),
    egui::pos2(rect.right() - minimap_w, rect.bottom()),
);

let minimap_rect = egui::Rect::from_min_max(
    egui::pos2(rect.right() - minimap_w, rect.top()),
    egui::pos2(rect.right(), rect.bottom()),
);
```
- **Si `!state.show_minimap`**:
  - `minimap_w` es `0.0`.
  - La barra de scroll vertical se alinea perfectamente al borde derecho del panel (`rect.right() - SCROLLBAR_W`).
  - **No se dibuja nada del minimap**. ¡La barra gris desaparece por completo y el editor ocupa todo el ancho disponible!
- **Si `state.show_minimap`**:
  - Se dibuja una línea divisoria vertical a la izquierda del minimap:
    ```rust
    painter.line_segment(
        [
            egui::pos2(minimap_rect.left(), minimap_rect.top()),
            egui::pos2(minimap_rect.left(), minimap_rect.bottom()),
        ],
        egui::Stroke::new(1.0, theme.border_subtle),
    );
    ```
  - Viewport interactivo con esquinas redondeadas y tooltip "Minimap (arrastrar para scrollear)".

#### 3. Atajo Global y Botón en la Barra de Herramientas (`karkinos/src/ui.rs` & `app.rs`)
- Atajo `Ctrl+Shift+M`:
  ```rust
  if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::M)) {
      self.doc.editor_state.show_minimap = !self.doc.editor_state.show_minimap;
      if let Some(ref mut sec) = self.secondary_editor {
          sec.show_minimap = self.doc.editor_state.show_minimap;
      }
  }
  ```
- Botón en la barra superior del editor o en el menú View:
  - Icono `🗺` con hover `Toggle Minimap (Ctrl+Shift+M)`.

---

### Tarea UI-8: Refinamiento Estético de Barra de Actividad, Tabs y Status Bar

#### 1. Barra de Actividad Izquierda (Activity Bar, 44-48px)
- **Fondo**: Usar `theme.title_bar_bg` o un tono ligeramente más oscuro que el panel de contenido para crear jerarquía visual clara.
- **Indicador de Herramienta Activa**:
  - Cuando el Explorer o la Búsqueda están abiertos, dibujar una barra vertical de acento de 3px de ancho en el borde izquierdo del botón activo con el color `theme.accent` (`rgb(222, 100, 50)`), como en los editores de primer nivel.
  - Los botones inactivos muestran sus íconos en `theme.text_muted`; el activo en `theme.text`.

#### 2. Pestañas del Editor (Tabs Bar)
- **Pestaña Activa**:
  - Resaltar la pestaña activa con una línea inferior horizontal de 2px en `theme.accent`.
  - Si el documento tiene cambios sin guardar (`is_dirty`), mostrar un punto sutil naranja `●` a la derecha del nombre en lugar de una cruz simple.
- **Pestañas Inactivas**:
  - Texto en `theme.text_muted`, fondo transparente o `theme.surface_bg`.

#### 3. Status Bar (Barra de Estado Inferior)
- Encapsular cada sección en "pills" visuales coherentes:
  - **Git Branch**: ` main` con ícono y texto limpio.
  - **Posición**: `Ln 14, Col 28`.
  - **Formato**: `UTF-8` • `LF` • `Rust`.
  - **Minimap Status**: Chip cliqueable `MAP` que indica si el minimap está visible u oculto y permite alternarlo con un click.

---

## Criterios de Aceptación y Verificación

1. **Compilación y Tests**:
   - `cargo check --workspace` compila con **0 warnings**.
   - `cargo test --workspace` pasa con **100% de tests verdes** (al menos 224+ tests).
2. **Explorer Jerárquico**:
   - No se muestran rutas planas largas como `crates/karkinos-core/src/buffer.rs`.
   - Se muestra la carpeta `crates` con chevron `▸`/`▾`, adentro `karkinos-core`, adentro `src`, y los archivos adentro.
   - Hacer click en una carpeta la pliega o despliega instantáneamente.
   - El botón "Collapse All" cierra todas las carpetas secundarias.
3. **Desaparición de la Barra Gris a la Derecha**:
   - Al abrir el editor normalmente, **no hay ninguna barra gris a la derecha**. El texto y la barra de scroll terminan limpios en el borde derecho.
   - Al presionar `Ctrl+Shift+M`, el minimap aparece a la derecha separado por una línea sutil de 1px. Al volver a presionar `Ctrl+Shift+M`, se oculta y el editor vuelve a 100% de ancho.
4. **Commits y Versión**:
   - Versionado incrementado a `v0.2.38`.
   - Commits convencionales limpios sin Co-Authored-By ni atribuciones de IA.
