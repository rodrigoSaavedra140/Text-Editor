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

/// A target location within a document (URI + Range) for Go to Definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

/// A single text edit replacement in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspTextEdit {
    pub range: LspRange,
    #[serde(rename = "newText")]
    pub new_text: String,
}

/// A workspace-wide edit across multiple documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LspWorkspaceEdit {
    pub changes: Option<HashMap<String, Vec<LspTextEdit>>>,
    #[serde(rename = "documentChanges")]
    pub document_changes: Option<Vec<Value>>,
}

impl LspWorkspaceEdit {
    /// Extracts all file URI -> text edits mappings from `changes` and `documentChanges`.
    pub fn all_changes(&self) -> HashMap<String, Vec<LspTextEdit>> {
        let mut map = HashMap::new();
        if let Some(ref changes) = self.changes {
            for (uri, edits) in changes {
                map.entry(uri.clone())
                    .or_insert_with(Vec::new)
                    .extend(edits.clone());
            }
        }
        if let Some(ref doc_changes) = self.document_changes {
            for doc_change in doc_changes {
                if let Some(uri) = doc_change
                    .get("textDocument")
                    .and_then(|td| td.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(edits_val) = doc_change.get("edits").and_then(|e| e.as_array()) {
                        let parsed: Vec<LspTextEdit> = edits_val
                            .iter()
                            .filter_map(|e| serde_json::from_value::<LspTextEdit>(e.clone()).ok())
                            .collect();
                        map.entry(uri.to_string())
                            .or_insert_with(Vec::new)
                            .extend(parsed);
                    }
                }
            }
        }
        map
    }
}

/// A code action (quick-fix, refactoring, assist) returned by the language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspCodeAction {
    pub title: String,
    pub kind: Option<String>,
    pub edit: Option<LspWorkspaceEdit>,
    #[serde(rename = "isPreferred")]
    pub is_preferred: Option<bool>,
}

/// Kind of an inlay hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InlayHintKind {
    Type = 1,
    Parameter = 2,
}

