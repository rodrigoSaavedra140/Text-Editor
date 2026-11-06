# Feature: Auto-actualización in-app, ícono del .exe de Windows y paquete AUR

**Objetivo**: un botón oculto que aparece cuando hay un release nuevo y actualiza la app solo (Windows y Linux), ícono embebido en el `.exe` de Windows, y paquete AUR `karkinos-bin` para que la actualización del sistema Linux también actualice Karkinos.
**Branch**: `feat/self-update` (commits convencionales, SIN líneas de atribución AI / Co-Authored-By). Un commit por tarea.
**Runner**: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` (hoy en verde: 266 + 2 ignored y 149 tests).
**Versionado**: lo sube el orquestador al commitear. El implementador NO toca versiones.

## Reglas para el implementador (leer antes de cada tarea)

1. Hacé SOLO la tarea que te asignaron. No commitees, no hagas push, no crees tags.
2. Leé completos los archivos y símbolos que nombra la tarea antes de tocar código. Los números de línea son de v0.2.84 y pueden correrse: buscá el símbolo.
3. Test primero cuando la regla es testeable: escribí el test, verificá que FALLA (RED), implementá, verificá que PASA (GREEN). Reportá la salida RED y GREEN.
4. Lógica pura (comparar versiones, elegir asset, parsear `SHA256SUMS`, decidir tipo de instalación) va en funciones puras testeables; la red, el disco y los procesos quedan en una capa fina. Los tests NUNCA usan red, NUNCA reemplazan el ejecutable real y escriben solo en un directorio temporal único.
5. Reutilizá lo que existe: cliente `https_agent()` / `write_atomic` / tope de bytes de `crates/karkinos-core/src/open_vsx.rs`; patrón `Option<Receiver<Result<T, String>>>` + `std::thread::spawn` + `try_recv` por frame de `karkinos/src/extensions_service.rs`; `title_bar::title_bar_icon_button` para el botón.
6. Comentarios en español, explicativos, como el código existente. Textos de UI en inglés.
7. No agregues dependencias salvo las que nombra la tarea. No reformatees archivos enteros. No cambies comportamiento que la tarea no pide.
8. Al terminar: `cargo test --workspace` y `cargo clippy --workspace --all-targets -- -D warnings` en verde; decí qué `S#` cubriste y con qué evidencia.

## Specs

Pedido original (verbatim): "ahora quiero que la app tenga un boton que este oculto y aparezca cuando haya una update y si lo apretas se actualize solo, tanto en windows como en linux y en windows agregale un icono al ejecutable y tambien cuando se haga un update sobre el sistema de linux y tenga una update se actualice"

- **S1 — Botón oculto que aparece con una update.** "un boton que este oculto y aparezca cuando haya una update". La app consulta el último release de GitHub (`rodrigoSaavedra140/Text-Editor`) al iniciar y cada 6 horas, en un hilo aparte. El botón en la barra de título está oculto mientras no haya una versión más nueva que `env!("CARGO_PKG_VERSION")`, y aparece cuando la hay, con tooltip `Update to vX.Y.Z`. Sin red o con error de la API no aparece nada ni se interrumpe al usuario.
- **S2 — Al apretarlo se actualiza solo (Windows y Linux).** "si lo apretas se actualize solo, tanto en windows como en linux". En una instalación portable (binario fuera del gestor de paquetes), apretar el botón descarga el binario de la plataforma, lo verifica (S5) y reemplaza el ejecutable actual. Decisión del usuario: **"Pregunta y reinicia"** — al terminar pregunta si reiniciar; si hay archivos sin guardar lo avisa antes; al confirmar la app se reinicia en la versión nueva.
- **S3 — Ícono del ejecutable de Windows.** "en windows agregale un icono al ejecutable". `karkinos.exe` lleva embebido el ícono de la app (se ve en el Explorador de archivos y la barra de tareas).
- **S4 — La actualización del sistema Linux actualiza Karkinos.** "cuando se haga un update sobre el sistema de linux y tenga una update se actualice". Decisión del usuario: **paquete AUR `karkinos-bin`** que instala el binario del release; cada release publica su versión en AUR, así `yay -Syu` / `paru -Syu` lo actualizan. Decisión del usuario para el botón cuando Karkinos lo instaló el gestor de paquetes: **"Abre terminal con yay -S"** — no reemplaza `/usr/bin/karkinos`; ejecuta `yay -S karkinos-bin` (o `paru -S karkinos-bin` si no hay yay) en la terminal integrada.
- **S5 — Verificación de integridad.** Decisión del usuario: **"SHA-256 del release"**. Cada release publica `SHA256SUMS`; la app solo reemplaza el ejecutable si el SHA-256 del archivo descargado coincide con su línea en `SHA256SUMS`. Si no coincide, o falta, no toca nada y muestra el error.

