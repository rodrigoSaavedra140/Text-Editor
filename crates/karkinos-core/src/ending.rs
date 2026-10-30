//! Line ending detection, serialization, and file naming utilities.

use std::path::{Path, PathBuf};

/// Disk line ending style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

/// Detects whether text uniformly uses CRLF line endings.
pub fn detect_line_ending(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    if crlf > 0 && crlf == text.matches('\n').count() {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    }
}

/// Serializes internal LF-normalized text into the target line ending style.
pub fn serialize_line_ending(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => text.to_string(),
        LineEnding::Crlf => text.replace('\n', "\r\n"),
    }
}

/// Resolves an unused default file name (`untitled.txt`, `untitled (1).txt`, etc.).
pub fn default_new_file_path(
    dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    for n in 0..1000u32 {
        let name = if n == 0 {
            "untitled.txt".to_string()
        } else {
            format!("untitled ({n}).txt")
        };
        let path = dir.join(name);
        if !exists(&path) {
            return path;
        }
    }
    dir.join("untitled-file.txt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_detect_line_ending() {
        assert_eq!(detect_line_ending("hello\r\nworld\r\n"), LineEnding::Crlf);
        assert_eq!(detect_line_ending("hello\nworld\n"), LineEnding::Lf);
        assert_eq!(detect_line_ending("hello\r\nworld\n"), LineEnding::Lf);
        assert_eq!(detect_line_ending(""), LineEnding::Lf);
    }

    #[test]
    fn test_serialize_line_ending() {
        let text = "line1\nline2\n";
        assert_eq!(serialize_line_ending(text, LineEnding::Lf), "line1\nline2\n");
        assert_eq!(serialize_line_ending(text, LineEnding::Crlf), "line1\r\nline2\r\n");
    }

    #[test]
    fn test_default_new_file_path_collision_avoidance() {
        let dir = Path::new("/tmp/test_dir");
        fn mk_exists<'a>(
            set: &'a HashSet<&'static str>,
        ) -> impl Fn(&Path) -> bool + 'a {
            move |p: &Path| set.contains(p.file_name().and_then(|n| n.to_str()).unwrap_or(""))
        }

        let mut set = HashSet::new();
        assert_eq!(default_new_file_path(dir, &mk_exists(&set)), PathBuf::from("/tmp/test_dir/untitled.txt"));

        set.insert("untitled.txt");
        assert_eq!(default_new_file_path(dir, &mk_exists(&set)), PathBuf::from("/tmp/test_dir/untitled (1).txt"));

        set.insert("untitled (1).txt");
        set.insert("untitled (2).txt");
        assert_eq!(default_new_file_path(dir, &mk_exists(&set)), PathBuf::from("/tmp/test_dir/untitled (3).txt"));
    }
}
