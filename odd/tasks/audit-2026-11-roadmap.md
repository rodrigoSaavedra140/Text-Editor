# Feature: Roadmap de auditoría 2026-11 (integridad de datos, seguridad y deuda técnica)

**Objetivo**: corregir las fallas encontradas en la auditoría del 2026-11-06 y bajar la deuda técnica, en tareas chicas e independientes, implementables una por una.
**Branch**: `main` (commits convencionales, SIN líneas de atribución AI / Co-Authored-By).
**Runner de tests**: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` (hoy ambos pasan limpios: mantenerlo así).
**Versionado**: cada tarea cerrada sube el patch (`0.2.64` → `0.2.65` …) en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml` y `scripts/PKGBUILD`, igual que los commits anteriores.

## Reglas para el implementador (leer antes de cada tarea)

1. Hacé UNA tarea por vez, en el orden de la tabla. No mezcles tareas en un commit.
2. Antes de tocar código, leé completos los archivos y rangos de líneas que nombra la tarea. Los números de línea son de la versión 0.2.64 y pueden correrse unas líneas: buscá el símbolo nombrado.
3. Test primero: escribí el test que se pide, verificá que FALLA (RED), implementá, verificá que PASA (GREEN).
4. Reutilizá los helpers que se nombran; no reimplementes la misma lógica con otra forma.
5. No cambies comportamiento que la tarea no pide. No reformatees archivos enteros. No toques dependencias salvo donde la tarea lo indica.
6. Los comentarios del código siguen el estilo existente (español, explicativos). Los textos de la UI siguen en inglés.
7. Al terminar cada tarea: `cargo test --workspace` y `cargo clippy --workspace --all-targets -- -D warnings` en verde, subir la versión y hacer un commit con el mensaje sugerido.
8. Los tests usan directorios temporales únicos (`std::env::temp_dir().join(format!("karkinos_<nombre>_{}", std::process::id()))`) y los borran al final, como los tests existentes.

## Specs

- **S1 — Guardado atómico.** Guardar un archivo nunca debe dejarlo truncado o a medio escribir si el proceso muere, el disco se llena o la escritura falla. El archivo original queda intacto hasta que el nuevo está completo en disco.
- **S2 — `dirty` correcto si se edita durante un guardado.** Si el usuario edita el documento mientras el guardado asincrónico está en curso, al terminar el documento debe seguir marcado como modificado (`dirty = true`), porque lo que está en disco ya no coincide con el buffer.
- **S3 — `current_file` solo cambia si el guardado tuvo éxito.** Un guardado fallido a una ruta nueva no debe dejar la pestaña apuntando a esa ruta.
- **S4 — Nombres de `.vsix` seguros.** Ningún dato que llegue de Open VSX (`name`, `version`, `download_url`) puede hacer que se escriba o se borre un archivo fuera de `~/.karkinos/extensions/`.
- **S5 — Mutex envenenado no tumba la UI.** Si un hilo de fondo entra en pánico mientras tiene tomado un `Mutex` compartido con la UI, la UI sigue funcionando.
- **S6 — Etiqueta honesta de extensiones.** Mientras las extensiones solo se descarguen (no se cargan), la UI no debe decir "Installed".
- **S7 — Cliente HTTP nativo.** Open VSX deja de depender del binario `curl` del sistema.
- **S8 — Una sola fuente de verdad del texto.** El texto de cada documento vive en un solo lugar (el `Rope` de `TextBuffer`); `lines: Vec<String>` desaparece o pasa a ser una vista derivada, nunca editada por separado.
- **S9 — Partir `app.rs` / `ui.rs`.** Extraer módulos por dominio sin cambiar comportamiento.

## Tasks