/// An inlay hint displaying ghost annotations for types or parameter names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspInlayHint {
    pub position: LspPosition,
    pub label: String,
    pub kind: Option<InlayHintKind>,
    #[serde(rename = "paddingLeft")]
    pub padding_left: Option<bool>,
    #[serde(rename = "paddingRight")]
    pub padding_right: Option<bool>,
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
    /// Pending requests mapping request ID -> method name
    pub pending_requests: HashMap<u64, &'static str>,
    /// Pending responses keyed by request ID
    pub responses: HashMap<u64, Value>,
    /// Latest hover result
    pub latest_hover: Option<LspHover>,
    /// Latest Go-to-Definition location
    pub latest_definition: Option<LspLocation>,
    /// Latest completion list
    pub latest_completions: Vec<LspCompletionItem>,
    /// Latest code actions
    pub latest_code_actions: Vec<LspCodeAction>,
    /// Latest inlay hints
    pub latest_inlay_hints: Vec<LspInlayHint>,
    /// Latest rename workspace edit
    pub latest_rename_edit: Option<LspWorkspaceEdit>,
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
        let mut cmd = Command::new(server_cmd);
        cmd.args(args);
        cmd.current_dir(root_path);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        crate::platform::configure_background_command(&mut cmd);
        let mut child = cmd
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
        let root_uri = crate::platform::path_to_uri(root_path);
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
                    },
                    "codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": [
                                    "",
                                    "quickfix",
                                    "refactor",
                                    "refactor.extract",
                                    "refactor.inline",
                                    "refactor.rewrite",
                                    "source",
                                    "source.organizeImports"
                                ]
                            }
                        },
                        "isPreferredSupport": true
                    },
                    "inlayHint": {
                        "dynamicRegistration": true
                    },
                    "rename": {
                        "prepareSupport": false
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
        if let Ok(mut state) = self.state.write() {
            state.pending_requests.insert(id, "completion");
        }
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
        if let Ok(mut state) = self.state.write() {
            state.pending_requests.insert(id, "hover");
        }
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

    /// Requests Go to Definition for a symbol at a given line and column.
    pub fn request_definition(&self, file_uri: &str, line: usize, character: usize) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut state) = self.state.write() {
            state.pending_requests.insert(id, "definition");
        }
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            },
            "position": {
                "line": line,
                "character": character,
            }
        });
        self.send_raw(format_request(id, "textDocument/definition", params));
        id
    }

    /// Requests code actions for a file URI, range, and diagnostics context.
    pub fn request_code_actions(
        &self,
        file_uri: &str,
        range: LspRange,
        diagnostics: &[LspDiagnostic],
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut state) = self.state.write() {
            state.pending_requests.insert(id, "code_action");
        }
        let diags_val = serde_json::to_value(diagnostics).unwrap_or_else(|_| json!([]));
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            },
            "range": range,
            "context": {
                "diagnostics": diags_val,
            }
        });
        self.send_raw(format_request(id, "textDocument/codeAction", params));
        id
    }

    /// Consumes and returns the latest code actions list if available.
    pub fn take_latest_code_actions(&self) -> Vec<LspCodeAction> {
        self.state
            .write()
            .ok()
            .map(|mut s| std::mem::take(&mut s.latest_code_actions))
            .unwrap_or_default()
    }

    /// Injects mock code actions directly (used by unit tests).
    pub fn set_mock_code_actions(&self, actions: Vec<LspCodeAction>) {
        if let Ok(mut state) = self.state.write() {
            state.latest_code_actions = actions;
        }
    }

    /// Requests inlay hints for a given file URI and visible line range.
    pub fn request_inlay_hints(&self, file_uri: &str, range: LspRange) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut state) = self.state.write() {
            state.pending_requests.insert(id, "inlay_hint");
        }
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            },
            "range": range,
        });
        self.send_raw(format_request(id, "textDocument/inlayHint", params));
        id
    }

    /// Consumes and returns the latest inlay hints if available.
    pub fn take_latest_inlay_hints(&self) -> Vec<LspInlayHint> {
        self.state
            .write()
            .ok()
            .map(|mut s| std::mem::take(&mut s.latest_inlay_hints))
            .unwrap_or_default()
    }

    /// Injects mock inlay hints directly (used by unit tests).
    pub fn set_mock_inlay_hints(&self, hints: Vec<LspInlayHint>) {
        if let Ok(mut state) = self.state.write() {
            state.latest_inlay_hints = hints;
        }
    }

    /// Requests workspace rename of a symbol at the given position.
    pub fn request_rename(&self, file_uri: &str, position: LspPosition, new_name: &str) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut state) = self.state.write() {
            state.pending_requests.insert(id, "rename");
        }
        let params = json!({
            "textDocument": {
                "uri": file_uri,
            },
            "position": position,
            "newName": new_name,
        });
        self.send_raw(format_request(id, "textDocument/rename", params));
        id
    }

    /// Consumes and returns the latest rename workspace edit if available.
    pub fn take_latest_rename(&self) -> Option<LspWorkspaceEdit> {
        self.state
            .write()
            .ok()
            .and_then(|mut s| s.latest_rename_edit.take())
    }

    /// Injects mock rename workspace edit directly (used by unit tests).
    pub fn set_mock_rename(&self, edit: LspWorkspaceEdit) {
        if let Ok(mut state) = self.state.write() {
            state.latest_rename_edit = Some(edit);
        }
    }

    /// Retrieves latest Go to Definition location if available.
    pub fn latest_definition(&self) -> Option<LspLocation> {
        self.state.read().ok().and_then(|s| s.latest_definition.clone())
    }

    /// Consumes and returns the latest Go to Definition location.
    pub fn take_latest_definition(&self) -> Option<LspLocation> {
        self.state.write().ok().and_then(|mut s| s.latest_definition.take())
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

    /// Injects definition location directly (used by mock tests and notifications).
    pub fn set_mock_definition(&self, target: LspLocation) {
        if let Ok(mut state) = self.state.write() {
            state.latest_definition = Some(target);
        }
    }

    /// Injects hover payload directly (used by mock tests and notifications).
    pub fn set_mock_hover(&self, hover: LspHover) {
        if let Ok(mut state) = self.state.write() {
            state.latest_hover = Some(hover);
        }
    }

    /// Retrieves latest hover information if available.
    pub fn latest_hover(&self) -> Option<LspHover> {
        self.state.read().ok().and_then(|s| s.latest_hover.clone())
    }

    /// Consumes and returns the latest hover information if available.
    pub fn take_latest_hover(&self) -> Option<LspHover> {
        self.state.write().ok().and_then(|mut s| s.latest_hover.take())
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
                    let req_type = state.pending_requests.remove(&id);

                    match req_type {
                        Some("code_action") => {
                            if let Some(arr) = result.as_array() {
                                state.latest_code_actions = parse_code_actions_result(arr);
                            }
                        }
                        Some("inlay_hint") => {
                            if let Some(arr) = result.as_array() {
                                state.latest_inlay_hints = parse_inlay_hints_result(arr);
                            }
                        }
                        Some("rename") => {
                            if let Ok(edit) =
                                serde_json::from_value::<LspWorkspaceEdit>(result.clone())
                            {
                                state.latest_rename_edit = Some(edit);
                            }
                        }
                        Some("hover") => {
                            if let Some(hover) = parse_hover_result(result) {
                                state.latest_hover = Some(hover);
                            }
                        }
                        Some("definition") => {
                            if let Some(target) = parse_definition_result(result) {
                                state.latest_definition = Some(target);
                            }
                        }
                        Some("completion") => {
                            if let Some(items) = parse_completion_result(result) {
                                state.latest_completions = items;
                            }
                        }
                        _ => {
                            // Fallback heuristic for untracked requests / tests
                            if let Some(hover) = parse_hover_result(result) {
                                state.latest_hover = Some(hover);
                            } else if let Some(target) = parse_definition_result(result) {
                                state.latest_definition = Some(target);
                            } else if let Ok(edit) =
                                serde_json::from_value::<LspWorkspaceEdit>(result.clone())
                            {
                                if edit.changes.is_some() || edit.document_changes.is_some() {
                                    state.latest_rename_edit = Some(edit);
                                }
                            } else if let Some(arr) = result.as_array() {
                                if arr.first().and_then(|x| x.get("title")).is_some() {
                                    state.latest_code_actions = parse_code_actions_result(arr);
                                } else if arr.first().and_then(|x| x.get("label")).is_some()
                                    && arr.first().and_then(|x| x.get("position")).is_some()
                                {
                                    state.latest_inlay_hints = parse_inlay_hints_result(arr);
                                } else if let Some(items) = parse_completion_result(result) {
                                    state.latest_completions = items;
                                }
                            } else if let Some(items) = parse_completion_result(result) {
                                state.latest_completions = items;
                            }
                        }
                    }

                    state.responses.insert(id, result.clone());
                }
            }
        }
    }
}

