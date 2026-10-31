use std::path::{Path, PathBuf};

/// Configures background processes so they do not spawn or flash an unwanted
/// console window on Windows.
pub fn configure_background_command(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// Converts a local filesystem path to a standard LSP RFC-3986 `file://` URI.
///
/// Ensures Windows drive letters (`C:\path`) and Unix paths (`/path`) are formatted
/// correctly with forward slashes and standard URI schemas (`file:///C:/...` vs `file:///...`).
pub fn path_to_uri(path: &Path) -> String {
    let lossy = path.to_string_lossy();
    let normalized = lossy.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{}", normalized)
    } else if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        format!("file:///{}", normalized)
    } else {
        format!("file://{}", normalized)
    }
}

/// Converts an LSP RFC-3986 `file://` URI back into a platform `PathBuf`.
///
/// Accurately strips Windows drive letter prefixes (`file:///C:/path` -> `C:\path` or `C:/path`).
pub fn uri_to_path(uri: &str) -> PathBuf {
    let without_scheme = uri.strip_prefix("file://").unwrap_or(uri);

    // On Windows or when parsing Windows drive URI: /C:/... -> C:/...
    if without_scheme.starts_with('/') && without_scheme.len() >= 3 {
        let bytes = without_scheme.as_bytes();
        if bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
            return PathBuf::from(&without_scheme[1..]);
        }
    }

    PathBuf::from(without_scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_uri_unix() {
        let path = Path::new("/home/user/project/src/main.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///home/user/project/src/main.rs");
        assert_eq!(uri_to_path(&uri), PathBuf::from("/home/user/project/src/main.rs"));
    }

    #[test]
    fn test_path_to_uri_windows_drive() {
        let path = Path::new(r"C:\Users\Developer\Text-Editor\src\main.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///C:/Users/Developer/Text-Editor/src/main.rs");
        assert_eq!(uri_to_path(&uri), PathBuf::from("C:/Users/Developer/Text-Editor/src/main.rs"));
    }

    #[test]
    fn test_path_to_uri_relative() {
        let path = Path::new("src/main.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file://src/main.rs");
        assert_eq!(uri_to_path(&uri), PathBuf::from("src/main.rs"));
    }
}