| ID | Specs | Riesgo | Estado | Commit sugerido |
|----|-------|--------|--------|-----------------|
| T1 | S1 | Alto (datos) | hecha | `fix(save): write files atomically via temp file and rename` |
| T2 | S2, S3 | Alto (datos) | hecha | `fix(save): keep dirty flag when buffer changes during save` |
| T3 | S4 | Alto (seguridad) | hecha | `fix(extensions): sanitize Open VSX file names and download URLs` |
| T4 | S5 | Medio | hecha | `fix(ui): recover from poisoned mutexes instead of panicking` |
| T5 | S6 | Bajo | hecha | `fix(extensions): label downloaded packages as Downloaded` |
| T6 | S7 | Medio (dependencia) | hecha | `refactor(open-vsx): replace curl subprocess with ureq` |
| T7 | S8 | Alto (núcleo) | pendiente | `refactor(document): make the rope the single text source` |
| T8 | S9 | Medio | pendiente | `refactor(app): split extensions, explorer and git into modules` |

---

### T1 — Guardado atómico (S1)

**Problema.** `karkinos/src/document_service.rs`, función `DocumentService::save` (~línea 412). Dentro del `thread::spawn` (~línea 457) hace `std::fs::File::create(&path)`, que VACÍA el archivo original, y después escribe en ~20 pedazos con `thread::sleep(15ms)` entre cada uno. Si algo falla en el medio, el archivo del usuario queda truncado.

**Referencia correcta que ya existe.** `crates/karkinos-core/src/recovery.rs`, `save_recovery_file` (~línea 182): escribe en `<nombre>.tmp`, hace `sync_all()`, luego `fs::rename` sobre el destino, con fallback para Windows usando `.old`.

**Cómo hacerlo.**
1. En `crates/karkinos-core/src/` creá un módulo nuevo `atomic_write.rs` (registralo en `lib.rs` con `pub mod atomic_write;`).
2. Implementá:
   ```rust
   /// Escribe `data` en `path` de forma atómica: primero a un archivo
   /// temporal hermano, `sync_all`, y después `rename` sobre el destino.
   /// `on_progress(written)` se llama después de cada pedazo escrito.
   pub fn write_atomic(
       path: &Path,
       data: &[u8],
       mut on_progress: impl FnMut(usize),
   ) -> io::Result<()>
   ```
   - Temporal: mismo directorio que `path`, nombre `.<nombre_archivo>.karkinos-tmp` (oculto, mismo filesystem para que `rename` sea atómico).
   - Escribí en pedazos (mismo esquema de ~20 pasos que hoy, SIN el `sleep`) y llamá `on_progress` tras cada uno.
   - `file.sync_all()` antes de cerrar.
   - Si el archivo destino ya existe, copiá sus permisos al temporal (`fs::set_permissions(&tmp, fs::metadata(path)?.permissions())`) antes del rename, para no perder el bit de ejecución de scripts.
   - `fs::rename(&tmp, path)`. Si falla y el destino existe, aplicá el mismo fallback `.old` que `save_recovery_file`. En cualquier error, borrá el temporal y devolvé el error.
3. Refactorizá `save_recovery_file` para que use `write_atomic` (misma conducta, menos código duplicado). Los tests existentes de `recovery.rs` deben seguir pasando sin cambios.
4. En `DocumentService::save`, reemplazá el bloque `File::create` + bucle + `flush` por `karkinos_core::atomic_write::write_atomic(&path, &data, |w| { if let Ok(mut p) = progress.lock() { p.written = w; } })`.
5. Eliminá el `thread::sleep(Duration::from_millis(15))`: solo existía para animar la barra y hace el guardado más lento a propósito.

**Tests (en `atomic_write.rs`).**
- `write_atomic_crea_archivo_nuevo`: el contenido final es exactamente `data`.
- `write_atomic_reemplaza_contenido_existente`: archivo previo con "viejo", escribir "nuevo", leer da "nuevo".
- `write_atomic_no_deja_temporales`: después de escribir, el directorio solo contiene el archivo destino.
- `write_atomic_fallido_preserva_original`: destino con "original"; hacé fallar la escritura (por ejemplo, un `path` cuyo directorio padre es un archivo, o un directorio padre sin permisos de escritura en Unix con `#[cfg(unix)]`) y verificá que el original sigue con "original".
- `write_atomic_preserva_permisos` (`#[cfg(unix)]`): archivo con modo `0o755`, después de reescribir sigue en `0o755`.
- `write_atomic_reporta_progreso`: el último valor que recibe `on_progress` es `data.len()`.
- El test existente `save_con_input_vacio_guarda_como_nuevo_txt` (app.rs ~3915) debe seguir pasando.

