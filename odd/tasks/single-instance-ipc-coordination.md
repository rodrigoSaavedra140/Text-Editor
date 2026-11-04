# Roadmap v0.2.48: Single-Instance IPC & Terminal Coordination

## Contexto & Objetivos
Actualmente, si Karkinos ya está en ejecución y el usuario ejecuta desde una terminal (o desde el dock terminal PTY integrado) `karkinos archivo.rs` o `karkinos carpeta/`, se abre un proceso secundario completo con una nueva ventana gráfica. Esto compite por recursos de GPU/CPU y fragmenta la experiencia de edición.

El estándar en editores industriales modernos (VS Code, Zed, Cursor, Sublime) es que una segunda invocación detecte la ventana abierta, le transfiera la orden de abrir el archivo o cambiar de workspace, solicite foco al gestor de ventanas y termine instantáneamente.

### Metas
1. **Detección y entrega de cliente en CLI (< 5 ms)**:
   - Al arrancar `main()`, comprobar si existe un socket IPC activo (`$XDG_RUNTIME_DIR/karkinos.sock` o `/tmp/karkinos-$UID.sock`).
   - Si la instancia primaria responde, enviar comando de apertura (`OPEN <path>`), esperar confirmación y salir inmediatamente.
2. **Servidor IPC en Instancia Primaria**:
   - `UnixListener` en un hilo secundario ligero que escucha pedidos entrantes sin bloquear el loop de UI.
   - Envío de comandos mediante canal `std::sync::mpsc` hacia `EditorApp`.
3. **Manejo de foco y pestañas en `EditorApp`**:
   - Al recibir `IpcCommand::Open(path)`, `EditorApp` abre la pestaña o workspace mediante `init_from_cli_path` y solicita foco del sistema operativo (`ViewportCommand::Focus`).
4. **Limpieza automática de sockets huérfanos**:
   - Si la app crasheó previamente y quedó el archivo `.sock`, detectar que la conexión falla y remover el archivo stale antes de crear el listener.
   - Remover el socket al cerrar la aplicación.

---

## Tarea A: Protocolo y Módulo IPC en `crates/karkinos-core`

### Archivos Afectados
- `crates/karkinos-core/src/ipc.rs` (nuevo módulo)
- `crates/karkinos-core/src/lib.rs`

### Especificación Técnica
1. Crear `crates/karkinos-core/src/ipc.rs`:
   - En Linux/macOS:
     - Ruta de socket:
       ```rust
       pub fn socket_path() -> std::path::PathBuf {
           if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
               std::path::PathBuf::from(runtime_dir).join("karkinos.sock")
           } else {
               let uid = unsafe { libc::getuid() };
               std::path::PathBuf::from(format!("/tmp/karkinos-{uid}.sock"))
           }
       }
       ```
     - Enviar mensaje como cliente:
       ```rust
       pub fn try_send_to_primary(path: &std::path::Path) -> Result<bool, String>
       ```
       - Intenta `UnixStream::connect(socket_path())`.
       - Si conecta, escribe `OPEN <canonical_path>\n`.
       - Lee respuesta (`OK\n`). Si responde OK, retorna `Ok(true)`.
       - Si la conexión falla (econrefused / not found), retorna `Ok(false)` (no hay primaria o socket stale).
     - Servidor IPC:
       ```rust
       pub enum IpcCommand {
           Open(std::path::PathBuf),
       }
       pub struct IpcServer {
           pub receiver: std::sync::mpsc::Receiver<IpcCommand>,
           socket_path: std::path::PathBuf,
       }
       impl IpcServer {
           pub fn spawn() -> Result<Self, String>
       }
       impl Drop for IpcServer {
           fn drop(&mut self) { let _ = std::fs::remove_file(&self.socket_path); }
       }
       ```
2. Tests unitarios en `ipc.rs`:
   - Envío y recepción de comando entre cliente y servidor en socket temporal.
   - Socket huérfano es detectado y reemplazado limpiamente.

---

## Tarea B: Integración en `main.rs` y `EditorApp`

### Archivos Afectados
- `karkinos/src/main.rs`
- `karkinos/src/app.rs`
- `karkinos/src/ui.rs`

### Especificación Técnica
1. En `karkinos/src/main.rs`:
   - Tras parsear `target_path`:
     - Si hay un `target_path` o llamada estándar, intentar `karkinos_core::ipc::try_send_to_primary(&path)`.
     - Si retorna `Ok(true)`:
       `println!("Karkinos: opened in existing window");`
       `return Ok(());`
     - Si retorna `Ok(false)`:
       Iniciar `IpcServer::spawn()` y almacenar el receiver en `EditorApp`.
2. En `karkinos/src/app.rs`:
   - Campo `pub ipc_server: Option<karkinos_core::ipc::IpcServer>` en `EditorApp`.
   - Método `pub fn poll_ipc_events(&mut self, ctx: &egui::Context)`:
     - Drena los comandos recibidos en el canal.
     - Para cada `IpcCommand::Open(path)`:
       - Invoca `self.init_from_cli_path(&path)`.
       - Solicita foco de ventana:
         `ctx.send_viewport_cmd(egui::ViewportCommand::Focus);`
         `ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical));`
         `ctx.request_repaint();`
3. En `karkinos/src/ui.rs`:
   - En `update()`, invocar `self.poll_ipc_events(ctx)`.
4. Tests unitarios:
   - `poll_ipc_events` procesa comandos entrantes de apertura de archivo.

---

## Tarea C: Versión v0.2.48 y Validación

### Archivos Afectados
- `karkinos/Cargo.toml` (bump a `0.2.48`)
- `crates/karkinos-core/Cargo.toml` (bump a `0.2.48`)
- `scripts/PKGBUILD`
- `docs/index.html`

### Especificación Técnica
1. Sincronizar versión `0.2.48` en Cargo.toml, PKGBUILD y docs.
2. `cargo check --workspace` y `cargo test --workspace` al 100% verdes.
3. `cargo clippy --workspace --all-targets` sin warnings.

---

## Log

- **L1** (v0.2.48): "Lee odd/tasks/single-instance-ipc-coordination.md e implementa las Tareas A, B y C" — protocolo IPC de instancia única, integración en la instancia primaria y bump de versión, con validación al 100%.
- **L2** — Tarea A completa: `crates/karkinos-core/src/ipc.rs` (`socket_path`, `try_send_to_primary`, `IpcCommand`, `IpcServer` con hilo de accept + `mpsc`, limpieza de socket stale y `Drop`); stubs no-op en `cfg(not(unix))`; registro en `lib.rs`; dependencia `libc` (unix); 4 tests unitarios.
- **L3** — Tarea B completa: coordinación en `main.rs` (reenvío `OPEN` + salida anticipada, `IpcServer::spawn()`); campo `ipc_server` y método `poll_ipc_events` en `app.rs`; llamada en `update()` de `ui.rs`; 2 tests unitarios.
- **L4** — Tarea C completa: `0.2.48` en `karkinos/Cargo.toml`, `crates/karkinos-core/Cargo.toml`, `scripts/PKGBUILD` y `docs/index.html`; `Cargo.lock` sincronizado.
- **L5** — Validación: `cargo check --workspace` 0 warnings; `cargo test --workspace` 303 tests verdes (185 bin + 118 core, 0 fallos); `cargo clippy --workspace --all-targets` 0 warnings.
