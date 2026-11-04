//! Crash-recovery journaling for unsaved editor buffers.
//!
//! Dirty buffers are journaled into the user state directory so unexpected
//! shutdowns can be recovered on the next start. The on-disk format is a
//! line-oriented header followed by a delimiter and the raw buffer content, so a
//! file truncated mid-write still yields its recoverable prefix instead of being
//! discarded entirely.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// First token of the format header.
const MAGIC: &str = "karkinos-recovery";
/// Supported on-disk format version.
const VERSION: &str = "1";
/// Marks the end of the metadata header; everything after it is buffer content.
const DELIMITER: &str = "---";
/// Extension used by journal files and recognized by the scanner.
const RECOVERY_EXTENSION: &str = "recovery";

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Snapshot of a dirty buffer persisted for crash recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub original_path: Option<PathBuf>,
    pub title: String,
    pub content: String,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub timestamp_secs: u64,
}

/// Resolves the recovery journal directory.
///
/// Uses `$XDG_STATE_HOME/karkinos/recovery`, then
/// `$HOME/.local/state/karkinos/recovery`, and finally a temporary directory.
pub fn recovery_dir() -> PathBuf {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(state_home).join("karkinos").join("recovery");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("karkinos")
            .join("recovery");
    }
    env::temp_dir().join("karkinos").join("recovery")
}

/// Builds a stable, filesystem-safe journal file name for a tab.
///
/// Saved buffers are keyed by an FNV-1a hash of their path, so reopening the same
/// file always maps to the same journal. Untitled buffers use their tab id.
pub fn journal_filename(path: Option<&Path>, tab_id: usize) -> String {
    match path {
        Some(path) => {
            let hash = fnv1a_64(path.to_string_lossy().as_bytes());
            format!("{hash:016x}.{RECOVERY_EXTENSION}")
        }
        None => format!("untitled_tab_{tab_id}.{RECOVERY_EXTENSION}"),
    }
}

/// Serializes an entry into the journal format.
///
/// The header carries one `key: value` pair per line, followed by the delimiter
/// line and the untouched buffer content.
pub fn serialize_entry(entry: &RecoveryEntry) -> String {
    let mut out = String::with_capacity(entry.content.len() + 128);
    out.push_str(MAGIC);
    out.push(' ');
    out.push_str(VERSION);
    out.push('\n');
    if let Some(path) = &entry.original_path {
        out.push_str("path: ");
        out.push_str(&escape_header(&path.to_string_lossy()));
        out.push('\n');
    }
    out.push_str("title: ");
    out.push_str(&escape_header(&entry.title));
    out.push('\n');
    out.push_str("cursor_line: ");
    out.push_str(&entry.cursor_line.to_string());
    out.push('\n');
    out.push_str("cursor_col: ");
    out.push_str(&entry.cursor_col.to_string());
    out.push('\n');
    out.push_str("timestamp: ");
    out.push_str(&entry.timestamp_secs.to_string());
    out.push('\n');
    out.push_str("content_len: ");
    out.push_str(&entry.content.len().to_string());
    out.push('\n');
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(&entry.content);
    out
}

/// Parses a journal file, tolerating truncated content and corrupt metadata.
///
/// Returns `None` when the header or delimiter is missing, which means there is
/// nothing trustworthy to recover. Malformed individual fields fall back to
/// defaults instead of discarding the buffer content.
pub fn deserialize_entry(data: &str) -> Option<RecoveryEntry> {
    let (header_line, rest) = data.split_once('\n')?;
    let (magic, version) = trim_line_end(header_line).split_once(' ')?;
    if magic != MAGIC || version != VERSION {
        return None;
    }

    let mut original_path = None;
    let mut title = String::new();
    let mut cursor_line = 0usize;
    let mut cursor_col = 0usize;
    let mut timestamp_secs = 0u64;
    let mut declared_len = None;
    let mut cursor = 0usize;
    let mut found_delimiter = false;

    while cursor < rest.len() {
        let newline = rest[cursor..].find('\n')?;
        let line = trim_line_end(&rest[cursor..cursor + newline]);
        cursor += newline + 1;
        if line == DELIMITER {
            found_delimiter = true;
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim_start();
        match key.trim() {
            "path" => {
                let decoded = unescape_header(value);
                original_path = if decoded.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(decoded))
                };
            }
            "title" => title = unescape_header(value),
            "cursor_line" => cursor_line = value.parse().unwrap_or(0),
            "cursor_col" => cursor_col = value.parse().unwrap_or(0),
            "timestamp" => timestamp_secs = value.parse().unwrap_or(0),
            "content_len" => declared_len = value.parse().ok(),
            _ => {}
        }
    }

    if !found_delimiter {
        return None;
    }

    let remainder = &rest[cursor..];
    // A truncated write recovers its prefix; trailing junk past the declared
    // length is ignored. Slicing is skipped when the length splits a code point.
    let content_len = match declared_len {
        Some(len) if len < remainder.len() && remainder.is_char_boundary(len) => len,
        _ => remainder.len(),
    };

    Some(RecoveryEntry {
        original_path,
        title,
        content: remainder[..content_len].to_string(),
        cursor_line,
        cursor_col,
        timestamp_secs,
    })
}