**Aceptación.** Ningún `File::create` sobre el destino final del usuario en `document_service.rs`.

---

### T2 — `dirty` y `current_file` correctos (S2, S3)

**Problema A (S2).** `DocumentService::save` toma una foto del texto al empezar (`let data = self.current_text_with_ending()`, ~línea 443). Al terminar, `karkinos/src/ui.rs` (~línea 203, rama `None =>` dentro de `if done`) hace `self.doc.dirty = false` sin condición. Si el usuario tecleó durante el guardado, eso queda marcado como guardado y se pierde al cerrar.

**Problema B (S3).** `self.current_file = Some(path.clone())` se asigna (~línea 448) ANTES de saber si la escritura funcionó.

**Cómo hacerlo.**
1. En `SaveProgress` (document_service.rs ~línea 33) agregá:
   - `pub saved_text_hash: u64` — hash del texto que se escribió.
   - `pub target: PathBuf` — ruta destino.
   - `pub previous_file: Option<PathBuf>` — el `current_file` anterior.
2. En `save`, calculá el hash con `std::hash::{DefaultHasher, Hash, Hasher}` sobre el `String` que vas a escribir (antes de `into_bytes`). Guardá el `current_file` previo en `previous_file`. Podés seguir asignando `current_file = Some(path)` al empezar (la UI lo usa para mostrar el nombre), pero ver paso 4.
3. Agregá en `DocumentService` un método `pub fn current_text_hash(&self) -> u64` que hashee `current_text_with_ending()` con el mismo hasher.
4. Movè la lógica de cierre de guardado de `ui.rs` a un método nuevo `DocumentService::finish_save(&mut self)` que:
   - lea `done`, `error`, `saved_text_hash`, `previous_file` del progreso;
   - si hubo error: restaurá `self.current_file = previous_file`, `status = "Save error: ..."`, dejá `dirty` como estaba;
   - si salió bien: `self.dirty = self.current_text_hash() != saved_text_hash`; `status = "Saved"` si quedó limpio, o `"Saved (buffer changed during save)"` si sigue sucio;
   - llamá a `note_save_done()` (ya existe) y ponga `save_progress = None`.
   - Devuelve `bool` indicando si terminó sin error, para que `ui.rs` siga llamando a `cleanup_recovery_for_active_tab()` solo en éxito.
5. En `ui.rs` reemplazá el bloque inline por `let ok = self.doc.finish_save();` y mantené la lógica de `close_after_save` que viene después. OJO: `close_after_save` solo debe cerrar si `ok && !self.doc.dirty`.

**Tests (en `app.rs`, módulo `tests`, junto a `save_con_input_vacio_guarda_como_nuevo_txt`).**
- `guardar_y_editar_durante_el_guardado_deja_dirty`: `set_text("a")`, `save()`, inmediatamente `set_text("ab")`, esperar `done` (bucle acotado como los tests existentes), `finish_save()`, assert `dirty == true` y el disco contiene `"a"`.
- `guardar_sin_editar_limpia_dirty`: mismo flujo sin editar → `dirty == false`.
- `guardado_fallido_restaura_current_file`: `current_file = None`, `file_path_input` apuntando a un directorio inexistente (`/nonexistent_dir_karkinos/x.txt`), `save()`, esperar `done`, `finish_save()`, assert `current_file == None` y `status` empieza con `"Save error"`.
- `note_save_done_refresca_mtime_y_cancela_cierre_en_error` (existe) debe seguir pasando.

---

### T3 — Nombres de `.vsix` y URLs seguros (S4)