## Tasks

| ID | Specs | Riesgo | Estado | Commit |
|----|-------|--------|--------|--------|
| T1 | S5, S2 | Alto: item 5 (release/CI) | hecha | `1f7b8fc` ci(release): publish raw binaries and SHA256SUMS |
| T2 | S3 | Alto: item 5 (dependencia de build) | hecha | `build(windows): embed the app icon in karkinos.exe` |
| T3 | S1 | Bajo | pendiente | `feat(update): show an update button when a new release exists` |
| T4 | S2, S5 | Alto: items 1, 2 (reemplaza el ejecutable, descarga código) | pendiente | `feat(update): download, verify and install updates in place` |
| T5 | S4 | Medio | pendiente | `feat(update): update package-managed installs through yay or paru` |
| T6 | S4 | Alto: item 5 (publicación AUR) | pendiente | `ci(aur): publish karkinos-bin to the AUR on each release` |

---

### T1 — Release: binarios sueltos y `SHA256SUMS` (S5, S2)

**Dónde.** `.github/workflows/release.yml` y su copia idéntica `scripts/release-workflow.yml` (mantené las dos iguales).

**Qué.** Sin cambiar los archivos que ya se publican (`karkinos-linux-x86_64.tar.gz`, `karkinos-windows-x86_64.zip`), cada release publica además:
- `karkinos-linux-x86_64` (el binario Linux suelto) y `karkinos-windows-x86_64.exe` (el `.exe` suelto). La app descarga estos, así no hace falta descomprimir (sin dependencias de tar/zip).
- `SHA256SUMS`: formato de `sha256sum` (`<hash hex en minúsculas>  <nombre>`), una línea por cada uno de los 4 archivos anteriores. Como cada plataforma corre en su propio job de la matriz, agregá un job final `checksums` (`needs: build`, `ubuntu-latest`) que baje los 4 assets del release (`gh release download "$GITHUB_REF_NAME"` con `GH_TOKEN`), corra `sha256sum` y suba `SHA256SUMS` con `gh release upload --clobber`.

**Verificación.** No hay runner local de Actions: validá el YAML (`python3 -c "import yaml,sys; yaml.safe_load(open(sys.argv[1]))" <archivo>` si está PyYAML, o `actionlint` si está) y que las dos copias sean idénticas (`diff`). Reportá el bloque nuevo.

### T2 — Ícono embebido en `karkinos.exe` (S3)

**Qué.**
1. Generá `karkinos/assets/karkinos.ico` desde `assets/karkinos.png` con ImageMagick (disponible: `magick`), con tamaños 16, 24, 32, 48, 64, 128 y 256: `magick assets/karkinos.png -define icon:auto-resize=256,128,64,48,32,24,16 karkinos/assets/karkinos.ico`.
2. Agregá `winresource` como `[build-dependencies]` de `karkinos` (la única dependencia nueva permitida en esta tarea) y un `karkinos/build.rs` que, SOLO cuando `CARGO_CFG_TARGET_OS == "windows"`, embeba el `.ico` (`WindowsResource::new().set_icon("assets/karkinos.ico").compile()`). En Linux el build script no hace nada más que `cargo:rerun-if-changed`.

**Verificación.** `cargo build -p karkinos` y la suite en Linux siguen verdes (el build script no rompe Linux). Reportá el tamaño del `.ico` y `magick identify karkinos/assets/karkinos.ico` (las 7 resoluciones). El efecto real en Windows se confirma en el release (job de Windows del CI).

### T3 — Chequeo de updates y botón oculto (S1)

**Lógica pura** en `crates/karkinos-core/src/updater.rs` (nuevo módulo, exportalo en `lib.rs`):
- `parse_version("v0.2.84") -> Option<(u64,u64,u64)>` (acepta con y sin `v`) y `is_newer(latest, current) -> bool`.
- `struct ReleaseInfo { version: String, assets: Vec<ReleaseAsset { name, download_url }> }` y `parse_latest_release(json: &str) -> Result<ReleaseInfo, String>` sobre la respuesta de `GET https://api.github.com/repos/rodrigoSaavedra140/Text-Editor/releases/latest` (`tag_name`, `assets[].name`, `assets[].browser_download_url`), con `serde`.
- `platform_asset_name() -> Option<&'static str>`: `karkinos-linux-x86_64` en Linux x86_64, `karkinos-windows-x86_64.exe` en Windows x86_64, `None` en otras.
- `fetch_latest_release() -> Result<ReleaseInfo, String>`: capa fina con `https_agent()`, header `User-Agent: karkinos/<versión>` (GitHub lo exige) y `Accept: application/vnd.github+json`, con timeout.

