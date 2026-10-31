//! Debug Adapter Protocol (DAP) client engine for Karkinos.
//! Handles background debugger communication (CodeLLDB / lldb-dap / gdb),
//! breakpoint synchronization, call stack frames, and variable inspection.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Reason why program execution was stopped by the debugger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DapStopReason {
    Breakpoint,
    Step,
    Pause,
    Exception,
    Entry,
    Unknown(String),
}

/// A stack frame representing a call level in the stopped thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DapStackFrame {
    pub id: u64,
    pub name: String,
    pub source_path: Option<PathBuf>,
    pub line: usize,
    pub column: usize,
}

/// A variable inspected in the current execution scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DapVariable {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
}

/// A breakpoint verified or reported by the debugger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DapBreakpoint {
    pub id: Option<u64>,
    pub verified: bool,
    pub line: usize,
    pub source_path: Option<PathBuf>,
}

/// State shared between UI and background debugger workers.
#[derive(Debug, Clone, Default)]
pub struct DapState {
    pub is_active: bool,
    pub is_stopped: bool,
    pub stop_reason: Option<DapStopReason>,
    pub current_frame: Option<DapStackFrame>,
    pub stack_frames: Vec<DapStackFrame>,
    pub variables: Vec<DapVariable>,
    pub output_log: Vec<String>,
    pub verified_breakpoints: HashMap<PathBuf, Vec<DapBreakpoint>>,
}

/// Thread-safe client managing DAP interactions.
pub struct DapClient {
    state: Arc<RwLock<DapState>>,
    next_id: AtomicU64,
    stdin_tx: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    server_process: Option<Arc<Mutex<Child>>>,
    debugger_name: String,
}

impl Clone for DapClient {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
            stdin_tx: self.stdin_tx.clone(),
            server_process: self.server_process.clone(),
            debugger_name: self.debugger_name.clone(),
        }
    }
}

impl std::fmt::Debug for DapClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DapClient")
            .field("debugger_name", &self.debugger_name)
            .field("is_active", &self.stdin_tx.is_some())
            .finish()
    }
}

impl DapClient {
    /// Formats a DAP JSON-RPC request packet with Content-Length header.
    pub fn format_dap_request(seq: u64, command: &str, arguments: Option<Value>) -> Vec<u8> {
        let mut msg = json!({
            "seq": seq,
            "type": "request",
            "command": command,
        });
        if let Some(args) = arguments {
            msg["arguments"] = args;
        }
        let body = msg.to_string();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
    }

    /// Spawns a DAP debugger server process (e.g. `lldb-dap` or `codelldb`).
    pub fn spawn(debugger_cmd: &str, args: &[&str], root_path: &Path) -> Result<Self, String> {
        let mut child = Command::new(debugger_cmd)
            .args(args)
            .current_dir(root_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn debugger '{}': {}", debugger_cmd, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture debugger stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture debugger stdout".to_string())?;

        let state = Arc::new(RwLock::new(DapState {
            is_active: true,
            ..Default::default()
        }));
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();

        // Stdin thread
        thread::Builder::new()
            .name("dap-stdin-writer".to_string())
            .spawn(move || {
                let mut writer = stdin;
                while let Ok(msg) = rx.recv() {
                    if writer.write_all(&msg).is_err() || writer.flush().is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn debugger stdin thread: {}", e))?;

        // Stdout reader thread
        let reader_state = Arc::clone(&state);
        thread::Builder::new()
            .name("dap-stdout-reader".to_string())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(body)) = crate::lsp::read_lsp_message(&mut reader) {
                    Self::handle_inbound_message(&reader_state, &body);
                }
                if let Ok(mut s) = reader_state.write() {
                    s.is_active = false;
                    s.is_stopped = false;
                }
            })
            .map_err(|e| format!("Failed to spawn debugger stdout thread: {}", e))?;

        let client = Self {
            state,
            next_id: AtomicU64::new(1),
            stdin_tx: Some(tx),
            server_process: Some(Arc::new(Mutex::new(child))),
            debugger_name: debugger_cmd.to_string(),
        };

        // Initialize handshake
        client.send_initialize();
        Ok(client)
    }