**Problema.** `karkinos/src/app.rs`: `is_vsix_installed` (~3136), `download_open_vsx_extension` (~3166) arman `format!("{}-{}.vsix", ext.name, ext.version)` con datos de la red y hacen `extensions_dir().join(...)`. `uninstall_vsix` (~3142) hace `join(filename)` sin validar. Un `name` como `../../.bashrc` escribe o borra fuera de `~/.karkinos/extensions`. Además `download_vsix` en `crates/karkinos-core/src/open_vsx.rs` (~línea 70) pasa `download_url` directo a `curl`: una URL que empiece con `-` se interpreta como opción.

**Cómo hacerlo.**
1. En `crates/karkinos-core/src/open_vsx.rs` agregá:
   ```rust
   /// Nombre de archivo `.vsix` seguro para una extensión, o `None` si
   /// `name`/`version` traen caracteres fuera de `[A-Za-z0-9._-]`,
   /// están vacíos o son `.`/`..`.
   pub fn safe_vsix_file_name(name: &str, version: &str) -> Option<String>
   /// `true` solo para URLs `https://` sin espacios ni caracteres de control.
   pub fn is_safe_download_url(url: &str) -> bool
   ```
   Y un `pub fn vsix_file_name(&self) -> Option<String>` en `OpenVsxExtension` que llame a `safe_vsix_file_name(&self.name, &self.version)`.
2. `download_vsix`: si `!is_safe_download_url(download_url)` devolvé `Err("Refusing unsafe download URL")`. Además agregá `.arg("--")` justo antes de la URL en el comando curl (hasta que T6 lo elimine) y `--proto =https`.
3. En `app.rs` creá un helper privado `fn vsix_path(file_name: &str) -> Option<PathBuf>` que rechace nombres con `/`, `\`, `..`, vacíos o sin extensión `.vsix` (mismo criterio que `validated_target` en `recovery.rs` ~291) y devuelva `extensions_dir().join(file_name)`.
4. Usá `ext.vsix_file_name()` + `vsix_path` en `is_vsix_installed`, `download_open_vsx_extension` (si es `None`: `open_vsx_status = Some("Invalid extension name".into())` y return) y en `ui.rs` ~3702 (donde se arma `vsix_name` para el botón Uninstall).
5. `uninstall_vsix` usa `vsix_path(filename)`; si es `None`, `status = "Invalid package name"` y return `false`.

**Tests.**
- En `open_vsx.rs`: `safe_vsix_file_name` acepta `("rust-analyzer", "0.3.2")` → `"rust-analyzer-0.3.2.vsix"`; rechaza `("../x", "1")`, `("a/b", "1")`, `("x", "..")`, `("", "1")`, `("x", "1 2")`.
- `is_safe_download_url`: acepta `https://open-vsx.org/x.vsix`; rechaza `http://...`, `-K/etc/passwd`, `file:///etc/passwd`, `""`.
- En `app.rs`: `uninstall_vsix("../../algo")` devuelve `false` y no borra nada (creá un archivo testigo en el directorio temporal padre y verificá que sigue existiendo).

---

### T4 — Mutex envenenados (S5)

**Problema.** `.lock().unwrap()` en código de producción hace que un pánico en un hilo de fondo tumbe la UI la próxima vez que se toma el lock. Ubicaciones conocidas: `karkinos/src/ui.rs` ~173 (`progress.lock().unwrap()`), `crates/karkinos-core/src/cargo.rs` ~364 (`proc_arc.lock().unwrap()`).

**Cómo hacerlo.**
1. Buscá todos los casos en código NO-test: `grep -rn "lock().unwrap()" karkinos/src crates/*/src` y descartá los que están dentro de `#[cfg(test)] mod tests`.
2. Reemplazá cada uno por `.lock().unwrap_or_else(|e| e.into_inner())`. No cambies los de tests.
3. Si aparece el mismo patrón 3+ veces en un archivo, un helper local `fn lock_or_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T>` es aceptable.

**Test.** En `document_service.rs` o `app.rs`: crear un `Arc<Mutex<SaveProgress>>`, envenenarlo con un hilo que hace `let _g = m.lock().unwrap(); panic!()` (joinear e ignorar el error), asignarlo a `doc.save_progress` y verificar que `finish_save()` (de T2) no paniquea.

---

