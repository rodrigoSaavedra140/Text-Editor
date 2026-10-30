//! Native Cargo pipeline execution and diagnostic parser for Karkinos.
//!
//! Provides background execution for `cargo check`, `cargo clippy`, `cargo test`,
//! `cargo build`, and `cargo run` with streaming output and line-exact error navigation.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use serde_json::Value;

/// Cargo subcommands supported directly by Karkinos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoAction {
    Check,
    Clippy,
    Test,
    Build,
    Run,
}

impl CargoAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            CargoAction::Check => "check",
            CargoAction::Clippy => "clippy",
            CargoAction::Test => "test",
            CargoAction::Build => "build",
            CargoAction::Run => "run",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            CargoAction::Check => "cargo check",
            CargoAction::Clippy => "cargo clippy",
            CargoAction::Test => "cargo test",
            CargoAction::Build => "cargo build",
            CargoAction::Run => "cargo run",
        }
    }
}

/// A structured diagnostic emitted by the Rust compiler through Cargo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoDiagnostic {
    pub level: String, // "error", "warning", "note", "help"
    pub message: String,
    pub file: Option<PathBuf>,
    pub line: usize,
    pub col: usize,
    pub rendered: String,
}

/// An individual raw line of output (stdout or stderr) from Cargo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoOutputLine {
    pub text: String,
    pub is_error: bool,
}

/// Current lifecycle status of the Cargo runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoStatus {
    Idle,
    Running,
    Success,
    Failed(i32),
    Killed,
}

/// Thread-safe coordinator that runs Cargo tasks asynchronously and streams output.
pub struct CargoRunner {
    pub current_action: Option<CargoAction>,
    status: Arc<RwLock<CargoStatus>>,
    diagnostics: Arc<RwLock<Vec<CargoDiagnostic>>>,
    raw_lines: Arc<RwLock<Vec<CargoOutputLine>>>,
    child_process: Arc<Mutex<Option<Child>>>,
}

