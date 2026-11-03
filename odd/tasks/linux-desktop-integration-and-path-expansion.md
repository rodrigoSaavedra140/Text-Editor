# Roadmap v0.2.46: Linux Desktop Integration, Arch PKGBUILD & Path Expansion (`~`)

## Contexto & Objetivos
Continuando con la resolución de los problemas de instalación y usabilidad encontrados en Arch Linux y otras distribuciones:

1. **Expansión de Tilde (`~`) y rutas de usuario**:
   - En Linux es estándar ingresar rutas abreviadas como `~/Proyectos` o `~/code/app`.
   - `std::path::Path::new("~/...")` en Rust no expande la tilde (`~`) por defecto, provocando que tanto el modal `📂 Open Folder` como la invocación por CLI fallen con `"Not a folder"`.
   - Solución: helper `expand_path_str(input: &str, base: &Path) -> PathBuf` que expande `~` usando `$HOME` y normaliza la ruta antes de verificar `is_dir()` o `is_file()`.

2. **Integración de Escritorio Linux (Desktop Entry & Icon)**:
   - Los usuarios de Linux esperan que Karkinos aparezca en el lanzador de aplicaciones (Rofi, Wofi, GNOME Dash, KDE Kickoff) y en el menú del sistema.
   - Solución: archivo `karkinos.desktop` estandarizado XDG y script para instalar el icono de alta resolución en `hicolor/256x256/apps/`.

3. **Instalador Nativo y PKGBUILD para Arch Linux**:
   - `scripts/install.sh`: instalador que soporta instalación local (`~/.local/bin`, `~/.local/share/`) o global (`/usr/local`), instalando binario, icono, desktop entry y comprobando si el directorio está en el `$PATH` del usuario.
   - `scripts/PKGBUILD`: receta estándar de empaquetado para Arch Linux (AUR / `makepkg -si`), permitiendo instalar Karkinos con gestión de paquetes nativa `pacman`.

---

## Tarea A: Helper de Expansión de Rutas (`~` y `$HOME`) en `karkinos`

### Archivos Afectados
- `karkinos/src/util.rs`
- `karkinos/src/app.rs`

### Especificación Técnica
1. En `karkinos/src/util.rs`:
   - Implementar:
     ```rust
     pub fn expand_user_path(raw: &str, base: &std::path::Path) -> std::path::PathBuf
     ```
     - Si `raw` empieza con `~` (ej. `~` o `~/...` o `~\...` en Windows):
       - Obtiene el directorio home con `std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))`.
       - Si existe, concatena el resto de la ruta.
     - Si la ruta resultante es relativa, la concatena contra `base`.
     - Si es absoluta, la mantiene.
2. Integrar en `karkinos/src/app.rs`:
   - En `confirm_open_folder`:
     - Usar `expand_user_path(trimmed, &self.doc.browse_dir)` antes de evaluar `is_dir()`.
   - En `init_from_cli_path`:
     - Usar `expand_user_path` sobre la ruta recibida.
3. Tests unitarios:
   - `expand_user_path` expande `~/test` al home del usuario.
   - `expand_user_path` mantiene rutas absolutas existentes.
   - `expand_user_path` resuelve rutas relativas contra `base`.
   - `confirm_open_folder` acepta `~/...` hacia un directorio válido del usuario.

---

## Tarea B: Desktop Entry y Activos del Sistema (`karkinos.desktop`)

### Archivos Afectados
- `assets/karkinos.desktop`
- `assets/karkinos.png`

### Especificación Técnica
1. Crear `assets/karkinos.desktop`:
   ```ini
   [Desktop Entry]
   Version=1.0
   Type=Application
   Name=Karkinos
   GenericName=Text Editor and IDE
   Comment=Lightweight, ultra-fast native Rust IDE
   Exec=karkinos %F
   Icon=karkinos
   Terminal=false
   Categories=Development;IDE;TextEditor;
   MimeType=text/plain;text/x-rust;text/x-c;text/x-c++;text/javascript;application/json;
   StartupNotify=true
   StartupWMClass=karkinos
   Keywords=text;editor;rust;ide;code;
   ```
2. Asegurar icono `assets/karkinos.png` en la raíz de assets (copia o enlace de `karkinos/assets/logo.png`).

---

## Tarea C: Scripts de Instalación y PKGBUILD de Arch Linux

### Archivos Afectados
- `scripts/install.sh`
- `scripts/PKGBUILD`
- `docs/index.html`

### Especificación Técnica
1. Crear `scripts/install.sh`:
   - Permisos ejecutables (`chmod +x`).
   - Detecta si corre como root o usuario normal:
     - Root: `PREFIX="/usr/local"`
     - Usuario normal: `PREFIX="$HOME/.local"`
   - Compila o copia el binario `karkinos` a `$PREFIX/bin/karkinos`.
   - Copia `assets/karkinos.desktop` a `$PREFIX/share/applications/karkinos.desktop`.
   - Copia el icono a `$PREFIX/share/icons/hicolor/256x256/apps/karkinos.png`.
   - Actualiza cache de iconos (`gtk-update-icon-cache` si existe) y de desktop (`update-desktop-database` si existe).
   - Comprueba si `$PREFIX/bin` está en `$PATH` y emite una recomendación amigable si no lo está.
2. Crear `scripts/PKGBUILD`:
   - Definición estándar de paquete Arch Linux:
     - `pkgname=karkinos`
     - `pkgver=0.2.46`
     - `pkgrel=1`
     - `pkgdesc="Lightweight, ultra-fast native Rust IDE"`
     - `arch=('x86_64')`
     - `license=('BSL-1.1')`
     - `depends=('gcc-libs')`
     - `makedepends=('cargo')`
     - `build()`: `cargo build --release --locked -p karkinos`
     - `package()`: instala binario en `/usr/bin`, desktop file en `/usr/share/applications`, icono en `/usr/share/icons/hicolor/256x256/apps/` y licencia.
3. Actualizar `docs/index.html` y versiones:
   - Sincronizar versión a `v0.2.46` en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml` y `docs/index.html`.
   - Agregar en la página de descargas de Linux la opción de instalar con `curl ... | bash` o PKGBUILD en Arch Linux.
