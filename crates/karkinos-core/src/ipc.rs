//! Single-instance coordination over a local Unix socket.
//!
//! A second invocation of Karkinos detects the running primary instance by
//! connecting to a well-known socket (`$XDG_RUNTIME_DIR/karkinos.sock` or
//! `/tmp/karkinos-<uid>.sock`), forwards an `OPEN <path>` command, waits for an
//! `OK` acknowledgement and exits immediately. The primary instance runs an
//! [`IpcServer`] that accepts connections in a lightweight background thread
//! and delivers parsed commands to the UI through an `mpsc` channel.
//!
//! A special `PING` command exists only for liveness probing: the server
//! acknowledges it without enqueuing any [`IpcCommand`] and without waking the
//! UI, so the post-update restart can ask "is the old instance still holding
//! the socket?" without stealing its focus ([`ping_primary`]).
//!
//! On non-Unix platforms every entry point degrades to a safe no-op that
//! reports "no primary instance" (`Ok(false)`).

use std::path::PathBuf;

/// Command forwarded from a secondary Karkinos process to the primary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcCommand {
    /// Open a file or folder in the running editor.
    Open(PathBuf),
    /// Focus the running editor window.
    Focus,
}

/// Resolve the well-known IPC socket path for the current user.
///
/// On Unix this is `$XDG_RUNTIME_DIR/karkinos.sock` when the environment
/// variable is set, falling back to `/tmp/karkinos-<uid>.sock` otherwise. The
/// non-Unix fallback returns an empty path (never used for a real socket).
#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("karkinos.sock")
    } else {
        // SAFETY: `getuid` is a read-only libc call with no preconditions.
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/karkinos-{uid}.sock"))
    }
}

#[cfg(not(unix))]
pub fn socket_path() -> PathBuf {
    PathBuf::new()
}

/// Try to forward `path` (or focus command if None) to an already-running primary instance.
///
/// Returns `Ok(true)` when the primary acknowledged the command with `OK`,
/// `Ok(false)` when no live primary is listening (no socket, refused
/// connection, or no `OK` reply), and `Err` only for an I/O failure while
/// talking to a socket that *did* connect.
#[cfg(unix)]
pub fn try_send_to_primary(target: Option<&std::path::Path>) -> Result<bool, String> {
    send_to_socket(&socket_path(), target)
}

#[cfg(not(unix))]
pub fn try_send_to_primary(_target: Option<&std::path::Path>) -> Result<bool, String> {
    Ok(false)
}

/// Sondea si hay una instancia primaria viva en el socket de siempre.
///
/// T4b (S2): a diferencia de `try_send_to_primary(None)`, este sondeo NO
/// genera ningún [`IpcCommand`] en la primaria: no encola nada, no despierta
/// su UI y no le pide foco. Es lo que necesita el reinicio tras una
/// actualización, que pregunta "¿ya soltaste el socket?" en un bucle.
#[cfg(unix)]
pub fn ping_primary() -> bool {
    ping_socket(&socket_path())
}

#[cfg(not(unix))]
pub fn ping_primary() -> bool {
    false
}

#[cfg(unix)]
type WakerCallback = Box<dyn Fn() + Send + Sync>;
#[cfg(unix)]
type SharedWaker = std::sync::Arc<std::sync::Mutex<Option<WakerCallback>>>;

/// Listening endpoint of the primary instance.
///
/// Holds the `mpsc` receiver the UI drains every frame and owns the socket
/// file: dropping the server removes it so the next launch starts clean.
pub struct IpcServer {
    /// Receives [`IpcCommand`]s as secondary processes connect.
    pub receiver: std::sync::mpsc::Receiver<IpcCommand>,
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(unix)]
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(unix)]
    waker: SharedWaker,
}

impl IpcServer {
    /// Bind the well-known socket and spawn the accept loop.
    ///
    /// Stale socket files left behind by a crashed instance are removed
    /// before binding. Returns `Err` if another live primary already owns the
    /// socket.
    #[cfg(unix)]
    pub fn spawn() -> Result<Self, String> {
        spawn_on(socket_path())
    }

    #[cfg(not(unix))]
    pub fn spawn() -> Result<Self, String> {
        let (_tx, receiver) = std::sync::mpsc::channel();
        Ok(IpcServer { receiver })
    }

    /// Sets a callback invoked immediately when an IPC command is received.
    /// Used by the UI thread to request an instant repaint even when idle/unfocused.
    pub fn set_waker<F>(&self, waker: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        #[cfg(unix)]
        if let Ok(mut guard) = self.waker.lock() {
            *guard = Some(Box::new(waker));
        }
    }