impl Default for CargoRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CargoRunner {
    /// Creates a new idle `CargoRunner`.
    pub fn new() -> Self {
        Self {
            current_action: None,
            status: Arc::new(RwLock::new(CargoStatus::Idle)),
            diagnostics: Arc::new(RwLock::new(Vec::new())),
            raw_lines: Arc::new(RwLock::new(Vec::new())),
            child_process: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the current execution status.
    pub fn status(&self) -> CargoStatus {
        *self.status.read().unwrap()
    }

    /// Returns whether a Cargo process is currently executing.
    pub fn is_running(&self) -> bool {
        matches!(self.status(), CargoStatus::Running)
    }

    /// Returns a snapshot of all accumulated compiler diagnostics.
    pub fn diagnostics(&self) -> Vec<CargoDiagnostic> {
        self.diagnostics.read().unwrap().clone()
    }

    /// Returns the count of errors and warnings respectively.
    pub fn error_and_warning_counts(&self) -> (usize, usize) {
        let diags = self.diagnostics.read().unwrap();
        let errors = diags.iter().filter(|d| d.level == "error").count();
        let warnings = diags.iter().filter(|d| d.level == "warning").count();
        (errors, warnings)
    }

    /// Returns a snapshot of all raw output lines.
    pub fn raw_lines(&self) -> Vec<CargoOutputLine> {
        self.raw_lines.read().unwrap().clone()
    }

    /// Clears all diagnostics and raw output.
    pub fn clear(&self) {
        self.diagnostics.write().unwrap().clear();
        self.raw_lines.write().unwrap().clear();
    }

    /// Terminate any active running Cargo process.
    pub fn kill(&mut self) {
        if let Ok(mut lock) = self.child_process.lock() {
            if let Some(ref mut child) = *lock {
                let _ = child.kill();
            }
            *lock = None;
        }
        *self.status.write().unwrap() = CargoStatus::Killed;
    }

    /// Spawns a background Cargo action within `working_dir`.
    pub fn spawn(
        &mut self,
        action: CargoAction,
        working_dir: &Path,
        extra_args: &[&str],
    ) -> Result<(), String> {
        // Kill any previous running job
        self.kill();
        self.clear();
        self.current_action = Some(action);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(working_dir);
        cmd.arg(action.as_str());

        // Use json message format for check, clippy, and build to parse exact file spans
        match action {
            CargoAction::Check | CargoAction::Clippy | CargoAction::Build => {
                cmd.arg("--message-format=json");
            }
            CargoAction::Test => {
                // Keep default test output for human reading
            }
            CargoAction::Run => {}
        }

        for arg in extra_args {
            cmd.arg(arg);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn 'cargo {}': {}", action.as_str(), e))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        if let Ok(mut proc_lock) = self.child_process.lock() {
            *proc_lock = Some(child);
        }

        *self.status.write().unwrap() = CargoStatus::Running;

        let status_arc = Arc::clone(&self.status);
        let diags_arc = Arc::clone(&self.diagnostics);
        let lines_arc = Arc::clone(&self.raw_lines);
        let proc_arc = Arc::clone(&self.child_process);

        thread::Builder::new()
            .name("cargo-runner-worker".to_string())
            .spawn(move || {
                // Stdout reading thread
                let diags_writer = Arc::clone(&diags_arc);
                let lines_writer = Arc::clone(&lines_arc);

                let stdout_handle = stdout.map(|out| {
                    thread::spawn(move || {
                        let reader = BufReader::new(out);
                        for line in reader.lines().map_while(Result::ok) {
                            if let Some(diag) = parse_compiler_message(&line) {
                                let rendered = diag.rendered.clone();
                                diags_writer.write().unwrap().push(diag);
                                if !rendered.is_empty() {
                                    lines_writer.write().unwrap().push(CargoOutputLine {
                                        text: rendered,
                                        is_error: false,
                                    });
                                }
                            } else {
                                lines_writer.write().unwrap().push(CargoOutputLine {
                                    text: line,
                                    is_error: false,
                                });
                            }
                        }
                    })
                });

                // Stderr reading thread
                let stderr_handle = stderr.map(|err| {
                    let lines_err_writer = Arc::clone(&lines_arc);
                    let diags_err_writer = Arc::clone(&diags_arc);
                    thread::spawn(move || {
                        let reader = BufReader::new(err);
                        for line in reader.lines().map_while(Result::ok) {
                            if let Some(diag) = parse_compiler_message(&line) {
                                diags_err_writer.write().unwrap().push(diag);
                            }
                            lines_err_writer.write().unwrap().push(CargoOutputLine {
                                text: line,
                                is_error: true,
                            });
                        }
                    })
                });

                if let Some(h) = stdout_handle {
                    let _ = h.join();
                }
                if let Some(h) = stderr_handle {
                    let _ = h.join();
                }

                // Wait for process exit status
                let exit_status = {
                    let mut lock = proc_arc.lock().unwrap();
                    if let Some(ref mut child) = *lock {
                        child.wait().ok()
                    } else {
                        None
                    }
                };

                let new_status = match exit_status {
                    Some(s) if s.success() => CargoStatus::Success,
                    Some(s) => CargoStatus::Failed(s.code().unwrap_or(1)),
                    None => CargoStatus::Killed,
                };

                *status_arc.write().unwrap() = new_status;
            })
            .map_err(|e| format!("Failed to spawn thread: {}", e))?;

        Ok(())
    }
}

/// Parses a compiler message from a single JSON line outputted by `cargo --message-format=json`.
pub fn parse_compiler_message(json_line: &str) -> Option<CargoDiagnostic> {
    let val: Value = serde_json::from_str(json_line).ok()?;
    if val.get("reason")?.as_str()? != "compiler-message" {
        return None;
    }

    let msg = val.get("message")?;
    let level = msg.get("level")?.as_str()?.to_string();
    let message_text = msg.get("message")?.as_str()?.to_string();
    let rendered = msg.get("rendered").and_then(|r| r.as_str()).unwrap_or("").to_string();

    let spans = msg.get("spans")?.as_array()?;
    let primary_span = spans.iter().find(|s| s.get("is_primary").and_then(|p| p.as_bool()) == Some(true))
        .or_else(|| spans.first());

    let (file, line, col) = if let Some(span) = primary_span {
        let file_name = span.get("file_name").and_then(|f| f.as_str()).map(PathBuf::from);
        let line_start = span.get("line_start").and_then(|l| l.as_u64()).unwrap_or(1) as usize;
        let column_start = span.get("column_start").and_then(|c| c.as_u64()).unwrap_or(1) as usize;
        (file_name, line_start.saturating_sub(1), column_start.saturating_sub(1))
    } else {
        (None, 0, 0)
    };

    Some(CargoDiagnostic {
        level,
        message: message_text,
        file,
        line,
        col,
        rendered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compiler_message_error_with_spans() {
        let json_line = r#"{
            "reason": "compiler-message",
            "package_id": "test_crate 0.1.0",
            "message": {
                "rendered": "error[E0425]: cannot find value `x` in this scope\n --> src/main.rs:12:9\n",
                "level": "error",
                "message": "cannot find value `x` in this scope",
                "spans": [
                    {
                        "file_name": "src/main.rs",
                        "byte_start": 120,
                        "byte_end": 121,
                        "line_start": 12,
                        "line_end": 12,
                        "column_start": 9,
                        "column_end": 10,
                        "is_primary": true
                    }
                ]
            }
        }"#;

        let diag = parse_compiler_message(json_line).expect("should parse compiler error");
        assert_eq!(diag.level, "error");
        assert_eq!(diag.message, "cannot find value `x` in this scope");
        assert_eq!(diag.file, Some(PathBuf::from("src/main.rs")));
        assert_eq!(diag.line, 11); // 0-based
        assert_eq!(diag.col, 8);   // 0-based
        assert!(diag.rendered.contains("error[E0425]"));
    }

    #[test]
    fn test_parse_compiler_message_ignores_artifacts() {
        let json_line = r#"{"reason": "compiler-artifact", "package_id": "foo 0.1.0"}"#;
        assert!(parse_compiler_message(json_line).is_none());
    }

    #[test]
    fn test_cargo_action_names() {
        assert_eq!(CargoAction::Check.as_str(), "check");
        assert_eq!(CargoAction::Clippy.as_str(), "clippy");
        assert_eq!(CargoAction::Test.as_str(), "test");
        assert_eq!(CargoAction::Build.as_str(), "build");
        assert_eq!(CargoAction::Run.as_str(), "run");
    }

    #[test]
    fn test_cargo_runner_lifecycle_and_counts() {
        let runner = CargoRunner::new();
        assert_eq!(runner.status(), CargoStatus::Idle);
        assert!(!runner.is_running());

        // Inject diagnostics for count testing
        runner.diagnostics.write().unwrap().push(CargoDiagnostic {
            level: "error".to_string(),
            message: "type mismatch".to_string(),
            file: Some(PathBuf::from("src/lib.rs")),
            line: 5,
            col: 2,
            rendered: "error: type mismatch".to_string(),
        });
        runner.diagnostics.write().unwrap().push(CargoDiagnostic {
            level: "warning".to_string(),
            message: "unused variable".to_string(),
            file: Some(PathBuf::from("src/lib.rs")),
            line: 8,
            col: 9,
            rendered: "warning: unused variable".to_string(),
        });

        let (errors, warnings) = runner.error_and_warning_counts();
        assert_eq!(errors, 1);
        assert_eq!(warnings, 1);

        runner.clear();
        assert_eq!(runner.diagnostics().len(), 0);
    }
}