**UI** en `karkinos`: un `UpdateService` (archivo nuevo `karkinos/src/update_service.rs`) con el patrón Receiver de `extensions_service.rs`: chequeo al iniciar y luego cada 6 horas (`request_repaint_after` para despertar), `available: Option<ReleaseInfo>` solo si `is_newer` y existe el asset de la plataforma. En la barra de título (`ui.rs`, bloque right-to-left ~3150-3185, junto a los botones de cargo) un botón con `title_bar_icon_button` que SOLO se dibuja cuando `available.is_some()`, con tooltip `Update to vX.Y.Z`. En esta tarea el click llama a un método `EditorApp::start_update()` que por ahora solo deja un estado `pending` observable (T4 lo implementa).

**Tests (RED primero).** `parse_version` (con/sin `v`, inválidas → `None`), `is_newer` (mayor/menor/igual en cada componente, `0.2.10 > 0.2.9`), `parse_latest_release` con un JSON fijo de ejemplo (y JSON inválido → `Err`), `platform_asset_name` en la plataforma del test. En `karkinos`: un test que, sin red, inyecte un `ReleaseInfo` (más nuevo / igual / sin asset de la plataforma) en el servicio y verifique que el botón está visible solo en el primer caso (exponé un `fn update_button_visible(&self) -> bool` que la UI y el test usan).

### T4 — Descargar, verificar e instalar (S2, S5)

**Lógica pura** en `updater.rs`:
- `parse_sha256sums(text) -> HashMap<String, String>` y `expected_sha256(sums, asset_name) -> Option<String>`.
- `sha256_hex(bytes) -> String` con el crate `sha2` (única dependencia nueva permitida, en `karkinos-core`).
- `verify(bytes, sums_text, asset_name) -> Result<(), String>`: error si falta la línea o el hash no coincide (mensajes en inglés: `checksum missing for <asset>` / `checksum mismatch for <asset>`).
- `install_kind(exe_path, owned_by_package: bool) -> InstallKind { Portable, Package }` (el chequeo real de pacman va en T5; acá `Package` solo si `owned_by_package`).

**Instalación (capa fina, en `updater.rs` o `update_service.rs`)**, en un hilo aparte:
1. Descargar `SHA256SUMS` y el asset de la plataforma (con el tope de bytes del módulo Open VSX o uno propio razonable, p. ej. 200 MB).
2. `verify`. Si falla: no tocar nada, mostrar el error.
3. Reemplazar `std::env::current_exe()`:
   - **Linux**: escribir `karkinos.new` en el MISMO directorio, `chmod 0755`, `rename` sobre el ejecutable (es atómico y válido aunque esté corriendo). Si el directorio no es escribible: error claro (`cannot write <dir>: reinstall Karkinos or update it with your package manager`).
   - **Windows**: un `.exe` en ejecución no se puede sobrescribir pero sí renombrar: renombrar el actual a `karkinos.exe.old`, escribir el nuevo con el nombre original; si falla la escritura, volver a renombrar el `.old` (rollback). Al iniciar la app, borrar un `karkinos.exe.old` sobrante si existe.
4. Al terminar: diálogo modal `Karkinos vX.Y.Z is installed. Restart now?` (`Restart` / `Later`). Si hay pestañas con cambios sin guardar, el diálogo lo dice (reutilizá el chequeo de `dirty` que ya usa el cierre/Exit) antes de reiniciar.
5. **Reinicio**: lanzar el ejecutable nuevo con los mismos argumentos y cerrar esta ventana. OJO: la app tiene single-instance IPC (ver `karkinos/src/ipc.rs`); el proceso nuevo no debe reenviar sus argumentos a la instancia vieja y salir. Resolvelo (por ejemplo con un flag interno `--after-update <pid>` que espera a que termine el proceso viejo antes de tomar el single-instance) y documentalo en el código.

Mientras descarga, el botón muestra un spinner y no se puede volver a apretar.

**Tests (RED primero).** `parse_sha256sums` (dos espacios y `*` de modo binario), `verify` OK / hash distinto / línea faltante; reemplazo del ejecutable sobre un archivo falso dentro de un directorio temporal (función que reciba la ruta destino; nunca `current_exe()` en tests): el destino queda con el contenido nuevo y, en Unix, modo 0755; si la verificación falla el destino queda intacto byte a byte. El flujo de reinicio y el single-instance se prueban con su lógica de decisión pura si la extraés; NO lances procesos en los tests.

### T5 — Instalaciones del gestor de paquetes (S4, botón)

