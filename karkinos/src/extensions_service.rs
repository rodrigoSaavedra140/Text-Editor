//! S9 (Roadmap v0.2.72): servicio de extensiones (Open VSX y `.vsix`).
//!
//! `ExtensionsService` posee el estado de la pestaña Open VSX (búsqueda,
//! resultados, errores y canales de fondo) y las operaciones sobre los
//! paquetes `.vsix` descargados. `app.rs` conserva solo la orquestación
//! que toca pestañas y presentación, igual que `lsp_service.rs` y
//! `terminal_service.rs`.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use karkinos_core::open_vsx::{OpenVsxExtension, OpenVsxSearchResult};

use crate::document_service::DocumentService;

/// S3 (Rust IDE): estado y operaciones del catálogo Open VSX.
pub struct ExtensionsService {
    pub open_vsx_query_input: String,
    pub open_vsx_results: Vec<OpenVsxExtension>,
    pub open_vsx_total: usize,
    pub open_vsx_searching: bool,
    pub open_vsx_error: Option<String>,
    pub open_vsx_status: Option<String>,
    /// Identificador (`namespace.name`) de la extensión que se está descargando.
    /// La UI lo usa para mostrar el estado "Downloading" en su tarjeta.
    pub downloading: Option<String>,
    open_vsx_rx: Option<Receiver<Result<OpenVsxSearchResult, String>>>,
    open_vsx_download_rx: Option<Receiver<Result<String, String>>>,
}

impl Default for ExtensionsService {
    fn default() -> Self {
        Self {
            open_vsx_query_input: "rust".to_string(),
            open_vsx_results: Vec::new(),
            open_vsx_total: 0,
            open_vsx_searching: false,
            open_vsx_error: None,
            open_vsx_status: None,
            downloading: None,
            open_vsx_rx: None,
            open_vsx_download_rx: None,
        }
    }
}

impl ExtensionsService {
    /// S3 (Rust IDE): dispara una búsqueda asincrónica en Open VSX.
    pub fn search_open_vsx(&mut self, query: &str) {
        let q = query.trim().to_string();
        self.open_vsx_query_input = q.clone();
        self.open_vsx_searching = true;
        self.open_vsx_error = None;
        self.open_vsx_status = Some("Searching Open VSX...".to_string());

        let (tx, rx) = std::sync::mpsc::channel();
        self.open_vsx_rx = Some(rx);

        std::thread::spawn(move || {
            let res = karkinos_core::open_vsx::OpenVsxClient::search(&q, 25);
            let _ = tx.send(res);
        });
    }