fn parse_code_actions_result(arr: &[Value]) -> Vec<LspCodeAction> {
    arr.iter()
        .filter_map(|item| {
            let title = item.get("title")?.as_str()?.to_string();
            let kind = item
                .get("kind")
                .and_then(|k| k.as_str())
                .map(|k| k.to_string());
            let is_preferred = item.get("isPreferred").and_then(|p| p.as_bool());
            let edit = item
                .get("edit")
                .and_then(|e| serde_json::from_value::<LspWorkspaceEdit>(e.clone()).ok());
            Some(LspCodeAction {
                title,
                kind,
                edit,
                is_preferred,
            })
        })
        .collect()
}

fn parse_inlay_hints_result(arr: &[Value]) -> Vec<LspInlayHint> {
    arr.iter()
        .filter_map(|item| {
            let pos_val = item.get("position")?;
            let position = serde_json::from_value::<LspPosition>(pos_val.clone()).ok()?;
            let label = if let Some(s) = item.get("label").and_then(|l| l.as_str()) {
                s.to_string()
            } else if let Some(parts) = item.get("label").and_then(|l| l.as_array()) {
                parts
                    .iter()
                    .filter_map(|p| p.get("value").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            } else {
                return None;
            };
            let kind = item.get("kind").and_then(|k| k.as_u64()).map(|k| match k {
                1 => InlayHintKind::Type,
                2 => InlayHintKind::Parameter,
                _ => InlayHintKind::Type,
            });
            let padding_left = item.get("paddingLeft").and_then(|p| p.as_bool());
            let padding_right = item.get("paddingRight").and_then(|p| p.as_bool());

            Some(LspInlayHint {
                position,
                label,
                kind,
                padding_left,
                padding_right,
            })
        })
        .collect()
}

fn parse_hover_result(result: &Value) -> Option<LspHover> {
    let contents = result.get("contents")?;
    let text = if let Some(s) = contents.as_str() {
        s.to_string()
    } else if let Some(val_str) = contents.get("value").and_then(|v| v.as_str()) {
        val_str.to_string()
    } else {
        contents.to_string()
    };
    let range = result
        .get("range")
        .and_then(|r| serde_json::from_value::<LspRange>(r.clone()).ok());
    Some(LspHover {
        contents: text,
        range,
    })
}

fn parse_completion_result(result: &Value) -> Option<Vec<LspCompletionItem>> {
    let items_opt = if let Some(arr) = result.as_array() {
        Some(arr)
    } else {
        result.get("items").and_then(|i| i.as_array())
    }?;

    let parsed_items: Vec<LspCompletionItem> = items_opt
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let kind = item
                .get("kind")
                .and_then(|k| k.as_u64())
                .map(|k| k as usize);
            let detail = item
                .get("detail")
                .and_then(|d| d.as_str())
                .map(|d| d.to_string());
            let documentation = item.get("documentation").and_then(|d| {
                if let Some(s) = d.as_str() {
                    Some(s.to_string())
                } else {
                    d.get("value")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
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
    Some(parsed_items)
}

fn parse_definition_result(result: &Value) -> Option<LspLocation> {
    if let Some(arr) = result.as_array() {
        let first = arr.first()?;
        // Check for LocationLink (targetUri + targetSelectionRange or targetRange)
        if let Some(target_uri) = first.get("targetUri").and_then(|u| u.as_str()) {
            let range_val = first.get("targetSelectionRange").or_else(|| first.get("targetRange"))?;
            let range = serde_json::from_value::<LspRange>(range_val.clone()).ok()?;
            return Some(LspLocation { uri: target_uri.to_string(), range });
        }
        // Check for standard Location (uri + range)
        if let Some(uri) = first.get("uri").and_then(|u| u.as_str()) {
            let range = serde_json::from_value::<LspRange>(first.get("range")?.clone()).ok()?;
            return Some(LspLocation { uri: uri.to_string(), range });
        }
    } else if let Some(uri) = result.get("uri").and_then(|u| u.as_str()) {
        let range = serde_json::from_value::<LspRange>(result.get("range")?.clone()).ok()?;
        return Some(LspLocation { uri: uri.to_string(), range });
    }
    None
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(ref proc_lock) = self.server_process {
            if Arc::strong_count(proc_lock) <= 1 {
                if let Ok(mut child) = proc_lock.lock() {
                    let _ = child.kill();
                }
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

    #[test]
    fn test_inbound_definition_dispatch() {
        let client = LspClient::mock();

        let def_resp = json!({
            "jsonrpc": "2.0",
            "id": 12,
            "result": [
                {
                    "uri": "file:///path/to/target.rs",
                    "range": {
                        "start": { "line": 42, "character": 7 },
                        "end": { "line": 42, "character": 15 }
                    }
                }
            ]
        });

        LspClient::handle_inbound_message(&client.state, &def_resp.to_string());
        let def = client.take_latest_definition().expect("should have definition target");
        assert_eq!(def.uri, "file:///path/to/target.rs");
        assert_eq!(def.range.start.line, 42);
        assert_eq!(def.range.start.character, 7);
        assert!(client.latest_definition().is_none(), "take should consume it");
    }

    #[test]
    fn test_inbound_code_action_and_workspace_edit() {
        let client = LspClient::mock();

        let req_id = client.request_code_actions(
            "file:///src/main.rs",
            LspRange {
                start: LspPosition { line: 10, character: 5 },
                end: LspPosition { line: 10, character: 15 },
            },
            &[],
        );

        let code_action_resp = json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": [
                {
                    "title": "Import `std::collections::HashMap`",
                    "kind": "quickfix",
                    "isPreferred": true,
                    "edit": {
                        "changes": {
                            "file:///src/main.rs": [
                                {
                                    "range": {
                                        "start": { "line": 0, "character": 0 },
                                        "end": { "line": 0, "character": 0 }
                                    },
                                    "newText": "use std::collections::HashMap;\n"
                                }
                            ]
                        }
                    }
                }
            ]
        });

        LspClient::handle_inbound_message(&client.state, &code_action_resp.to_string());
        let actions = client.take_latest_code_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Import `std::collections::HashMap`");
        assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
        assert_eq!(actions[0].is_preferred, Some(true));

        let edit = actions[0].edit.as_ref().expect("should have workspace edit");
        let all_changes = edit.all_changes();
        let main_edits = all_changes.get("file:///src/main.rs").expect("should have edits for file");
        assert_eq!(main_edits.len(), 1);
        assert_eq!(main_edits[0].new_text, "use std::collections::HashMap;\n");
    }

    #[test]
    fn test_inbound_inlay_hint_and_rename() {
        let client = LspClient::mock();

        let hint_id = client.request_inlay_hints(
            "file:///src/main.rs",
            LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 50, character: 0 },
            },
        );

        let hint_resp = json!({
            "jsonrpc": "2.0",
            "id": hint_id,
            "result": [
                {
                    "position": { "line": 5, "character": 12 },
                    "label": ": i32",
                    "kind": 1,
                    "paddingLeft": true,
                    "paddingRight": false
                }
            ]
        });

        LspClient::handle_inbound_message(&client.state, &hint_resp.to_string());
        let hints = client.take_latest_inlay_hints();
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, ": i32");
        assert_eq!(hints[0].kind, Some(InlayHintKind::Type));
        assert_eq!(hints[0].padding_left, Some(true));

        let rename_id = client.request_rename(
            "file:///src/main.rs",
            LspPosition { line: 5, character: 10 },
            "new_identifier",
        );

        let rename_resp = json!({
            "jsonrpc": "2.0",
            "id": rename_id,
            "result": {
                "changes": {
                    "file:///src/main.rs": [
                        {
                            "range": {
                                "start": { "line": 5, "character": 8 },
                                "end": { "line": 5, "character": 12 }
                            },
                            "newText": "new_identifier"
                        }
                    ]
                }
            }
        });

        LspClient::handle_inbound_message(&client.state, &rename_resp.to_string());
        let rename_edit = client.take_latest_rename().expect("should have rename edit");
        let changes = rename_edit.all_changes();
        assert_eq!(changes["file:///src/main.rs"][0].new_text, "new_identifier");
    }
}
