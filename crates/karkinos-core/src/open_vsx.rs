//! Open VSX Registry client for community extensions and themes.
//!
//! Provides querying of https://open-vsx.org/api/-/search and downloading of .vsix packages.
//! Todo el transporte HTTP es nativo (`ureq` con rustls): no depende del
//! binario `curl` del sistema.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::atomic_write::write_atomic;

/// Tiempo máximo de espera de una búsqueda en Open VSX.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Tiempo máximo de espera de una descarga de `.vsix`.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// Tamaño máximo aceptado para un `.vsix` (100 MiB).
const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// Agente HTTP que solo habla `https`, también al seguir redirecciones:
/// una respuesta que redirija a `http://` se rechaza en vez de bajar el
/// paquete por un canal sin cifrar. `pub(crate)` para que el actualizador
/// (S1/S2) reuse el mismo agente en vez de duplicarlo.
pub(crate) fn https_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().https_only(true).build()
}

/// Metadata for an extension published on Open VSX.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenVsxExtension {
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub download_url: String,
    pub download_count: u64,
}

impl OpenVsxExtension {
    /// Formatted identifier: `namespace.name` (e.g. `rust-lang.rust-analyzer`)
    pub fn id(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// Nombre `.vsix` seguro para esta extensión, o `None` si `name`/`version`
    /// no son válidos como nombre de archivo.
    pub fn vsix_file_name(&self) -> Option<String> {
        safe_vsix_file_name(&self.name, &self.version)
    }
}

/// Search response summary from Open VSX.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpenVsxSearchResult {
    pub total: usize,
    pub extensions: Vec<OpenVsxExtension>,
}

/// Client for communicating with the Open VSX API.
pub struct OpenVsxClient;

impl OpenVsxClient {
    /// Performs an Open VSX search query (e.g. `rust`, `theme`, `cargo`).
    pub fn search(query: &str, size: usize) -> Result<OpenVsxSearchResult, String> {
        let trimmed = query.trim();
        let query_param = if trimmed.is_empty() { "rust" } else { trimmed };
        let url = format!(
            "https://open-vsx.org/api/-/search?query={}&size={}",
            urlencode(query_param),
            size.clamp(1, 50)
        );

        let body = https_agent()
            .get(&url)
            .timeout(SEARCH_TIMEOUT)
            .call()
            .map_err(|e| format!("Open VSX request failed: {}", e))?
            .into_string()
            .map_err(|e| format!("Invalid UTF-8 response: {}", e))?;

        parse_search_json(&body)
    }

    /// Downloads a `.vsix` file from `download_url` to `destination`.
    pub fn download_vsix(download_url: &str, destination: &Path) -> Result<(), String> {
        if download_url.is_empty() {
            return Err("Extension does not provide a direct download URL".to_string());
        }
        if !is_safe_download_url(download_url) {
            return Err("Refusing unsafe download URL".to_string());
        }

        let response = https_agent()
            .get(download_url)
            .timeout(DOWNLOAD_TIMEOUT)
            .call()
            .map_err(|e| format!("Download request failed: {}", e))?;

        // Leemos un byte de más que el límite para poder distinguir un
        // archivo justo en el límite de uno que lo excede.
        let mut data = Vec::new();
        response
            .into_reader()
            .take(MAX_DOWNLOAD_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("Failed to read response body: {}", e))?;
        if data.len() as u64 > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "Download exceeds the {} byte limit",
                MAX_DOWNLOAD_BYTES
            ));
        }

        if let Some(parent) = destination.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        write_atomic(destination, &data, |_| {})
            .map_err(|e| format!("Failed to write {}: {}", destination.display(), e))?;

        Ok(())
    }
}

/// Nombre de archivo `.vsix` seguro para una extensión, o `None` si
/// `name`/`version` traen caracteres fuera de `[A-Za-z0-9._-]`,
/// están vacíos o son `.`/`..`.
pub fn safe_vsix_file_name(name: &str, version: &str) -> Option<String> {
    fn valid(component: &str) -> bool {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    }

    if !valid(name) || !valid(version) {
        return None;
    }
    Some(format!("{}-{}.vsix", name, version))
}