### T5 — Etiqueta "Downloaded" (S6)

**Problema.** `karkinos/src/ui.rs` ~3707 muestra `"● Installed"` pero el `.vsix` solo se guarda en disco; nunca se carga.

**Cómo hacerlo.** Cambiá el texto a `"● Downloaded"`, el botón `"📥 Install"` a `"📥 Download"`, `"🗑 Uninstall"` a `"🗑 Delete"`, y el encabezado `"Downloaded .vsix Packages"` queda igual. Cambiá los mensajes de estado en `uninstall_vsix` ("Uninstalled .vsix extension" → "Deleted .vsix package"). No renombres funciones (eso sería otra tarea). Actualizá `README.md` si menciona "install" de extensiones.

**Test.** Si hay tests que comparan esos strings, actualizalos. Si no hay, no hace falta agregar (cambio de copy).

---

### T6 — `ureq` en lugar de `curl` (S7) — requiere T3

**Cómo hacerlo.**
1. En `crates/karkinos-core/Cargo.toml` agregá `ureq = { version = "2", default-features = false, features = ["tls"] }` (rustls, sin OpenSSL del sistema).
2. `OpenVsxClient::search`: `ureq::get(&url).timeout(Duration::from_secs(10)).call()`, leé el body con `into_string()` y pasalo a `parse_search_json` (no cambia).
3. `download_vsix`: mantené la validación de T3; `ureq::get(url).timeout(30s).call()`, copiá `into_reader()` con un límite de tamaño (`.take(100 * 1024 * 1024)`) a un temporal y movelo al destino con `write_atomic`/rename (o escribí al temporal y `rename`). Si la respuesta excede el límite, error.
4. Mantené las firmas públicas (`Result<_, String>`) para no tocar `app.rs`.
5. Eliminá `crate::platform::configure_background_command` de estas funciones (ya no hay subproceso).

**Tests.** Los de `parse_search_json` siguen pasando. No agregues tests que salgan a la red. `cargo build` en release debe compilar.

---

### T7 — Una sola fuente de verdad del texto (S8) — la más grande; hacerla después de T1–T6

**Problema.** `DocumentService` (document_service.rs ~123) y `TabDocument` (~57) guardan a la vez `lines: Vec<String>` y `buffer: TextBuffer` (ropey). El editor edita `lines`; los edits de LSP (`app.rs` ~2822 y ~2859) editan `buffer` y después reconstruyen. Cada sincronización es un punto de desincronización, y `current_text()` hace `lines.join("\n")` (O(n)).

**Cómo hacerlo (en sub-pasos, un commit por sub-paso, todos los tests verdes en cada uno).**
1. **Inventario.** Listá cada lugar que escribe `doc.lines`, `tab.lines`, `doc.buffer`, `tab.buffer` (`grep -n "\.lines\b\|\.buffer\b" karkinos/src/*.rs`). Anotalo en la sección Log de este documento.
2. **Encapsular.** Hacé privados los campos `lines` y `buffer` y exponé métodos en `DocumentService`: `line(i) -> Cow<str>`, `line_count()`, `insert(char_idx, &str)`, `remove(Range<usize>)`, `set_text(&str)`, `text() -> String`. Por ahora cada método actualiza AMBAS estructuras. Arreglá todos los usos para que pasen por los métodos. Este paso no cambia comportamiento.
3. **Cambiar el backend.** Hacé que los métodos lean y escriban solo el `Rope` (`TextBuffer`) y borrá `lines`. `editor_widget.rs` hoy recibe `&mut Vec<String>` (ver funciones con `lines[..]` ~556, ~582, ~694): cambialas para operar sobre el `TextBuffer` o sobre una función de acceso por línea.
4. **Rendimiento.** `current_text()` pasa a ser `buffer.to_string()`; el render solo pide las líneas visibles (`rope.line(i)`), nunca el texto entero por frame.

**Tests.** Todos los existentes (230 en `karkinos`, 139 en core) deben seguir pasando en cada sub-paso. Agregá: `edit_lsp_y_edit_de_teclado_mantienen_el_mismo_texto` (aplicar un `LspWorkspaceEdit` y luego tipear; el texto final es el esperado).