    /// Build a server around an already-created receiver.
    ///
    /// Used by tests and embedders that drive the command stream themselves.
    #[doc(hidden)]
    pub fn from_receiver(receiver: std::sync::mpsc::Receiver<IpcCommand>) -> Self {
        IpcServer {
            receiver,
            #[cfg(unix)]
            socket_path: PathBuf::new(),
            #[cfg(unix)]
            shutdown: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(unix)]
            waker: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

#[cfg(unix)]
impl Drop for IpcServer {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(unix)]
mod unix_impl {
    use super::IpcCommand;
    use super::IpcServer;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};

    /// T4b: sondeo de vida contra un socket explícito.
    ///
    /// Manda `PING`, que el servidor contesta `OK` sin encolar ningún
    /// [`IpcCommand`] ni llamar al waker (ver `handle_connection`). `true`
    /// solo si la primaria contestó: un socket huérfano, un connect
    /// rechazado o un `OK` que no llega cuentan como "no hay primaria",
    /// que es justo lo que el bucle de espera quiere saber.
    pub fn ping_socket(socket: &Path) -> bool {
        let Ok(mut stream) = UnixStream::connect(socket) else {
            return false;
        };
        // El sondeo se repite cada 25 ms: un tope corto evita que una
        // primaria medio muerta estanque el bucle de espera.
        let timeout = Some(std::time::Duration::from_millis(500));
        if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
            return false;
        }
        if writeln!(stream, "PING").is_err() || stream.flush().is_err() {
            return false;
        }
        let mut response = String::new();
        matches!(BufReader::new(&stream).read_line(&mut response), Ok(n) if n > 0)
            && response.trim() == "OK"
    }

    /// Core client logic against an explicit socket path (testable without
    /// touching the process-wide well-known path).
    pub fn send_to_socket(socket: &Path, target: Option<&Path>) -> Result<bool, String> {
        let mut stream = match UnixStream::connect(socket) {
            Ok(stream) => stream,
            Err(_) => return Ok(false),
        };
        // Bound the round-trip so a half-alive primary can't stall the CLI.
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;

        if let Some(target) = target {
            let abs_path = if target.is_absolute() {
                target.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(target))
                    .unwrap_or_else(|_| target.to_path_buf())
            };
            let canonical = std::fs::canonicalize(&abs_path).unwrap_or(abs_path);
            writeln!(stream, "OPEN {}", canonical.display()).map_err(|e| e.to_string())?;
        } else {
            writeln!(stream, "FOCUS").map_err(|e| e.to_string())?;
        }
        stream.flush().map_err(|e| e.to_string())?;

        let mut reader = BufReader::new(&stream);
        let mut response = String::new();
        match reader.read_line(&mut response) {
            Ok(0) => Ok(false),
            Ok(_) => Ok(response.trim() == "OK"),
            Err(_) => Ok(false),
        }
    }

    /// Bind `socket_path` and spawn the accept loop, removing any stale file
    /// first. Returns an `Err` if a live primary already owns the socket.
    pub fn spawn_on(socket_path: PathBuf) -> Result<IpcServer, String> {
        if socket_path.exists() {
            match UnixStream::connect(&socket_path) {
                Ok(_) => {
                    return Err("another Karkinos instance is already running".to_string());
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&socket_path);
                }
            }
        }

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("failed to bind IPC socket {}: {e}", socket_path.display()))?;

        let (tx, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_socket = socket_path.clone();
        let waker: super::SharedWaker = Arc::new(std::sync::Mutex::new(None));
        let thread_waker = Arc::clone(&waker);

        std::thread::Builder::new()
            .name("karkinos-ipc".to_string())
            .spawn(move || accept_loop(listener, tx, thread_shutdown, thread_socket, thread_waker))
            .map_err(|e| e.to_string())?;

        Ok(IpcServer {
            receiver,
            socket_path,
            shutdown,
            waker,
        })
    }

    fn accept_loop(
        listener: UnixListener,
        tx: mpsc::Sender<IpcCommand>,
        shutdown: Arc<AtomicBool>,
        socket_path: PathBuf,
        waker: super::SharedWaker,
    ) {
        // Non-blocking accept lets the loop poll the shutdown flag and exit
        // promptly when the server drops.
        listener.set_nonblocking(true).ok();
        while !shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _addr)) => handle_connection(stream, &tx, &waker),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        // Best-effort final cleanup in case the socket outlived `Drop`.
        let _ = std::fs::remove_file(&socket_path);
    }

    fn handle_connection(
        mut stream: UnixStream,
        tx: &mpsc::Sender<IpcCommand>,
        waker: &super::SharedWaker,
    ) {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
        let mut line = String::new();
        let mut success = false;
        // T4b: `PING` contesta OK pero no encola ni despierta la UI, así que
        // el ack y el waker se deciden por separado.
        let mut wake_ui = false;
        {
            let mut reader = BufReader::new(&stream);
            if reader.read_line(&mut line).is_ok() {
                let trimmed = line.trim_end();
                if let Some(rest) = trimmed.strip_prefix("OPEN ") {
                    let path = rest.trim();
                    if !path.is_empty() {
                        let _ = tx.send(IpcCommand::Open(PathBuf::from(path)));
                        success = true;
                        wake_ui = true;
                    }
                } else if trimmed == "FOCUS" {
                    let _ = tx.send(IpcCommand::Focus);
                    success = true;
                    wake_ui = true;
                } else if trimmed == "PING" {
                    // Sondeo de vida: solo el ack, nada para la UI.
                    success = true;
                }
            }
        }
        if success {
            if wake_ui {
                if let Ok(guard) = waker.lock() {
                    if let Some(ref wake) = *guard {
                        wake();
                    }
                }
            }
            let _ = stream.write_all(b"OK\n");
        } else {
            let _ = stream.write_all(b"ERR\n");
        }
        let _ = stream.flush();
    }
}

