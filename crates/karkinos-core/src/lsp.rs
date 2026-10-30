//! Language Server Protocol (LSP) client engine for Karkinos.
//! Handles JSON-RPC 2.0 framing, background process I/O, live diagnostics,
//! code completion, and hover queries without blocking UI rendering.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 0-based position in a text document (line and UTF-16/character column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LspPosition {
    pub line: usize,
    pub character: usize,
}

/// A range within a text document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// Diagnostic severity levels defined by the LSP specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

impl<'de> Deserialize<'de> for DiagnosticSeverity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = u8::deserialize(deserializer)?;
        match val {
            1 => Ok(DiagnosticSeverity::Error),
            2 => Ok(DiagnosticSeverity::Warning),
            3 => Ok(DiagnosticSeverity::Information),
            4 => Ok(DiagnosticSeverity::Hint),
            _ => Err(serde::de::Error::custom(format!("invalid severity: {}", val))),
        }
    }
}

impl Serialize for DiagnosticSeverity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

fn deserialize_optional_code<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val = Option::<Value>::deserialize(deserializer)?;
    match val {
        Some(Value::String(s)) => Ok(Some(s)),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        _ => Ok(None),
    }
}

/// A diagnostic item (error, warning, lint) published by the language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub range: LspRange,
    #[serde(default)]
    pub severity: Option<DiagnosticSeverity>,
    #[serde(default, deserialize_with = "deserialize_optional_code")]
    pub code: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub message: String,
}

/// Autocompletion suggestion returned by the language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCompletionItem {
    pub label: String,
    pub kind: Option<usize>,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: Option<String>,
}

/// Hover information returned by the language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspHover {
    pub contents: String,
    pub range: Option<LspRange>,
}

/// Formats a JSON-RPC 2.0 request into raw LSP byte payload with `Content-Length`.
pub fn format_request(id: u64, method: &str, params: Value) -> Vec<u8> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let body = msg.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Formats a JSON-RPC 2.0 notification into raw LSP byte payload with `Content-Length`.
pub fn format_notification(method: &str, params: Value) -> Vec<u8> {
    let msg = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    let body = msg.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Parses an LSP Content-Length framed packet from a buffered reader.
pub fn read_lsp_message<R: Read>(reader: &mut BufReader<R>) -> std::io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut header = String::new();
        let bytes_read = reader.read_line(&mut header)?;
        if bytes_read == 0 {
            return Ok(None);
        }

        let trimmed = header.trim_end_matches(|c| c == '\r' || c == '\n');
        if trimmed.is_empty() {
            // End of headers, body follows
            break;
        }

        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            if let Ok(len) = val.trim().parse::<usize>() {
                content_length = Some(len);
            }
        }
    }

    if let Some(len) = content_length {
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        let text = String::from_utf8(body)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(text))
    } else {
        Ok(None)
    }
}

/// State shared between UI thread and asynchronous LSP communication workers.
#[derive(Debug, Default)]
pub struct LspState {
    /// File URI -> diagnostics list
    pub diagnostics: HashMap<String, Vec<LspDiagnostic>>,
    /// Pending responses keyed by request ID
    pub responses: HashMap<u64, Value>,
    /// Latest hover result
    pub latest_hover: Option<LspHover>,
    /// Latest completion list
    pub latest_completions: Vec<LspCompletionItem>,
    /// Server initialization status
    pub is_initialized: bool,
}

/// Thread-safe client managing language server interactions.
pub struct LspClient {
    state: Arc<RwLock<LspState>>,
    next_id: AtomicU64,
    stdin_tx: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    server_process: Option<Arc<Mutex<Child>>>,
    server_name: String,
}

impl Clone for LspClient {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
            stdin_tx: self.stdin_tx.clone(),
            server_process: self.server_process.clone(),
            server_name: self.server_name.clone(),
        }
    }
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("server_name", &self.server_name)
            .field("is_active", &self.stdin_tx.is_some())
            .finish()
    }
}

