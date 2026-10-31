//! F8 (Roadmap v0.2.35): estado de Git para la barra de estado y el blame inline.
//!
//! La rama se lee de `.git/HEAD` sin lanzar subprocesos. El blame corre en un
//! worker aparte (`git blame --line-porcelain`), se cachea por archivo y se
//! consulta de forma perezosa sólo cuando el cursor queda quieto.

use std::collections::HashMap;
use std::path::Path;

/// Nombre de la rama actual, o el sha corto si el repo está en detached HEAD.
pub fn read_branch(repo_root: &Path) -> Option<String> {
    let head = std::fs::read_to_string(repo_root.join(".git/HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        let name = reference.rsplit('/').next().unwrap_or(reference);
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    } else {
        let short: String = head.chars().take(7).collect();
        if short.len() == 7 {
            Some(short)
        } else {
            None
        }
    }
}

/// Datos de blame de una línea.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameLine {
    pub author: String,
    pub timestamp: i64,
    pub summary: String,
}

/// Parsea la salida de `git blame --line-porcelain`.
///
/// Porcelain repite la cabecera de cada commit de forma abreviada, así que los
/// metadatos (`author`, `author-time`, `summary`) se guardan por sha y se
/// resuelven cuando aparece la línea de contenido (`\t...`).
pub fn parse_blame_porcelain(output: &str) -> HashMap<usize, BlameLine> {
    let mut by_commit: HashMap<String, BlameLine> = HashMap::new();
    let mut current: Option<(String, usize)> = None;
    let mut map = HashMap::new();

    for line in output.lines() {
        if line.starts_with('\t') {
            if let Some((sha, number)) = &current {
                if let Some(blame) = by_commit.get(sha) {
                    map.insert(*number, blame.clone());
                }
            }
            continue;
        }

        let parts: Vec<&str> = line.split(' ').collect();
        let is_header = parts.len() >= 3
            && parts[0].len() == 40
            && parts[0].chars().all(|c| c.is_ascii_hexdigit());
        if is_header {
            if let Ok(number) = parts[2].parse::<usize>() {
                current = Some((parts[0].to_string(), number));
            }
            continue;
        }

        if let Some((sha, _)) = &current {
            let entry = by_commit.entry(sha.clone()).or_default();
            if let Some(value) = line.strip_prefix("author ") {
                entry.author = value.to_string();
            } else if let Some(value) = line.strip_prefix("author-time ") {
                entry.timestamp = value.parse().unwrap_or(0);
            } else if let Some(value) = line.strip_prefix("summary ") {
                entry.summary = value.to_string();
            }
        }
    }

    map
}

/// Texto tenue para el ghost text: `autor, hace N días • mensaje`.
pub fn format_blame_line(blame: &BlameLine, now_unix: i64) -> String {
    let age = relative_age(blame.timestamp, now_unix);
    if blame.summary.trim().is_empty() {
        format!("{}, {}", blame.author, age)
    } else {
        format!("{}, {} • {}", blame.author, age, blame.summary)
    }
}

/// Antigüedad relativa simple, sin dependencias de fechas.
pub fn relative_age(timestamp: i64, now_unix: i64) -> String {
    let seconds = (now_unix - timestamp).max(0);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3_599 => {
            let minutes = seconds / 60;
            format!("{minutes} minute{} ago", if minutes == 1 { "" } else { "s" })
        }
        3_600..=86_399 => {
            let hours = seconds / 3_600;
            format!("{hours} hour{} ago", if hours == 1 { "" } else { "s" })
        }
        _ => {
            let days = seconds / 86_400;
            format!("{days} day{} ago", if days == 1 { "" } else { "s" })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_is_read_from_head_ref() {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_head_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join(".git")).unwrap();

        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        assert_eq!(read_branch(&dir).as_deref(), Some("x"));

        std::fs::write(dir.join(".git/HEAD"), "0123456789abcdef0123456789abcdef01234567\n").unwrap();
        assert_eq!(read_branch(&dir).as_deref(), Some("0123456"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn branch_is_none_without_git_head() {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_nogit_head_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).ok();
        assert!(read_branch(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn porcelain_parses_repeated_commits() {
        let sha_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let sha_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let output = format!(
            "{sha_a} 1 1 2\nauthor Linus\nauthor-time 1700000000\nsummary fix: buffer\nfilename src/a.rs\n\tlet a = 1;\n{sha_a} 2 2\nauthor Linus\nauthor-time 1700000000\nsummary fix: buffer\nfilename src/a.rs\n\tlet b = 2;\n{sha_b} 3 3 1\nauthor Ada\nauthor-time 1700100000\nsummary feat: search\nfilename src/a.rs\n\tlet c = 3;\n"
        );
        let map = parse_blame_porcelain(&output);
        assert_eq!(map.len(), 3);
        assert_eq!(map[&1].author, "Linus");
        assert_eq!(map[&2].summary, "fix: buffer");
        assert_eq!(map[&3].author, "Ada");
        assert_eq!(map[&3].timestamp, 1_700_100_000);
    }

    #[test]
    fn blame_line_formats_with_relative_age() {
        let blame = BlameLine {
            author: "Linus".to_string(),
            timestamp: 1_700_000_000,
            summary: "fix: buffer boundary".to_string(),
        };
        let now = 1_700_000_000 + 2 * 86_400;
        assert_eq!(
            format_blame_line(&blame, now),
            "Linus, 2 days ago • fix: buffer boundary"
        );
        assert_eq!(relative_age(1_700_000_000, 1_700_000_030), "just now".to_string());
        assert_eq!(relative_age(1_700_000_000, 1_700_000_060), "1 minute ago".to_string());
    }
}
