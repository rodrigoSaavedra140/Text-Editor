# Roadmap v0.2.45: Linux Wayland Native, CLI Path Arguments & Workspace Folder Switcher

## Contexto & Objetivos
Tras probar Karkinos en instalaciones nativas de Arch Linux (Wayland, KDE Plasma 6 / Hyprland / GNOME), se diagnosticaron tres fricciones críticas que impedían la adopción en Linux moderno:

1. **Falta de soporte Wayland en `eframe` & pánicos de terminal**:
   - `karkinos/Cargo.toml` incluye `eframe` solo con `x11` y sin `wayland`, fallando en entornos Wayland puros sin Xwayland.
   - Pánicos en la negociación de display OpenGL/EGL dejaban la TTY/consola en estados corruptos sin restaurar.
   - Solución: habilitar el feature `wayland` en `eframe` para Linux y agregar un panic hook de seguridad en `main.rs` que restaure la terminal a modo canónico.

2. **Argumentos de línea de comandos (`std::env::args`) ignorados**:
   - Ejecutar `karkinos path/to/file.rs` o `karkinos path/to/project` no hacía nada con los argumentos; iniciaba en blanco en el `current_dir()`.
   - Solución: procesar argumentos CLI en `main.rs`, soportando flags `--version`, `--help`, y apertura directa de carpetas o archivos en la pestaña inicial.

3. **Imposibilidad de cambiar de carpeta de trabajo (encapsulamiento en install dir)**:
   - El explorador lateral estaba acoplado rígidamente a `current_dir()`, sin botón ni comando de "Open Folder". Si se ejecutaba desde `~/.cargo/bin`, no permitía abrir ningún proyecto externo.
   - Solución: función `open_workspace_folder(&mut self, path: &Path)`, comando en Command Palette `file:open_folder` y botón/modal en el explorador para cambiar de carpeta raíz.

---

## Tarea A: Soporte Wayland nativo y Panic Hook en `karkinos`

### Archivos Afectados
- `karkinos/Cargo.toml`
- `karkinos/src/main.rs`

### Especificación Técnica
1. En `karkinos/Cargo.toml`:
   - En `[target.'cfg(target_os = "linux")'.dependencies]`:
     ```toml
     eframe = { version = "0.27", default-features = false, features = ["default_fonts", "glow", "persistence", "x11", "wayland"] }
     ```
2. En `karkinos/src/main.rs`:
   - Instalar un panic hook con `std::panic::set_hook` al inicio de `main()`:
     - Emite el mensaje de pánico con formato claro a `eprintln!`.
     - Imprime secuencias ANSI de escape estándar de restauración de consola (`\x1b[0m\x1b[?25h\x1b[?1049l` para restaurar atributos, mostrar cursor y salir de alternate screen buffer).
   - Manejo de argumentos iniciales `--help`, `-h`, `--version`, `-v`:
     - `--version` / `-v`: imprime `karkinos <version>` y sale con código 0.
     - `--help` / `-h`: imprime uso básico (`karkinos [PATH]`) y sale con código 0.
   - Detectar si se pasó un path como argumento posicional (`target_path: Option<PathBuf>`) y pasarlo al setup de `EditorApp`.

---

## Tarea B: Apertura de archivo o carpeta desde CLI en `EditorApp`

### Archivos Afectados
- `karkinos/src/app.rs`
- `karkinos/src/document_service.rs`

### Especificación Técnica
1. En `EditorApp`:
   - Método `pub fn init_from_cli_path(&mut self, path: &std::path::Path)`:
     - Si `path.is_dir()`:
       - Configura `self.doc.browse_dir = canonical_or_absolute(path)`.
       - Refresca el explorador con `self.refresh_explorer()`.
       - Refresca Git (`refresh_git_status()`, `refresh_branch()`).
     - Si `path.is_file()`:
       - Si tiene carpeta contenedora (`parent()`): establece `self.doc.browse_dir` al padre y refresca el explorador.
       - Abre el archivo con `self.open_path(canonical_or_absolute(path))`.
     - Si el archivo no existe todavía pero su padre sí (creación de archivo nuevo vía `karkinos nuevo.rs`):
       - Establece `browse_dir` al padre o actual.
       - Abre/prepara la pestaña con esa ruta y buffer vacío, lista para guardar.
2. Tests unitarios en `karkinos/src/app.rs`:
   - `init_from_cli_path` con directorio existente cambia `browse_dir` y refresca explorador.
   - `init_from_cli_path` con archivo existente abre la pestaña activa y setea `browse_dir` al padre.
   - `init_from_cli_path` con archivo nuevo prepara la pestaña con el path indicado.

---

## Tarea C: Comando "Open Folder" y cambio dinámico de Workspace

### Archivos Afectados
- `karkinos/src/app.rs`
- `karkinos/src/ui.rs`

### Especificación Técnica
1. En `karkinos/src/app.rs`:
   - `pub fn open_workspace_folder(&mut self, folder: &std::path::Path)`:
     - Asigna `self.doc.browse_dir = folder.to_path_buf()`.
     - Llama a `self.refresh_explorer()`.
     - Llama a `self.refresh_git_status()` y `self.refresh_branch()`.
     - Limpia estados de búsqueda global de workspace y modal de git diff si estaban abiertos.
     - Notifica en `self.doc.status = format!("Opened folder: {}", folder.display())`.
   - Estado de modal para cambio de carpeta:
     - `pub open_folder_modal: Option<String>` (input de ruta).
     - `pub fn prompt_open_folder(&mut self)`
     - `pub fn confirm_open_folder(&mut self)`
   - Catálogo `PALETTE_COMMANDS`:
     - Agregar `PaletteCommand { id: "file:open_folder", title: "File: Open Folder (Change Workspace)", category: "File", shortcut: Some("Ctrl+K Ctrl+O") }`.
     - En `execute_palette_command("file:open_folder")`: invoca `self.prompt_open_folder()`.
2. En `karkinos/src/ui.rs`:
   - Renderizar modal de "Open Folder" cuando `self.open_folder_modal.is_some()`:
     - Input de texto con la ruta de la carpeta y autocompletado/validación simple (`Enter` confirma, `Escape` cancela).
   - En la barra de herramientas del explorador lateral (`render_explorer_tab`):
     - Agregar botón `📁` con tooltip `"Open Folder..."` al lado de los botones de crear archivo/carpeta.
3. Tests unitarios:
   - `open_workspace_folder` actualiza `browse_dir`, refresca el explorador y notifica el status.
   - `execute_palette_command` abre el modal de `file:open_folder`.
