//! F1 (Roadmap v0.2.27): worker de diff contra `HEAD` fuera del hilo de UI.
//!
//! La UI nunca ejecuta `git` de forma síncrona: pide el diff de un archivo y
//! el hilo de fondo corre `git show HEAD:<ruta>` y el algoritmo de Myers.
//! Si el archivo no está versionado (o no hay repo) el resultado es un diff
//! vacío, así que el gutter simplemente no dibuja marcas.

use karkinos_core::git_diff::FileDiffHunks;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

/// Pedido de diff: ruta absoluta del archivo y su contenido actual.
pub struct GitDiffRequest {
    pub path: PathBuf,
    pub text: String,
}

/// Resultado de diff listo para el gutter.
pub struct GitDiffResult {
    pub path: PathBuf,
    pub hunks: Arc<FileDiffHunks>,
}

pub struct GitDiffWorker {
    request_tx: Option<mpsc::Sender<GitDiffRequest>>,
    result_rx: Option<mpsc::Receiver<GitDiffResult>>,
    // Hilo joinable: se suelta al cerrar la app (el hilo termina cuando el
    // canal de pedidos se cierra, porque `request_tx` se cae con el worker).
    _handle: Option<thread::JoinHandle<()>>,
}

impl Default for GitDiffWorker {
    fn default() -> Self {
        Self::spawn()
    }
}

impl GitDiffWorker {
    pub fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<GitDiffRequest>();
        let (result_tx, result_rx) = mpsc::channel::<GitDiffResult>();

        let handle = thread::Builder::new()
            .name("git-diff".to_string())
            .spawn(move || {
                for request in request_rx {
                    let hunks = compute_for(&request.path, &request.text);
                    if result_tx
                        .send(GitDiffResult {
                            path: request.path,
                            hunks: Arc::new(hunks),
                        })
                        .is_err()
                    {
                        break; // la UI ya no escucha
                    }
                }
            })
            .ok();

        Self {
            request_tx: Some(request_tx),
            result_rx: Some(result_rx),
            _handle: handle,
        }
    }

    /// Encola un pedido. Nunca bloquea: si el hilo de fondo está ocupado, el
    /// pedido espera en la cola y el resultado viejo se descarta al aplicar.
    pub fn request(&self, path: PathBuf, text: String) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(GitDiffRequest { path, text });
        }
    }

    /// Devuelve el próximo resultado disponible, si hay.
    pub fn try_recv_result(&self) -> Option<GitDiffResult> {
        self.result_rx.as_ref()?.try_recv().ok()
    }
}

fn compute_for(path: &Path, current_text: &str) -> FileDiffHunks {
    match head_text_for(path) {
        Some(head) => FileDiffHunks::compute(&head, current_text),
        None => FileDiffHunks::default(),
    }
}

/// Texto del archivo en `HEAD`, o `None` si no está versionado.
pub fn head_text_for(path: &Path) -> Option<String> {
    let root = repo_root_for(path)?;
    let relative = path.strip_prefix(&root).ok()?;
    let spec = format!("HEAD:{}", relative.to_string_lossy().replace('\\', "/"));

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("show")
        .arg(&spec)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Raíz del repo que contiene a `path` (busca `.git` hacia arriba).
pub fn repo_root_for(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() {
        Some(path.to_path_buf())
    } else {
        path.parent().map(Path::to_path_buf)
    };
    while let Some(current) = dir {
        if current.join(".git").exists() {
            return Some(current);
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_walks_up_from_a_nested_file() {
        let root = std::env::temp_dir().join(format!("karkinos_diff_{}", std::process::id()));
        let nested = root.join("src").join("deep");
        std::fs::create_dir_all(&nested).expect("crear carpetas de prueba");
        std::fs::create_dir_all(root.join(".git")).expect("crear .git de prueba");
        let file = nested.join("main.rs");
        std::fs::write(&file, "fn main() {}").expect("archivo de prueba");

        let found = repo_root_for(&file);
        assert_eq!(found.as_deref(), Some(root.as_path()));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn repo_root_is_none_without_git() {        let lonely = std::env::temp_dir().join(format!(
            "karkinos_nogit_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&lonely).ok();
        assert!(repo_root_for(&lonely.join("a.rs")).is_none());
        std::fs::remove_dir_all(&lonely).ok();
    }

    /// F1: pipeline completo — repo real, `git show HEAD:<rel>` y diff
    /// calculado en el worker de fondo.
    #[test]
    fn worker_delivers_hunks_for_a_tracked_file() {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_diffpipe_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        if !run(&["init", "-q"]) {
            std::fs::remove_dir_all(&dir).ok();
            return; // sin git en el entorno no se puede probar el pipeline
        }
        run(&["config", "user.email", "test@karkinos.local"]);
        run(&["config", "user.name", "Karkinos Test"]);
        let file = dir.join("main.rs");
        std::fs::write(&file, "a\nb\nc\n").unwrap();
        if !run(&["add", "."]) || !run(&["commit", "-q", "-m", "init"]) {
            std::fs::remove_dir_all(&dir).ok();
            return;
        }

        assert_eq!(head_text_for(&file).as_deref(), Some("a\nb\nc\n"));

        let worker = GitDiffWorker::spawn();
        worker.request(file.clone(), "a\nB\nc\n".to_string());
        let mut hunks = None;
        for _ in 0..100 {
            if let Some(result) = worker.try_recv_result() {
                hunks = Some(result.hunks);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let hunks = hunks.expect("el worker tiene que responder el diff");
        assert_eq!(
            hunks.status_of(1),
            Some(karkinos_core::git_diff::DiffStatus::Modified)
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