impl LspClient {
    /// Spawns an LSP server process (e.g. `rust-analyzer`) and starts the background worker.
    pub fn spawn(server_cmd: &str, args: &[&str], root_path: &Path) -> Result<Self, String> {
        let mut child = Command::new(server_cmd)
            .args(args)
            .current_dir(root_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn LSP server '{}': {}", server_cmd, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture stdin of LSP server".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture stdout of LSP server".to_string())?;

        let state = Arc::new(RwLock::new(LspState::default()));
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();

        // Stdin writer thread
        thread::Builder::new()
            .name("lsp-stdin-writer".to_string())
            .spawn(move || {
                let mut writer = stdin;
                while let Ok(msg) = rx.recv() {
                    if writer.write_all(&msg).is_err() || writer.flush().is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn stdin thread: {}", e))?;

        // Stdout reader thread
        let reader_state = Arc::clone(&state);
        thread::Builder::new()
            .name("lsp-stdout-reader".to_string())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(body)) = read_lsp_message(&mut reader) {
                    Self::handle_inbound_message(&reader_state, &body);
                }
            })
            .map_err(|e| format!("Failed to spawn stdout thread: {}", e))?;

        let client = Self {
            state,
            next_id: AtomicU64::new(1),
            stdin_tx: Some(tx),
            server_process: Some(Arc::new(Mutex::new(child))),
            server_name: server_cmd.to_string(),
        };

        // Send LSP initialize handshake
        client.send_initialize(root_path);
        Ok(client)
    }

    /// Creates a mock client for headless unit testing without spawning an OS process.
    pub fn mock() -> Self {
        Self {
            state: Arc::new(RwLock::new(LspState::default())),
            next_id: AtomicU64::new(1),
            stdin_tx: None,
            server_process: None,
            server_name: "mock".to_string(),
        }
    }

    /// Sends raw JSON-RPC payload to the server.
    pub fn send_raw(&self, payload: Vec<u8>) {
        if let Some(ref tx) = self.stdin_tx {
            let _ = tx.send(payload);
        }
    }

    /// Sends the `initialize` handshake request.
    fn send_initialize(&self, root_path: &Path) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let root_uri = format!("file://{}", root_path.display());
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "synchronization": {
                        "didSave": true,
                        "dynamicRegistration": true,
                    },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true
                        }
                    },
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "publishDiagnostics": {
                        "relatedInformation": true
                    }
                }
            }
        });
        self.send_raw(format_request(id, "initialize", params));
    }

    /// Notifies the language server that a document was opened.
    pub fn did_open(&self, file_uri: &str, language_id: &str, text: &str) {
        let params = json!({
            "textDocument": {
                "uri": file_uri,
                "languageId": language_id,
                "version": 1,
                "text": text,
            }
        });
        self.send_raw(format_notification("textDocument/didOpen", params));
    }

    /// Notifies the language server that a document's full content changed.
    pub fn did_change(&self, file_uri: &str, version: i32, text: &str) {
        let params = json!({
            "textDocument": {
                "uri": file_uri,
                "version": version,
            },
            "contentChanges": [
                {
                    "text": text,
                }
            ]
        });
        self.send_raw(format_notification("textDocument/didChange", params));
    }

    /// Notifies the language server that a document was closed.
    pub fn did_close(&self, file_uri: &str) {
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            }
        });
        self.send_raw(format_notification("textDocument/didClose", params));
    }

    /// Requests code completion at a given line and column.
    pub fn request_completion(&self, file_uri: &str, line: usize, character: usize) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            },
            "position": {
                "line": line,
                "character": character,
            }
        });
        self.send_raw(format_request(id, "textDocument/completion", params));
        id
    }

    /// Requests hover tooltip information at a given line and column.
    pub fn request_hover(&self, file_uri: &str, line: usize, character: usize) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            },
            "position": {
                "line": line,
                "character": character,
            }
        });
        self.send_raw(format_request(id, "textDocument/hover", params));
        id
    }

    /// Retrieves cached diagnostics for a file URI.
    pub fn diagnostics_for_file(&self, file_uri: &str) -> Vec<LspDiagnostic> {
        self.state
            .read()
            .ok()
            .and_then(|s| s.diagnostics.get(file_uri).cloned())
            .unwrap_or_default()
    }

    /// Injects diagnostics directly (used by mock tests and notifications).
    pub fn set_diagnostics_for_file(&self, file_uri: &str, diags: Vec<LspDiagnostic>) {
        if let Ok(mut state) = self.state.write() {
            state.diagnostics.insert(file_uri.to_string(), diags);
        }
    }

    /// Retrieves latest hover information if available.
    pub fn latest_hover(&self) -> Option<LspHover> {
        self.state.read().ok().and_then(|s| s.latest_hover.clone())
    }

    /// Retrieves latest completion items if available.
    pub fn latest_completions(&self) -> Vec<LspCompletionItem> {
        self.state
            .read()
            .ok()
            .map(|s| s.latest_completions.clone())
            .unwrap_or_default()
    }

    /// Handles incoming JSON-RPC messages parsed from stdout.
    pub fn handle_inbound_message(state_lock: &Arc<RwLock<LspState>>, body: &str) {
        let Ok(val) = serde_json::from_str::<Value>(body) else {
            return;
        };

        let method = val.get("method").and_then(|m| m.as_str());

        // Notification: textDocument/publishDiagnostics
        if method == Some("textDocument/publishDiagnostics") {
            if let Some(params) = val.get("params") {
                if let Some(uri) = params.get("uri").and_then(|u| u.as_str()) {
                    if let Some(diags_val) = params.get("diagnostics").and_then(|d| d.as_array()) {
                        let parsed: Vec<LspDiagnostic> = diags_val
                            .iter()
                            .filter_map(|d| serde_json::from_value::<LspDiagnostic>(d.clone()).ok())
                            .collect();

                        if let Ok(mut state) = state_lock.write() {
                            state.diagnostics.insert(uri.to_string(), parsed);
                        }
                    }
                }
            }
            return;
        }

        // Response with ID
        if let Some(id) = val.get("id").and_then(|i| i.as_u64()) {
            if let Some(result) = val.get("result") {
                if let Ok(mut state) = state_lock.write() {
                    // Check if it's hover response
                    if let Some(contents) = result.get("contents") {
                        let text = if let Some(s) = contents.as_str() {
                            s.to_string()
                        } else if let Some(val_str) = contents.get("value").and_then(|v| v.as_str()) {
                            val_str.to_string()
                        } else {
                            contents.to_string()
                        };
                        state.latest_hover = Some(LspHover {
                            contents: text,
                            range: result
                                .get("range")
                                .and_then(|r| serde_json::from_value::<LspRange>(r.clone()).ok()),
                        });
                    }

                    // Check if it's completion response
                    let items_opt = if let Some(arr) = result.as_array() {
                        Some(arr)
                    } else {
                        result.get("items").and_then(|i| i.as_array())
                    };

                    if let Some(items) = items_opt {
                        let parsed_items: Vec<LspCompletionItem> = items
                            .iter()
                            .filter_map(|item| {
                                let label = item.get("label")?.as_str()?.to_string();
                                let kind = item.get("kind").and_then(|k| k.as_u64()).map(|k| k as usize);
                                let detail = item.get("detail").and_then(|d| d.as_str()).map(|d| d.to_string());
                                let documentation = item
                                    .get("documentation")
                                    .and_then(|d| {
                                        if let Some(s) = d.as_str() {
                                            Some(s.to_string())
                                        } else {
                                            d.get("value").and_then(|v| v.as_str()).map(|s| s.to_string())
                                        }
                                    });
                                let insert_text = item
                                    .get("insertText")
                                    .and_then(|it| it.as_str())
                                    .map(|it| it.to_string());

                                Some(LspCompletionItem {
                                    label,
                                    kind,
                                    detail,
                                    documentation,
                                    insert_text,
                                })
                            })
                            .collect();
                        state.latest_completions = parsed_items;
                    }

                    state.responses.insert(id, result.clone());
                }
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(ref proc_lock) = self.server_process {
            if let Ok(mut child) = proc_lock.lock() {
                let _ = child.kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_request_and_notification() {
        let req = format_request(42, "testMethod", json!({"key": "value"}));
        let req_str = String::from_utf8(req).unwrap();
        assert!(req_str.starts_with("Content-Length: "));
        assert!(req_str.contains("\"id\":42"));
        assert!(req_str.contains("\"method\":\"testMethod\""));

        let notif = format_notification("testNotif", json!({"status": "ok"}));
        let notif_str = String::from_utf8(notif).unwrap();
        assert!(notif_str.starts_with("Content-Length: "));
        assert!(!notif_str.contains("\"id\":"));
        assert!(notif_str.contains("\"method\":\"testNotif\""));
    }

    #[test]
    fn test_read_lsp_message_framing() {
        let payload = r#"{"jsonrpc":"2.0","method":"ping"}"#;
        let formatted = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);

        let mut reader = BufReader::new(formatted.as_bytes());
        let read = read_lsp_message(&mut reader).expect("should read message");
        assert_eq!(read, Some(payload.to_string()));
    }

    #[test]
    fn test_inbound_diagnostics_dispatch() {
        let client = LspClient::mock();
        let uri = "file:///test/main.rs";

        let sample_msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": uri,
                "diagnostics": [
                    {
                        "range": {
                            "start": { "line": 5, "character": 4 },
                            "end": { "line": 5, "character": 12 }
                        },
                        "severity": 1,
                        "code": "E0308",
                        "source": "rustc",
                        "message": "mismatched types"
                    }
                ]
            }
        });

        LspClient::handle_inbound_message(&client.state, &sample_msg.to_string());

        let diags = client.diagnostics_for_file(uri);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
        assert_eq!(diags[0].code.as_deref(), Some("E0308"));
        assert_eq!(diags[0].range.start.line, 5);
        assert_eq!(diags[0].range.start.character, 4);
    }

    #[test]
    fn test_inbound_hover_and_completion_dispatch() {
        let client = LspClient::mock();

        let hover_resp = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "result": {
                "contents": {
                    "kind": "markdown",
                    "value": "```rust\nfn calculate() -> i32\n```\nDocumentation here"
                },
                "range": {
                    "start": { "line": 2, "character": 3 },
                    "end": { "line": 2, "character": 12 }
                }
            }
        });

        LspClient::handle_inbound_message(&client.state, &hover_resp.to_string());
        let hover = client.latest_hover().expect("should have hover info");
        assert!(hover.contents.contains("fn calculate()"));

        let completion_resp = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "result": [
                {
                    "label": "compute",
                    "kind": 3,
                    "detail": "fn() -> i32",
                    "insertText": "compute()"
                }
            ]
        });

        LspClient::handle_inbound_message(&client.state, &completion_resp.to_string());
        let completions = client.latest_completions();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, "compute");
        assert_eq!(completions[0].insert_text.as_deref(), Some("compute()"));
    }
}
