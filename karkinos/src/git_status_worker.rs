//! F8 (Roadmap v0.2.35): worker de `git blame` fuera del hilo de UI.
//!
//! Corre `git blame --line-porcelain` una sola vez por archivo y cachea el
//! resultado; la UI sólo consulta cuando el cursor queda quieto 300 ms.

use karkinos_core::git_status::{parse_blame_porcelain, BlameLine};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;

pub struct BlameWorker {
    request_tx: Option<mpsc::Sender<PathBuf>>,
    result_rx: Option<mpsc::Receiver<(PathBuf, Arc<HashMap<usize, BlameLine>>)>>,
    _handle: Option<thread::JoinHandle<()>>,
}

impl Default for BlameWorker {
    fn default() -> Self {
        Self::spawn()
    }
}

impl BlameWorker {
    pub fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<PathBuf>();
        let (result_tx, result_rx) = mpsc::channel::<(PathBuf, Arc<HashMap<usize, BlameLine>>)>();
        let handle = thread::Builder::new()
            .name("git-blame".to_string())
            .spawn(move || {
                for path in request_rx {
                    if let Some(lines) = blame_for(&path) {
                        if result_tx.send((path, Arc::new(lines))).is_err() {
                            break;
                        }
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

    pub fn request(&self, path: PathBuf) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(path);
        }
    }

    pub fn try_recv(&self) -> Option<(PathBuf, Arc<HashMap<usize, BlameLine>>)> {
        self.result_rx.as_ref()?.try_recv().ok()
    }
}

fn blame_for(path: &Path) -> Option<HashMap<usize, BlameLine>> {
    let root = crate::git_diff_worker::repo_root_for(path)?;
    let relative = path.strip_prefix(&root).ok()?;
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("blame")
        .arg("--line-porcelain")
        .arg("--")
        .arg(relative)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_blame_porcelain(&String::from_utf8_lossy(&output.stdout)))
}