/// Writes an entry atomically and returns the resulting journal path.
///
/// Data goes to a sibling `.tmp` file first and is renamed into place, so a crash
/// never leaves a half-written journal at the destination.
pub fn save_recovery_file(
    dir: &Path,
    filename: &str,
    entry: &RecoveryEntry,
) -> io::Result<PathBuf> {
    let destination = validated_target(dir, filename)?;
    fs::create_dir_all(dir)?;

    let tmp_path = dir.join(format!("{filename}.tmp"));
    let data = serialize_entry(entry);

    let write_result = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp_path, &destination) {
        // Windows cannot rename over an existing file, so replace it explicitly.
        if destination.exists() {
            fs::remove_file(&destination)?;
            fs::rename(&tmp_path, &destination)?;
        } else {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }
    }

    Ok(destination)
}

/// Removes a journal file, succeeding when it is already gone.
pub fn remove_recovery_file(dir: &Path, filename: &str) -> io::Result<()> {
    let target = validated_target(dir, filename)?;
    match fs::remove_file(target) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Collects every valid journal in `dir`, sorted by path.
///
/// Missing directories, unreadable files, and files that fail to parse are
/// skipped rather than surfaced as errors.
pub fn scan_recovery_dir(dir: &Path) -> Vec<(PathBuf, RecoveryEntry)> {
    let mut found = Vec::new();
    let Ok(read_dir) = fs::read_dir(dir) else {
        return found;
    };

    for item in read_dir.flatten() {
        let path = item.path();
        if !is_recovery_file(&path) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if let Some(entry) = deserialize_entry(&String::from_utf8_lossy(&bytes)) {
            found.push((path, entry));
        }
    }

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Deletes every journal file in `dir`, leaving unrelated files untouched.
pub fn clear_recovery_dir(dir: &Path) -> io::Result<()> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for item in read_dir {
        let path = item?.path();
        if !is_recovery_file(&path) {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn validated_target(dir: &Path, filename: &str) -> io::Result<PathBuf> {
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.contains('/')
        || filename.contains('\\')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid recovery journal file name",
        ));
    }
    Ok(dir.join(filename))
}

fn is_recovery_file(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some(RECOVERY_EXTENSION)
}

fn trim_line_end(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn escape_header(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_header(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "karkinos-recovery-test-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn saved_entry() -> RecoveryEntry {
        RecoveryEntry {
            original_path: Some(PathBuf::from("/home/user/project/src/main.rs")),
            title: "main.rs".to_string(),
            content: "fn main() {\n    println!(\"hi\");\n}\n".to_string(),
            cursor_line: 2,
            cursor_col: 5,
            timestamp_secs: 1_700_000_000,
        }
    }

    fn untitled_entry() -> RecoveryEntry {
        RecoveryEntry {
            original_path: None,
            title: "Untitled 3".to_string(),
            content: String::new(),
            cursor_line: 0,
            cursor_col: 0,
            timestamp_secs: 42,
        }
    }

    #[test]
    fn test_roundtrip_saved_entry() {
        let entry = saved_entry();
        let data = serialize_entry(&entry);
        assert_eq!(deserialize_entry(&data), Some(entry));
    }

    #[test]
    fn test_roundtrip_untitled_entry() {
        let entry = untitled_entry();
        let data = serialize_entry(&entry);
        assert_eq!(deserialize_entry(&data), Some(entry));
    }

    #[test]
    fn test_roundtrip_content_with_delimiter_and_crlf() {
        let mut entry = saved_entry();
        entry.title = "weird\\title\nwith newline".to_string();
        entry.original_path = Some(PathBuf::from("/tmp/dir with space/li\nne.rs"));
        entry.content = "---\ncontent_len: 999\n\r\nraw --- text\n".to_string();

        let data = serialize_entry(&entry);
        assert_eq!(deserialize_entry(&data), Some(entry));
    }

    #[test]
    fn test_deserialize_rejects_corrupt_input() {
        assert_eq!(deserialize_entry(""), None);
        assert_eq!(deserialize_entry("not a recovery file\n"), None);
        assert_eq!(deserialize_entry("karkinos-recovery 2\n---\n"), None);
        assert_eq!(deserialize_entry("karkinos-recovery 1\ntitle: x\n"), None);
        assert_eq!(deserialize_entry("karkinos-recovery 1"), None);
    }

    #[test]
    fn test_deserialize_tolerates_partial_content_and_bad_fields() {
        let partial = "karkinos-recovery 1\ntitle: x\ncursor_line: nope\ntimestamp: bad\ncontent: ignored\n---\npartial buf";
        let entry = deserialize_entry(partial).expect("partial journal should recover");
        assert_eq!(entry.title, "x");
        assert_eq!(entry.content, "partial buf");
        assert_eq!(entry.cursor_line, 0);
        assert_eq!(entry.timestamp_secs, 0);
        assert_eq!(entry.original_path, None);
    }

    #[test]
    fn test_deserialize_ignores_trailing_bytes_past_declared_length() {
        let data = "karkinos-recovery 1\ntitle: x\ncontent_len: 5\n---\nhello-junk";
        let entry = deserialize_entry(data).expect("declared length is trusted");
        assert_eq!(entry.content, "hello");
    }

    #[test]
    fn test_journal_filename_is_stable_and_filesystem_safe() {
        let path = Path::new("/home/user/project/src/main.rs");
        let first = journal_filename(Some(path), 7);
        let second = journal_filename(Some(path), 99);
        assert_eq!(first, second, "tab id must not affect saved buffers");
        assert!(first.ends_with(".recovery"));
        assert!(!first.contains('/'));

        let other = journal_filename(Some(Path::new("/home/user/other.rs")), 0);
        assert_ne!(first, other);

        assert_eq!(journal_filename(None, 4), "untitled_tab_4.recovery");
    }

    #[test]
    fn test_recovery_dir_is_absolute() {
        let dir = recovery_dir();
        assert!(dir.is_absolute());
        assert!(dir.ends_with(Path::new("karkinos").join("recovery")));
    }

    #[test]
    fn test_save_writes_atomically_and_overwrites() {
        let dir = TestDir::new();
        let entry = saved_entry();
        let written = save_recovery_file(dir.path(), "journal.recovery", &entry).unwrap();

        assert_eq!(written, dir.path().join("journal.recovery"));
        assert_eq!(
            fs::read_to_string(&written).unwrap(),
            serialize_entry(&entry)
        );
        assert!(
            !dir.path().join("journal.recovery.tmp").exists(),
            "temporary file must be renamed away"
        );

        let mut updated = entry.clone();
        updated.content = "updated buffer".to_string();
        save_recovery_file(dir.path(), "journal.recovery", &updated).unwrap();
        let scan = scan_recovery_dir(dir.path());
        assert_eq!(scan.len(), 1);
        assert_eq!(scan[0].1, updated);
    }

    #[test]
    fn test_save_creates_missing_directories() {
        let dir = TestDir::new();
        let nested = dir.path().join("nested").join("recovery");
        let written = save_recovery_file(&nested, "a.recovery", &untitled_entry()).unwrap();
        assert!(written.is_file());
    }

    #[test]
    fn test_save_rejects_path_traversal_names() {
        let dir = TestDir::new();
        let err = save_recovery_file(dir.path(), "../escape.recovery", &saved_entry()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_remove_deletes_existing_and_tolerates_missing() {
        let dir = TestDir::new();
        save_recovery_file(dir.path(), "one.recovery", &saved_entry()).unwrap();

        remove_recovery_file(dir.path(), "one.recovery").unwrap();
        assert!(!dir.path().join("one.recovery").exists());

        remove_recovery_file(dir.path(), "one.recovery").unwrap();
        remove_recovery_file(dir.path(), "never-written.recovery").unwrap();
    }

    #[test]
    fn test_remove_rejects_path_traversal_names() {
        let dir = TestDir::new();
        let err = remove_recovery_file(dir.path(), "a/b.recovery").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_scan_collects_valid_entries_and_skips_noise() {
        let dir = TestDir::new();
        let first = saved_entry();
        let mut second = untitled_entry();
        second.content = "draft".to_string();
        save_recovery_file(dir.path(), "a.recovery", &first).unwrap();
        save_recovery_file(dir.path(), "b.recovery", &second).unwrap();
        fs::write(dir.path().join("broken.recovery"), "garbage").unwrap();
        fs::write(dir.path().join("notes.txt"), "keep me").unwrap();

        let scan = scan_recovery_dir(dir.path());
        assert_eq!(scan.len(), 2);
        assert_eq!(scan[0].0, dir.path().join("a.recovery"));
        assert_eq!(scan[0].1, first);
        assert_eq!(scan[1].0, dir.path().join("b.recovery"));
        assert_eq!(scan[1].1, second);
    }

    #[test]
    fn test_scan_missing_directory_is_empty() {
        let dir = TestDir::new();
        assert!(scan_recovery_dir(&dir.path().join("absent")).is_empty());
    }

    #[test]
    fn test_clear_removes_only_recovery_files() {
        let dir = TestDir::new();
        save_recovery_file(dir.path(), "a.recovery", &saved_entry()).unwrap();
        save_recovery_file(dir.path(), "b.recovery", &untitled_entry()).unwrap();
        fs::write(dir.path().join("keep.bin"), "data").unwrap();

        clear_recovery_dir(dir.path()).unwrap();
        assert!(scan_recovery_dir(dir.path()).is_empty());
        assert!(dir.path().join("keep.bin").exists());
    }

    #[test]
    fn test_clear_missing_directory_is_ok() {
        let dir = TestDir::new();
        clear_recovery_dir(&dir.path().join("absent")).unwrap();
    }
}