    /// Returns the directory where extensions and downloaded .vsix files are stored.
    pub fn extensions_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(".karkinos")
            .join("extensions")
    }

    /// Resuelve la ruta de un `.vsix` dentro de `extensions_dir`, rechazando
    /// nombres con separadores, `..`, vacíos o sin extensión `.vsix`.
    fn vsix_path(file_name: &str) -> Option<PathBuf> {
        if file_name.is_empty()
            || file_name == "."
            || file_name == ".."
            || file_name.contains('/')
            || file_name.contains('\\')
            || file_name.contains("..")
            || std::path::Path::new(file_name)
                .extension()
                .and_then(|ext| ext.to_str())
                != Some("vsix")
        {
            return None;
        }
        Some(Self::extensions_dir().join(file_name))
    }

    /// Lists downloaded .vsix packages on disk. Returns list of (filename, file_size_bytes).
    pub fn list_installed_vsix() -> Vec<(String, u64)> {
        let dir = Self::extensions_dir();
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("vsix") {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    list.push((name, size));
                }
            }
        }
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }

    /// Checks if a .vsix package for an extension exists on disk.
    pub fn is_vsix_installed(&self, ext: &OpenVsxExtension) -> bool {
        ext.vsix_file_name()
            .and_then(|file_name| Self::vsix_path(&file_name))
            .map(|path| path.exists())
            .unwrap_or(false)
    }

    /// Deletes a downloaded .vsix package by filename.
    pub fn uninstall_vsix(&mut self, doc: &mut DocumentService, filename: &str) -> bool {
        let Some(path) = Self::vsix_path(filename) else {
            doc.status = "Invalid package name".to_string();
            return false;
        };
        if path.exists() {
            if std::fs::remove_file(&path).is_ok() {
                doc.status = format!("Deleted .vsix package: {}", filename);
                true
            } else {
                doc.status = format!("Failed to delete: {}", filename);
                false
            }
        } else {
            false
        }
    }

    /// S3 (Rust IDE): descarga un paquete `.vsix` de forma asincrónica.
    pub fn download_open_vsx_extension(&mut self, ext: &OpenVsxExtension) {
        let url = ext.download_url.clone();
        let (file_name, dest) = match ext
            .vsix_file_name()
            .and_then(|name| Self::vsix_path(&name).map(|path| (name, path)))
        {
            Some(target) => target,
            None => {
                self.open_vsx_status = Some("Invalid extension name".to_string());
                return;
            }
        };
        let id_name = ext.id();

        self.downloading = Some(id_name.clone());
        self.open_vsx_status = Some(format!("Downloading {}...", file_name));

        let (tx, rx) = std::sync::mpsc::channel();
        self.open_vsx_download_rx = Some(rx);

        std::thread::spawn(move || {
            let res = karkinos_core::open_vsx::OpenVsxClient::download_vsix(&url, &dest);
            let msg = match res {
                Ok(_) => Ok(format!("Successfully downloaded {} to {}", id_name, dest.display())),
                Err(e) => Err(format!("Download failed: {}", e)),
            };
            let _ = tx.send(msg);
        });
    }

    /// S3 (Rust IDE): consume las tareas de fondo de Open VSX pendientes.
    pub fn poll_open_vsx(&mut self, doc: &mut DocumentService) {
        if let Some(ref rx) = self.open_vsx_rx {
            if let Ok(result) = rx.try_recv() {
                self.open_vsx_searching = false;
                match result {
                    Ok(data) => {
                        self.open_vsx_total = data.total;
                        self.open_vsx_results = data.extensions;
                        self.open_vsx_status = Some(format!("Found {} extensions", self.open_vsx_total));
                    }
                    Err(err) => {
                        self.open_vsx_error = Some(err);
                        self.open_vsx_status = None;
                    }
                }
                self.open_vsx_rx = None;
            }
        }

        if let Some(ref rx) = self.open_vsx_download_rx {
            if let Ok(result) = rx.try_recv() {
                // La descarga terminó (con éxito o error): la tarjeta deja de
                // mostrar "Downloading" y vuelve a su estado normal.
                self.downloading = None;
                match result {
                    Ok(msg) => {
                        self.open_vsx_status = Some(msg.clone());
                        doc.status = msg;
                    }
                    Err(err) => {
                        self.open_vsx_error = Some(err.clone());
                        doc.status = err;
                    }
                }
                self.open_vsx_download_rx = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::EditorApp;
    #[test]
    fn test_editor_app_open_vsx_polling() {
        use karkinos_core::open_vsx::{OpenVsxExtension, OpenVsxSearchResult};

        let mut app = EditorApp::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.extensions.open_vsx_rx = Some(rx);
        app.extensions.open_vsx_searching = true;

        tx.send(Ok(OpenVsxSearchResult {
            total: 1,
            extensions: vec![OpenVsxExtension {
                namespace: "rust-lang".to_string(),
                name: "rust-analyzer".to_string(),
                version: "0.4.3101".to_string(),
                display_name: "rust-analyzer".to_string(),
                description: "Rust language support".to_string(),
                download_url: "https://example.com/rust-analyzer.vsix".to_string(),
                download_count: 5000,
            }],
        })).unwrap();

        app.extensions.poll_open_vsx(&mut app.doc);
        assert!(!app.extensions.open_vsx_searching);
        assert_eq!(app.extensions.open_vsx_total, 1);
        assert_eq!(app.extensions.open_vsx_results.len(), 1);
        assert_eq!(app.extensions.open_vsx_results[0].id(), "rust-lang.rust-analyzer");
        assert!(app.extensions.open_vsx_status.as_ref().unwrap().contains("Found 1 extensions"));
    }

    #[test]
    fn test_editor_app_uninstall_extensions_and_vsix() {
        let mut app = EditorApp::default();
        assert_eq!(app.plugin_manager.len(), 3);

        // Test uninstall plugin
        let ok = app.uninstall_plugin("Case Transformer");
        assert!(ok);
        assert_eq!(app.plugin_manager.len(), 2);
        assert!(app.doc.status.contains("Uninstalled plugin: Case Transformer"));

        // Test uninstall vsix file
        let ext_dir = ExtensionsService::extensions_dir();
        let _ = std::fs::create_dir_all(&ext_dir);
        let test_vsix = ext_dir.join("test-ext-1.0.0.vsix");
        std::fs::write(&test_vsix, "fake vsix content").unwrap();

        let list = ExtensionsService::list_installed_vsix();
        assert!(list.iter().any(|(name, _)| name == "test-ext-1.0.0.vsix"));

        let uninstalled = app.extensions.uninstall_vsix(&mut app.doc, "test-ext-1.0.0.vsix");
        assert!(uninstalled);
        assert!(!test_vsix.exists());
        assert!(app.doc.status.contains("Deleted .vsix package: test-ext-1.0.0.vsix"));
    }

    #[test]
    fn downloading_se_limpia_al_terminar() {
        let mut app = EditorApp::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.extensions.open_vsx_download_rx = Some(rx);
        app.extensions.downloading = Some("rust-lang.rust-analyzer".to_string());

        tx.send(Ok("Successfully downloaded rust-lang.rust-analyzer".to_string()))
            .unwrap();
        app.extensions.poll_open_vsx(&mut app.doc);

        assert_eq!(app.extensions.downloading, None);
    }

    #[test]
    fn uninstall_vsix_rejects_path_traversal() {
        let mut app = EditorApp::default();
        // Ruta a la que resolvería `extensions_dir().join("../../algo")` sin validar.
        let outside = ExtensionsService::extensions_dir()
            .join("..")
            .join("..")
            .join("algo");
        let created = !outside.exists();
        if created {
            std::fs::write(&outside, b"testigo").unwrap();
        }

        let uninstalled = app.extensions.uninstall_vsix(&mut app.doc, "../../algo");

        assert!(!uninstalled);
        assert_eq!(app.doc.status, "Invalid package name");
        assert!(outside.exists(), "el archivo testigo no debe borrarse");

        if created {
            let _ = std::fs::remove_file(&outside);
        }
    }


}