/// `true` solo para URLs `https://` sin espacios ni caracteres de control.
pub fn is_safe_download_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() > "https://".len()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Parses the JSON payload from Open VSX search API.
pub fn parse_search_json(json_str: &str) -> Result<OpenVsxSearchResult, String> {
    let root: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let exts_val = root.get("extensions").and_then(|v| v.as_array());
    let mut extensions = Vec::new();

    if let Some(arr) = exts_val {
        for item in arr {
            let namespace = item.get("namespace").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let version = item.get("version").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let display_name = item.get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(&name)
                .to_string();
            let description = item.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let download_count = item.get("downloadCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let download_url = item.get("files")
                .and_then(|f| f.get("download"))
                .and_then(|d| d.as_str())
                .or_else(|| item.get("downloadUrl").and_then(|d| d.as_str()))
                .unwrap_or_default()
                .to_string();

            if !namespace.is_empty() && !name.is_empty() {
                extensions.push(OpenVsxExtension {
                    namespace,
                    name,
                    version,
                    display_name,
                    description,
                    download_url,
                    download_count,
                });
            }
        }
    }

    let total = root.get("totalSize")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(extensions.len());

    Ok(OpenVsxSearchResult { total, extensions })
}

/// Simple percent-encoding for query string parameters.
pub fn urlencode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencode() {
        assert_eq!(urlencode("rust analyzer"), "rust%20analyzer");
        assert_eq!(urlencode("rust-lang"), "rust-lang");
        assert_eq!(urlencode("hello/world?"), "hello%2Fworld%3F");
    }

    #[test]
    fn test_parse_search_json_valid() {
        let json_payload = r#"{
            "totalSize": 42,
            "extensions": [
                {
                    "namespace": "rust-lang",
                    "name": "rust-analyzer",
                    "version": "0.4.3101",
                    "displayName": "rust-analyzer",
                    "description": "Rust language support for Visual Studio Code",
                    "downloadCount": 1000,
                    "files": {
                        "download": "https://open-vsx.org/api/rust-lang/rust-analyzer/0.4.3101/file/rust-analyzer.vsix"
                    }
                },
                {
                    "namespace": "dustypomerleau",
                    "name": "rust-syntax",
                    "version": "0.5.2",
                    "displayName": "Rust Syntax",
                    "description": "Enhanced Rust syntax highlighting",
                    "downloadCount": 500,
                    "downloadUrl": "https://open-vsx.org/api/dustypomerleau/rust-syntax/0.5.2/file/rust-syntax.vsix"
                }
            ]
        }"#;

        let result = parse_search_json(json_payload).expect("should parse successfully");
        assert_eq!(result.total, 42);
        assert_eq!(result.extensions.len(), 2);

        let ext1 = &result.extensions[0];
        assert_eq!(ext1.id(), "rust-lang.rust-analyzer");
        assert_eq!(ext1.version, "0.4.3101");
        assert_eq!(ext1.display_name, "rust-analyzer");
        assert_eq!(ext1.description, "Rust language support for Visual Studio Code");
        assert_eq!(ext1.download_count, 1000);
        assert_eq!(ext1.download_url, "https://open-vsx.org/api/rust-lang/rust-analyzer/0.4.3101/file/rust-analyzer.vsix");

        let ext2 = &result.extensions[1];
        assert_eq!(ext2.id(), "dustypomerleau.rust-syntax");
        assert_eq!(ext2.download_url, "https://open-vsx.org/api/dustypomerleau/rust-syntax/0.5.2/file/rust-syntax.vsix");
    }

    #[test]
    fn test_safe_vsix_file_name() {
        assert_eq!(
            safe_vsix_file_name("rust-analyzer", "0.3.2").as_deref(),
            Some("rust-analyzer-0.3.2.vsix")
        );
        assert_eq!(safe_vsix_file_name("../x", "1"), None);
        assert_eq!(safe_vsix_file_name("a/b", "1"), None);
        assert_eq!(safe_vsix_file_name("x", ".."), None);
        assert_eq!(safe_vsix_file_name("", "1"), None);
        assert_eq!(safe_vsix_file_name("x", "1 2"), None);
    }

    #[test]
    fn test_is_safe_download_url() {
        assert!(is_safe_download_url("https://open-vsx.org/x.vsix"));
        assert!(!is_safe_download_url("http://open-vsx.org/x.vsix"));
        assert!(!is_safe_download_url("-K/etc/passwd"));
        assert!(!is_safe_download_url("file:///etc/passwd"));
        assert!(!is_safe_download_url(""));
    }

    #[test]
    fn test_parse_search_json_empty_or_invalid() {
        assert!(parse_search_json("not json").is_err());

        let empty_payload = r#"{"extensions": []}"#;
        let res = parse_search_json(empty_payload).expect("should parse empty array");
        assert_eq!(res.total, 0);
        assert!(res.extensions.is_empty());
    }
}