**Si se traba:** pará después del sub-paso 2 (que ya da valor y es seguro), anotá en el Log dónde quedó y qué falló.

---

### T8 — Partir `app.rs` y `ui.rs` (S9) — después de T7

**Cómo hacerlo.** Solo mover código; cero cambios de comportamiento.
1. `karkinos/src/extensions_service.rs`: todo lo de Open VSX y `.vsix` de `app.rs` (`extensions_dir`, `search_open_vsx`, `list_installed_vsix`, `is_vsix_installed`, `uninstall_vsix`, `download_open_vsx_extension`, `poll_open_vsx`, `vsix_path`) y sus campos de estado (`open_vsx_*`) en un struct `ExtensionsService`, igual que `lsp_service.rs`/`terminal_service.rs`. `EditorApp` tiene un campo `extensions: ExtensionsService`.
2. `karkinos/src/explorer_ops.rs`: `create_explorer_item`, `delete_explorer_file`, rename/duplicate del explorador.
3. `karkinos/src/git_ops.rs`: stage/unstage/commit/diff que hoy están en `app.rs`.
4. El panel de extensiones de `ui.rs` (~3380–3740) a `karkinos/src/ui/extensions_panel.rs` (o `ui_extensions.rs` si `ui` no es un directorio).
5. Mové los tests de cada función junto con ella.

**Aceptación.** `app.rs` < 4500 líneas, `ui.rs` < 4000 líneas, tests y clippy verdes, sin cambios visibles.

## Fuera de alcance (no hacer sin pedirlo)

- Cargar/ejecutar extensiones `.vsix` de verdad.
- Traducir los comentarios del código al inglés.
- Cambiar temas, animaciones o atajos.

## Log

