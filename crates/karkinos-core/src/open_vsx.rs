//! Open VSX Registry client for community extensions and themes.
//!
//! Provides querying of https://open-vsx.org/api/-/search and downloading of .vsix packages.

use serde::{Deserialize, Serialize};
use std::path::Path;

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

        let output = std::process::Command::new("curl")
            .arg("-s")
            .arg("-L")
            .arg("--max-time")
            .arg("10")
            .arg(&url)
            .output()
            .map_err(|e| format!("Failed to execute curl: {}", e))?;

        if !output.status.success() {
            return Err(format!("curl request failed with exit code {:?}", output.status.code()));
        }

        let body = String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid UTF-8 response: {}", e))?;

        parse_search_json(&body)
    }

    /// Downloads a `.vsix` file from `download_url` to `destination`.
    pub fn download_vsix(download_url: &str, destination: &Path) -> Result<(), String> {
        if download_url.is_empty() {
            return Err("Extension does not provide a direct download URL".to_string());
        }

        if let Some(parent) = destination.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let output = std::process::Command::new("curl")
            .arg("-s")
            .arg("-L")
            .arg("--max-time")
            .arg("30")
            .arg(download_url)
            .arg("-o")
            .arg(destination)
            .output()
            .map_err(|e| format!("Failed to execute curl: {}", e))?;

        if !output.status.success() {
            return Err(format!("curl download failed with exit code {:?}", output.status.code()));
        }

        if !destination.exists() {
            return Err("Destination file was not created".to_string());
        }

        Ok(())
    }
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
    fn test_parse_search_json_empty_or_invalid() {
        assert!(parse_search_json("not json").is_err());

        let empty_payload = r#"{"extensions": []}"#;
        let res = parse_search_json(empty_payload).expect("should parse empty array");
        assert_eq!(res.total, 0);
        assert!(res.extensions.is_empty());
    }
}