    /// Creates a headless mock client for unit testing.
    pub fn mock() -> Self {
        Self {
            state: Arc::new(RwLock::new(DapState {
                is_active: true,
                ..Default::default()
            })),
            next_id: AtomicU64::new(1),
            stdin_tx: None,
            server_process: None,
            debugger_name: "mock-dap".to_string(),
        }
    }

    /// Returns a snapshot of the current debugger state.
    pub fn state(&self) -> DapState {
        self.state.read().ok().map(|s| s.clone()).unwrap_or_default()
    }

    /// Sends raw bytes to the debugger stdin channel.
    pub fn send_raw(&self, payload: Vec<u8>) {
        if let Some(ref tx) = self.stdin_tx {
            let _ = tx.send(payload);
        }
    }

    /// Sends the DAP `initialize` request.
    pub fn send_initialize(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({
            "clientID": "karkinos",
            "clientName": "Karkinos Rust IDE",
            "adapterID": "lldb",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
            "supportsVariableType": true,
        });
        self.send_raw(Self::format_dap_request(seq, "initialize", Some(args)));
        seq
    }

    /// Launches a target executable binary.
    pub fn launch(&self, binary_path: &Path, program_args: &[&str]) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({
            "program": binary_path.display().to_string(),
            "args": program_args,
            "stopOnEntry": false,
            "cwd": binary_path.parent().map(|p| p.display().to_string()).unwrap_or_else(|| ".".to_string()),
        });
        self.send_raw(Self::format_dap_request(seq, "launch", Some(args)));
        if let Ok(mut s) = self.state.write() {
            s.is_active = true;
            s.is_stopped = false;
            s.output_log.push(format!("Launching: {}", binary_path.display()));
        }
        seq
    }

    /// Sets breakpoints for a file.
    pub fn set_breakpoints(&self, file_path: &Path, lines: &[usize]) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let bps: Vec<Value> = lines.iter().map(|line| json!({ "line": line + 1 })).collect();
        let args = json!({
            "source": {
                "path": file_path.display().to_string(),
            },
            "breakpoints": bps,
        });
        self.send_raw(Self::format_dap_request(seq, "setBreakpoints", Some(args)));
        seq
    }

    /// Continues execution of a stopped thread.
    pub fn continue_execution(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({ "threadId": 1 });
        self.send_raw(Self::format_dap_request(seq, "continue", Some(args)));
        if let Ok(mut s) = self.state.write() {
            s.is_stopped = false;
            s.stop_reason = None;
            s.output_log.push("Execution resumed".to_string());
        }
        seq
    }

    /// Steps over the current statement (next).
    pub fn step_over(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({ "threadId": 1 });
        self.send_raw(Self::format_dap_request(seq, "next", Some(args)));
        if let Ok(mut s) = self.state.write() {
            s.is_stopped = false;
        }
        seq
    }

    /// Steps into the current function call.
    pub fn step_in(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({ "threadId": 1 });
        self.send_raw(Self::format_dap_request(seq, "stepIn", Some(args)));
        if let Ok(mut s) = self.state.write() {
            s.is_stopped = false;
        }
        seq
    }

    /// Steps out of the current function.
    pub fn step_out(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({ "threadId": 1 });
        self.send_raw(Self::format_dap_request(seq, "stepOut", Some(args)));
        if let Ok(mut s) = self.state.write() {
            s.is_stopped = false;
        }
        seq
    }

    /// Pauses execution.
    pub fn pause(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({ "threadId": 1 });
        self.send_raw(Self::format_dap_request(seq, "pause", Some(args)));
        seq
    }

    /// Terminates the debug session.
    pub fn disconnect(&self) -> u64 {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let args = json!({ "terminateDebuggee": true });
        self.send_raw(Self::format_dap_request(seq, "disconnect", Some(args)));
        if let Ok(mut s) = self.state.write() {
            s.is_active = false;
            s.is_stopped = false;
            s.stack_frames.clear();
            s.variables.clear();
            s.current_frame = None;
            s.output_log.push("Debug session terminated".to_string());
        }
        seq
    }

    /// Injects mock state directly for unit testing.
    pub fn set_mock_stopped(&self, reason: DapStopReason, frame: DapStackFrame, vars: Vec<DapVariable>) {
        if let Ok(mut s) = self.state.write() {
            s.is_active = true;
            s.is_stopped = true;
            s.stop_reason = Some(reason);
            s.current_frame = Some(frame.clone());
            s.stack_frames = vec![frame];
            s.variables = vars;
        }
    }

    /// Handles incoming DAP JSON messages (events, responses).
    pub fn handle_inbound_message(state_lock: &Arc<RwLock<DapState>>, body: &str) {
        let Ok(val) = serde_json::from_str::<Value>(body) else {
            return;
        };

        let msg_type = val.get("type").and_then(|t| t.as_str());

        // Event handling
        if msg_type == Some("event") {
            let event = val.get("event").and_then(|e| e.as_str()).unwrap_or("");
            match event {
                "stopped" => {
                    let reason_str = val.get("body").and_then(|b| b.get("reason")).and_then(|r| r.as_str()).unwrap_or("");
                    let reason = match reason_str {
                        "breakpoint" => DapStopReason::Breakpoint,
                        "step" => DapStopReason::Step,
                        "pause" => DapStopReason::Pause,
                        "exception" => DapStopReason::Exception,
                        "entry" => DapStopReason::Entry,
                        other => DapStopReason::Unknown(other.to_string()),
                    };

                    if let Ok(mut s) = state_lock.write() {
                        s.is_stopped = true;
                        s.stop_reason = Some(reason);
                        s.output_log.push(format!("Target stopped: {}", reason_str));
                    }
                }
                "output" => {
                    if let Some(output) = val.get("body").and_then(|b| b.get("output")).and_then(|o| o.as_str()) {
                        if let Ok(mut s) = state_lock.write() {
                            s.output_log.push(output.trim_end().to_string());
                        }
                    }
                }
                "terminated" | "exited" => {
                    if let Ok(mut s) = state_lock.write() {
                        s.is_active = false;
                        s.is_stopped = false;
                        s.current_frame = None;
                        s.stack_frames.clear();
                        s.variables.clear();
                        s.output_log.push("Target exited".to_string());
                    }
                }
                _ => {}
            }
            return;
        }

        // Response handling
        if msg_type == Some("response") {
            let command = val.get("command").and_then(|c| c.as_str()).unwrap_or("");
            let success = val.get("success").and_then(|s| s.as_bool()).unwrap_or(false);

            if !success {
                let msg = val.get("message").and_then(|m| m.as_str()).unwrap_or("DAP error");
                if let Ok(mut s) = state_lock.write() {
                    s.output_log.push(format!("Debugger error: {}", msg));
                }
                return;
            }

            match command {
                "stackTrace" => {
                    if let Some(frames_val) = val.get("body").and_then(|b| b.get("stackFrames")).and_then(|f| f.as_array()) {
                        let parsed: Vec<DapStackFrame> = frames_val.iter().filter_map(|f| {
                            let id = f.get("id")?.as_u64()?;
                            let name = f.get("name")?.as_str()?.to_string();
                            let line = f.get("line").and_then(|l| l.as_u64()).map(|l| (l as usize).saturating_sub(1)).unwrap_or(0);
                            let column = f.get("column").and_then(|c| c.as_u64()).map(|c| (c as usize).saturating_sub(1)).unwrap_or(0);
                            let source_path = f.get("source").and_then(|s| s.get("path")).and_then(|p| p.as_str()).map(PathBuf::from);
                            Some(DapStackFrame { id, name, source_path, line, column })
                        }).collect();

                        if let Ok(mut s) = state_lock.write() {
                            s.current_frame = parsed.first().cloned();
                            s.stack_frames = parsed;
                        }
                    }
                }
                "variables" => {
                    if let Some(vars_val) = val.get("body").and_then(|b| b.get("variables")).and_then(|v| v.as_array()) {
                        let parsed: Vec<DapVariable> = vars_val.iter().filter_map(|v| {
                            let name = v.get("name")?.as_str()?.to_string();
                            let value = v.get("value")?.as_str()?.to_string();
                            let type_name = v.get("type").and_then(|t| t.as_str()).map(|t| t.to_string());
                            Some(DapVariable { name, value, type_name })
                        }).collect();

                        if let Ok(mut s) = state_lock.write() {
                            s.variables = parsed;
                        }
                    }
                }
                "setBreakpoints" => {
                    if let Some(bps_val) = val.get("body").and_then(|b| b.get("breakpoints")).and_then(|b| b.as_array()) {
                        let verified: Vec<DapBreakpoint> = bps_val.iter().filter_map(|b| {
                            let id = b.get("id").and_then(|i| i.as_u64());
                            let verified = b.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
                            let line = b.get("line").and_then(|l| l.as_u64()).map(|l| (l as usize).saturating_sub(1)).unwrap_or(0);
                            Some(DapBreakpoint { id, verified, line, source_path: None })
                        }).collect();

                        if let Ok(mut s) = state_lock.write() {
                            s.output_log.push(format!("Synced {} breakpoints", verified.len()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for DapClient {
    fn drop(&mut self) {
        if let Some(ref proc_lock) = self.server_process {
            if Arc::strong_count(proc_lock) <= 1 {
                if let Ok(mut child) = proc_lock.lock() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dap_format_request() {
        let req = DapClient::format_dap_request(1, "initialize", Some(json!({ "clientID": "test" })));
        let text = String::from_utf8(req).unwrap();
        assert!(text.starts_with("Content-Length: "));
        assert!(text.contains("\"command\":\"initialize\""));
        assert!(text.contains("\"seq\":1"));
    }

    #[test]
    fn test_dap_inbound_stopped_and_variables() {
        let client = DapClient::mock();

        let stop_msg = json!({
            "seq": 10,
            "type": "event",
            "event": "stopped",
            "body": {
                "reason": "breakpoint",
                "threadId": 1
            }
        });

        DapClient::handle_inbound_message(&client.state, &stop_msg.to_string());
        let st = client.state();
        assert!(st.is_stopped);
        assert_eq!(st.stop_reason, Some(DapStopReason::Breakpoint));

        let stack_msg = json!({
            "seq": 11,
            "type": "response",
            "command": "stackTrace",
            "success": true,
            "body": {
                "stackFrames": [
                    {
                        "id": 100,
                        "name": "main()",
                        "line": 42,
                        "column": 5,
                        "source": { "path": "/src/main.rs" }
                    }
                ]
            }
        });

        DapClient::handle_inbound_message(&client.state, &stack_msg.to_string());
        let st = client.state();
        assert_eq!(st.stack_frames.len(), 1);
        assert_eq!(st.stack_frames[0].name, "main()");
        assert_eq!(st.stack_frames[0].line, 41); // 0-based
        assert_eq!(st.current_frame.as_ref().unwrap().name, "main()");

        let vars_msg = json!({
            "seq": 12,
            "type": "response",
            "command": "variables",
            "success": true,
            "body": {
                "variables": [
                    {
                        "name": "counter",
                        "value": "42",
                        "type": "i32"
                    }
                ]
            }
        });

        DapClient::handle_inbound_message(&client.state, &vars_msg.to_string());
        let st = client.state();
        assert_eq!(st.variables.len(), 1);
        assert_eq!(st.variables[0].name, "counter");
        assert_eq!(st.variables[0].value, "42");
        assert_eq!(st.variables[0].type_name.as_deref(), Some("i32"));
    }

    #[test]
    fn test_dap_mock_control() {
        let client = DapClient::mock();
        client.set_mock_stopped(
            DapStopReason::Step,
            DapStackFrame {
                id: 1,
                name: "calculate()".to_string(),
                source_path: Some(PathBuf::from("/test.rs")),
                line: 10,
                column: 4,
            },
            vec![DapVariable {
                name: "x".to_string(),
                value: "100".to_string(),
                type_name: Some("usize".to_string()),
            }],
        );

        let st = client.state();
        assert!(st.is_stopped);
        assert_eq!(st.stop_reason, Some(DapStopReason::Step));
        assert_eq!(st.variables[0].value, "100");

        client.continue_execution();
        assert!(!client.state().is_stopped);
    }
}