- **L1 (2026-11-06)** — Pedido original: "quiero que hagas un roadmap con todas las cosas que hay que hacer con el como hacerlo para que con opencode deepseek v4.1 flash lo implementes".
- **L2 (2026-11-06)** — Auditoría base: clippy limpio, 369 tests en verde. Hallazgos: guardado no atómico (`document_service.rs` ~457), `dirty=false` incondicional (`ui.rs` ~203), `current_file` asignado antes del éxito (~448), path traversal en `.vsix` (`app.rs` ~3136/3142/3166), `lock().unwrap()` en producción, doble fuente de texto `lines` + `buffer`, archivos gigantes (`app.rs` 6.7k, `ui.rs` 4.8k). El repintado ya es correcto (solo durante animaciones y `request_repaint_after` para el parpadeo): no requiere tarea.
- **L3 (2026-11-06)** — T1 hecha (S1, v0.2.64 → v0.2.65). Nuevo `crates/karkinos-core/src/atomic_write.rs` con `write_atomic` (temporal hermano `.<nombre>.karkinos-tmp`, escritura en ~20 pedazos, `on_progress` por pedazo, `sync_all`, copia de permisos del destino, `rename` con fallback `.old` y limpieza del temporal ante cualquier error). `save_recovery_file` ahora delega en `write_atomic` (misma conducta; sus tests pasan sin cambios). `DocumentService::save` usa `write_atomic` y quedó sin `File::create` sobre el destino ni `thread::sleep(15ms)`. Evidencia: RED con los 6 tests nuevos fallando (`not implemented`), GREEN 6/6; `cargo test --workspace` → 230 (karkinos) + 145 (karkinos-core) en verde; `cargo clippy --workspace --all-targets -- -D warnings` limpio. Archivos: `atomic_write.rs`, `lib.rs` (`pub mod atomic_write`), `recovery.rs`, `document_service.rs`, las tres versiones y este documento. Commit: `fix(save): write files atomically via temp file and rename`.
- **L4 (2026-11-06)** — T2 hecha (S2, S3, v0.2.65 → v0.2.66). `SaveProgress` ahora lleva `saved_text_hash`, `target` y `previous_file`; `save()` hashea el texto con `DefaultHasher` antes de escribir y guarda el `current_file` previo. Nuevo `DocumentService::current_text_hash()` (mismo hasher) y `DocumentService::finish_save() -> bool`: con error restaura `current_file = previous_file`, deja `status = "Save error: ..."` y NO toca `dirty`; con éxito recalcula `dirty = current_text_hash() != saved_text_hash` y deja `"Saved"` o `"Saved (buffer changed during save)"`; llama a `note_save_done()` y limpia `save_progress`. `ui.rs` delega el cierre del guardado en `finish_save()` y solo borra el journal si `ok`, y `close_after_save` solo cierra con `ok && !dirty` (si el buffer cambió durante el guardado, no se cierra y no se pierde la edición). Tests nuevos en `app.rs`: `guardar_y_editar_durante_el_guardado_deja_dirty`, `guardar_sin_editar_limpia_dirty`, `guardado_fallido_restaura_current_file`; el test existente `note_save_done_refresca_mtime_y_cancela_cierre_en_error` solo se actualizó para llenar los campos nuevos del struct (mismas aserciones). Evidencia: RED con los 3 tests nuevos fallando (`not implemented`), GREEN 3/3; `cargo test --workspace` → 233 (karkinos) + 147 (karkinos-core) en verde; `cargo clippy --workspace --all-targets -- -D warnings` limpio. `SaveProgress::target` queda marcado `#[allow(dead_code)]` (metadato del destino, igual que `TabDocument::id`). Archivos: `document_service.rs`, `ui.rs`, `app.rs`, las tres versiones, `Cargo.lock` y este documento. Commit: `fix(save): keep dirty flag when buffer changes during save`.
- **L5 (2026-11-06)** — T3 hecha (S4, v0.2.66 → v0.2.67). En `open_vsx.rs` nuevos `safe_vsix_file_name` (solo `[A-Za-z0-9._-]`, rechaza vacíos y `.`/`..`), `is_safe_download_url` (solo `https://` con host y sin espacios/controles) y `OpenVsxExtension::vsix_file_name()`. `download_vsix` rechaza URLs no seguras con `"Refusing unsafe download URL"`, pasa `--proto =https` y coloca `--` justo antes de la URL (con `-o` antes del separador). En `app.rs`, helper privado `vsix_path` (rechaza vacío, `.`/`..`, `/`, `\`, `..` y extensión distinta de `.vsix`) usado por `is_vsix_installed`, `download_open_vsx_extension` (`"Invalid extension name"` si falla) y `uninstall_vsix` (`"Invalid package name"` y `false` si falla). `ui.rs` arma el nombre del botón Uninstall con `ext.vsix_file_name()`. Tests nuevos: `safe_vsix_file_name` (acepta `rust-analyzer`/`0.3.2`, rechaza `../x`, `a/b`, versión `..`, vacío y espacio), `is_safe_download_url` (acepta https, rechaza http, `-K/etc/passwd`, `file://` y vacío) y `uninstall_vsix_rejects_path_traversal` (devuelve `false`, status `"Invalid package name"` y no borra el testigo fuera de `extensions_dir`). Evidencia: RED por símbolos ausentes, GREEN 5/5 en open_vsx y 1/1 en app; `cargo test --workspace` → 234 (karkinos) + 149 (karkinos-core) en verde; `cargo clippy --workspace --all-targets -- -D warnings` limpio. Archivos: `open_vsx.rs`, `app.rs`, `ui.rs`, las tres versiones, `Cargo.lock` y este documento. Commit: `fix(extensions): sanitize Open VSX file names and download URLs`.
- **L6 (2026-11-06)** — T4 hecha (S5, v0.2.67 → v0.2.68). `grep -rn "lock().unwrap()" karkinos/src crates/*/src` dejó tres casos en producción: `document_service.rs` (`finish_save`), `ui.rs` (`show_save_progress`) y `crates/karkinos-core/src/cargo.rs` (espera del proceso). Los tres pasaron a `.lock().unwrap_or_else(|e| e.into_inner())`; los de `app.rs` y el usado para envenenar en el test quedan igual porque están dentro de `#[cfg(test)] mod tests`. Test nuevo en `document_service.rs`: `finish_save_no_paniquea_con_mutex_envenenado` envenena un `Arc<Mutex<SaveProgress>>` (`done = true`) con un hilo que paniquea con el lock tomado, lo asigna a `doc.save_progress` y verifica que `finish_save()` devuelve `true` sin paniquear y limpia `save_progress`. Evidencia: RED con `called Result::unwrap() on an Err value: PoisonError` en `document_service.rs:548`, GREEN 1/1; `cargo test --workspace` → 235 (karkinos) + 149 (karkinos-core) en verde; `cargo clippy --workspace --all-targets -- -D warnings` limpio. Nota: los `RwLock::read()/write().unwrap()` de `cargo.rs` (estado/diagnósticos del runner) no entran en el patrón que pide esta tarea; quedan como deuda aparte. Archivos: `document_service.rs`, `ui.rs`, `cargo.rs`, las tres versiones, `Cargo.lock` y este documento. Commit: `fix(ui): recover from poisoned mutexes instead of panicking`.
- **L7 (2026-11-06)** — T5 hecha (S6, v0.2.68 → v0.2.69). Copy honesta en el panel de extensiones: `"● Installed"` → `"● Downloaded"`, `"📥 Install"` → `"📥 Download"` y los botones `"🗑 Uninstall"` de los `.vsix` descargados (lista de paquetes y resultados de Open VSX) → `"🗑 Delete"`; el encabezado `"Downloaded .vsix Packages"` queda igual. El botón `"🗑 Uninstall"` de `Active Plugins` no se tocó porque esos plugins sí están cargados. `uninstall_vsix` ahora reporta `"Deleted .vsix package: <archivo>"` (antes `"Uninstalled .vsix extension: ..."`) y su test existente se actualizó a ese string; también se ajustó el texto del estado vacío a `"Search Open VSX to download packages."` y los doc comments de `list_installed_vsix`/`uninstall_vsix`. No se renombraron funciones ni campos. `README.md` no menciona la instalación de extensiones, así que no cambió. Evidencia: `cargo test --workspace` → 235 (karkinos) + 149 (karkinos-core) en verde; `cargo clippy --workspace --all-targets -- -D warnings` limpio. Archivos: `ui.rs`, `app.rs`, las tres versiones y este documento. Commit: `fix(extensions): label downloaded packages as Downloaded`.
- **L8 (2026-11-06)** — T6 hecha (S7, v0.2.69 → v0.2.70). `open_vsx.rs` dejó de invocar el binario `curl`: nueva dependencia `ureq = { version = "2", default-features = false, features = ["tls"] }` (rustls + webpki-roots, sin OpenSSL del sistema). `OpenVsxClient::search` usa `ureq::get(&url).timeout(10 s).call()`, lee el body con `into_string()` y lo pasa sin cambios a `parse_search_json`. `download_vsix` conserva las validaciones de T3 (`"Refusing unsafe download URL"`), descarga con `timeout(30 s)`, lee el body con `take(MAX_DOWNLOAD_BYTES + 1)` y falla con `"Download exceeds the 104857600 byte limit"` si excede los 100 MiB, y escribe el destino con `write_atomic` (temporal hermano + `rename`, permisos y fallback de T1). Se eliminaron las llamadas a `configure_background_command` de ambas funciones (ya no hay subproceso) y las firmas `Result<_, String>` no cambiaron, así que `app.rs` quedó intacto. No se agregaron tests porque exigirían salir a la red; los tests existentes de `parse_search_json`, `safe_vsix_file_name` e `is_safe_download_url` siguen pasando. Evidencia: `cargo test --workspace` → 235 (karkinos) + 149 (karkinos-core) en verde; `cargo clippy --workspace --all-targets -- -D warnings` limpio; `cargo build --release -p karkinos` compila. Archivos: `open_vsx.rs`, `crates/karkinos-core/Cargo.toml`, `Cargo.lock`, las tres versiones y este documento. Commit: `refactor(open-vsx): replace curl subprocess with ureq`.