- Linux: `owned_by_package(exe) -> bool` = `exe` bajo `/usr/` y `pacman -Qo <exe>` termina con éxito (si `pacman` no existe → `false`).
- Si `InstallKind::Package`, apretar el botón NO reemplaza nada: abre la terminal integrada y ejecuta `yay -S karkinos-bin`; si no hay `yay` pero sí `paru`, `paru -S karkinos-bin`; si no hay ninguno, un mensaje `Update Karkinos with your package manager (karkinos-bin on the AUR).` Buscá en `karkinos/src/terminal.rs` / `ui.rs` cómo se abre la terminal y se le escribe un comando y reutilizalo; no arranques una PTY real en tests.
- Tests: la decisión de qué comando usar es una función pura `package_update_command(has_yay, has_paru) -> Option<&'static str>` con sus 4 casos; `install_kind` con `owned_by_package` true/false.

### T6 — Paquete AUR `karkinos-bin` (S4)

- `packaging/aur/karkinos-bin/PKGBUILD`: `pkgname=karkinos-bin`, `provides=('karkinos')`, `conflicts=('karkinos')`, `arch=('x86_64')`, `source` = `karkinos-linux-x86_64.tar.gz` del release `v$pkgver` + `assets/karkinos.desktop`, `assets/karkinos.png` y `LICENSE` desde `https://raw.githubusercontent.com/rodrigoSaavedra140/Text-Editor/v$pkgver/...`; `sha256sums` reales; `package()` instala en las mismas rutas que `scripts/PKGBUILD`.
- Job nuevo `aur` en `release.yml` (y su copia), `needs: checksums`, que: arma el PKGBUILD con `pkgver` del tag y los `sha256sums` de los archivos, genera `.SRCINFO`, y lo publica en `ssh://aur@aur.archlinux.org/karkinos-bin.git` con la clave del secret `AUR_SSH_PRIVATE_KEY` (podés usar `KSXGitHub/github-action-publish-to-aur@v4`). Si el secret no está definido, el job se SALTA sin fallar (`if: ${{ secrets.AUR_SSH_PRIVATE_KEY != '' }}` vía `env`).
- README: sección corta "Install on Arch Linux (AUR)" con `yay -S karkinos-bin`, y para el mantenedor los pasos para crear la cuenta AUR, cargar la clave pública SSH y el secret `AUR_SSH_PRIVATE_KEY`.
- **Bloqueo del usuario**: la primera publicación necesita la cuenta AUR y el secret; sin ellos el job se salta.

## Log

- **L1** (pedido original, verbatim): "ahora quiero que la app tenga un boton que este oculto y aparezca cuando haya una update y si lo apretas se actualize solo, tanto en windows como en linux y en windows agregale un icono al ejecutable y tambien cuando se haga un update sobre el sistema de linux y tenga una update se actualice"
- **L2** (decisiones del usuario): Linux sistema → "Paquete AUR karkinos-bin (recomendado)"; botón con instalación de pacman → "Abre terminal con yay -S (recomendado)"; verificación → "SHA-256 del release (recomendado)"; tras actualizar → "Pregunta y reinicia (recomendado)".
- **L3** (preferencia del usuario): "quiero que toda la implementacion lo haga deepseek" — cada tarea la implementa `gentle-ai-worker` (opencode-go/deepseek-v4.1-flash); Opus solo orquesta.
- **L4** (exploración, v0.2.84): los releases no publican checksums; los archivos contienen solo el binario; no existe `build.rs` ni ícono `.ico`; no hay paquete AUR (`scripts/PKGBUILD` compila desde el checkout); single-instance IPC en `karkinos/src/ipc.rs`.
- **L5** (T1, DeepSeek): `release.yml` publica `karkinos-linux-x86_64` y `karkinos-windows-x86_64.exe` sueltos y un job `checksums` (`needs: build`, solo en tags `v*`) sube `SHA256SUMS` con las 4 líneas. Verificación estructural: YAML parsea (Ruby Psych; no hay PyYAML/actionlint), `bash -n` de los bloques `run:`, `scripts/release-workflow.yml` idéntico (`diff` vacío). Revisión del orquestador: nombres y formato coinciden con S5/T4. Pendiente: se ejercita de verdad en el primer tag.
- **L6** (T2, DeepSeek): `karkinos/assets/karkinos.ico` (7 resoluciones 16-256, desde `assets/karkinos.png`) y `karkinos/build.rs` que lo embebe con `winresource` 0.1.34 solo si `CARGO_CFG_TARGET_OS == "windows"`. Build Linux, suite (266 + 2 ignored, 149) y clippy en verde. Sin RED posible: el recurso PE solo existe en el build de Windows; se confirma en el job de Windows del próximo release.
- **L7** (decisión del orquestador): la versión se sube una sola vez, en el último commit de la feature (no por tarea), porque los commits de la rama no se taggean.