#[cfg(unix)]
use unix_impl::ping_socket;

#[cfg(unix)]
use unix_impl::send_to_socket;

#[cfg(unix)]
use unix_impl::spawn_on;

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique per-test scratch directory (socket + payload files).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_ipc_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("crear carpeta temporal");
        dir
    }

    #[cfg(unix)]
    #[test]
    fn client_sends_open_and_server_delivers_command() {
        let dir = temp_dir("roundtrip");
        let socket = dir.join("karkinos.sock");
        let server = spawn_on(socket.clone()).expect("spawn server on temp socket");

        let target = dir.join("archivo.rs");
        std::fs::write(&target, "fn main() {}").expect("crear archivo");

        let sent = send_to_socket(&socket, Some(&target)).expect("client send");
        assert!(sent, "the server must answer OK");

        let cmd = server
            .receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("command delivered over mpsc");
        assert_eq!(
            cmd,
            IpcCommand::Open(std::fs::canonicalize(&target).expect("canonical path"))
        );

        // Also test FOCUS command
        let sent_focus = send_to_socket(&socket, None).expect("client send focus");
        assert!(sent_focus, "the server must answer OK to FOCUS");
        let cmd_focus = server
            .receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("focus command delivered over mpsc");
        assert_eq!(cmd_focus, IpcCommand::Focus);

        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_is_removed_and_replaced() {
        let dir = temp_dir("stale");
        let socket = dir.join("karkinos.sock");
        // A leftover file with no listener behind it: a crashed instance.
        std::fs::write(&socket, b"stale").expect("escribir socket huérfano");

        let server = spawn_on(socket.clone()).expect("spawn over stale socket");
        assert!(socket.exists(), "the listener re-created the socket");
        // And it actually answers, proving the stale file was replaced.
        let target = dir.join("x.txt");
        std::fs::write(&target, "hola").expect("crear archivo");
        assert!(send_to_socket(&socket, Some(&target)).expect("send to fresh socket"));

        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn client_returns_false_without_a_server() {
        let dir = temp_dir("none");
        let socket = dir.join("missing.sock");
        let result =
            send_to_socket(&socket, Some(std::path::Path::new("/no/such/file"))).expect("send");
        assert!(!result, "no server must yield Ok(false)");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// T4b (S2): el sondeo de vida que usa el reinicio tras actualizar no
    /// puede dejar NADA en la instancia primaria: ni un `IpcCommand` en el
    /// canal, ni un despertar de la UI (que le robaba el foco ~40 veces por
    /// segundo a la ventana que se estaba cerrando).
    #[cfg(unix)]
    #[test]
    fn probe_no_encola_comandos_ni_despierta_la_ui() {
        let dir = temp_dir("probe");
        let socket = dir.join("karkinos.sock");
        let server = spawn_on(socket.clone()).expect("spawn server on temp socket");

        let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let counter = std::sync::Arc::clone(&wakes);
            server.set_waker(move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        }

        for _ in 0..5 {
            assert!(
                ping_socket(&socket),
                "una primaria viva contesta OK al sondeo"
            );
        }

        assert!(
            matches!(
                server.receiver.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ),
            "el sondeo no puede dejar comandos pendientes en la primaria"
        );
        assert_eq!(
            wakes.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "el sondeo no debe despertar la UI de la primaria"
        );

        // El reenvío normal de una segunda instancia NO cambia.
        assert!(send_to_socket(&socket, None).expect("client send focus"));
        let cmd = server
            .receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("focus command delivered over mpsc");
        assert_eq!(cmd, IpcCommand::Focus);
        assert_eq!(
            wakes.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "el FOCUS de una segunda instancia sí despierta la UI"
        );

        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Sin primaria escuchando el sondeo dice que no, en vez de colgarse.
    #[cfg(unix)]
    #[test]
    fn probe_sin_servidor_es_falso() {
        let dir = temp_dir("probe_none");
        assert!(!ping_socket(&dir.join("missing.sock")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn drop_removes_the_socket_file() {
        let dir = temp_dir("drop");
        let socket = dir.join("karkinos.sock");
        {
            let server = spawn_on(socket.clone()).expect("spawn");
            assert!(socket.exists());
            drop(server);
        }
        // The accept loop shuts down within a few poll cycles.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while socket.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!socket.exists(), "socket removed on drop");
        std::fs::remove_dir_all(&dir).ok();
    }
}
