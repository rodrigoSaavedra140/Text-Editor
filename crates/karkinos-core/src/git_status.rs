//! F8 (Roadmap v0.2.35): estado de Git para la barra de estado y el blame inline.
//!
//! La rama se lee de `.git/HEAD` sin lanzar subprocesos. El blame corre en un
//! worker aparte (`git blame --line-porcelain`), se cachea por archivo y se
//! consulta de forma perezosa sólo cuando el cursor queda quieto.
//!
//! v0.2.42: la pestaña Source Control usa `git status --porcelain=v1` para
//! listar cambios y `git add` / `git commit` para operar sobre el índice.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

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

/// v0.2.42: clasificación de un archivo con cambios pendientes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitStatusKind {
    Modified,
    Added,
    Deleted,
    Untracked,
    Renamed,
    /// Cualquier otro código de `git status` (conflictos, typechange, etc.).
    Other(String),
}

/// v0.2.42: un archivo con cambios pendientes según `git status --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusEntry {
    pub path: std::path::PathBuf,
    pub rel_path: String,
    pub staged: bool,
    pub kind: GitStatusKind,
}

/// v0.2.42: parsea la salida de `git status --porcelain=v1`.
///
/// Formato por línea: `XY<espacio>ruta`. El primer carácter es el estado del
/// índice (staged) y el segundo el del árbol de trabajo. `??` es un archivo
/// sin seguimiento y `!!` uno ignorado (se descarta). En renombres
/// (`R  viejo -> nuevo`) se conserva sólo la ruta nueva.
pub fn parse_porcelain_status(output: &str, repo_root: &Path) -> Vec<GitStatusEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }

        if let Some(rel_path) = line.strip_prefix("?? ") {
            let rel_path = porcelain_path(rel_path);
            entries.push(GitStatusEntry {
                path: repo_root.join(&rel_path),
                rel_path,
                staged: false,
                kind: GitStatusKind::Untracked,
            });
            continue;
        }
        if line.starts_with("!! ") {
            continue;
        }

        let mut chars = line.chars();
        let (Some(index_status), Some(worktree_status), Some(' ')) =
            (chars.next(), chars.next(), chars.next())
        else {
            continue;
        };

        let raw_path = chars.as_str();
        let raw_path = if index_status == 'R' {
            raw_path
                .rsplit_once(" -> ")
                .map(|(_, new_path)| new_path)
                .unwrap_or(raw_path)
        } else {
            raw_path
        };
        let rel_path = porcelain_path(raw_path);

        let staged = matches!(index_status, 'M' | 'A' | 'D' | 'R');
        let fallback_code = if index_status == ' ' {
            worktree_status
        } else {
            index_status
        };
        let kind = status_kind(index_status)
            .or_else(|| status_kind(worktree_status))
            .unwrap_or_else(|| GitStatusKind::Other(fallback_code.to_string()));

        entries.push(GitStatusEntry {
            path: repo_root.join(&rel_path),
            rel_path,
            staged,
            kind,
        });
    }

    entries
}

fn status_kind(code: char) -> Option<GitStatusKind> {
    match code {
        'M' => Some(GitStatusKind::Modified),
        'A' => Some(GitStatusKind::Added),
        'D' => Some(GitStatusKind::Deleted),
        'R' => Some(GitStatusKind::Renamed),
        _ => None,
    }
}

/// Normaliza el path de porcelain: Git lo entrecomilla y escapa cuando
/// contiene caracteres especiales.
fn porcelain_path(raw: &str) -> String {
    let raw = raw.trim_end_matches('\r');
    if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
        return raw.to_string();
    }

    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            out.push(match character {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            out.push(character);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// v0.2.42: consulta `git status --porcelain=v1` en `repo_root`.
pub fn query_git_status(repo_root: &Path) -> Result<Vec<GitStatusEntry>, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("Failed to run git status: {error}"))?;

    if !output.status.success() {
        return Err(git_error("git status", &output));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_porcelain_status(&stdout, repo_root))
}

/// v0.2.42: crea un commit con `message`. Un mensaje vacío se rechaza antes de
/// lanzar el subproceso.
pub fn execute_git_commit(repo_root: &Path, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Commit message cannot be empty".to_string());
    }

    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("Failed to run git commit: {error}"))?;

    if !output.status.success() {
        return Err(git_error("git commit", &output));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// v0.2.42: agrega todos los cambios al índice (`git add -A`).
pub fn execute_git_stage_all(repo_root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("Failed to run git add: {error}"))?;

    if !output.status.success() {
        return Err(git_error("git add", &output));
    }
    Ok(())
}

/// v0.2.42: agrega un único archivo al índice.
pub fn execute_git_stage_path(repo_root: &Path, path: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["add", "--"])
        .arg(path)
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("Failed to run git add: {error}"))?;

    if !output.status.success() {
        return Err(git_error("git add", &output));
    }
    Ok(())
}

/// v0.2.44: tipo de cada línea de un diff unificado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Metadatos: `diff --git`, `index`, `---`, `+++`, renombres.
    Header,
    /// Cabecera de hunk (`@@ -a,b +c,d @@`).
    HunkHeader,
    /// Línea agregada (`+`).
    Added,
    /// Línea eliminada (`-`).
    Deleted,
    /// Línea sin cambios, o espacio en blanco.
    Context,
}

/// v0.2.44: una línea de diff junto a su clasificación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

/// v0.2.44: clasifica la salida de `git diff` línea por línea.
///
/// El orden de las comprobaciones importa: `+++ `/`--- ` también empiezan con
/// `+`/`-`, así que los encabezados se resuelven antes que agregados y
/// borrados.
pub fn parse_unified_diff(diff: &str) -> Vec<ParsedDiffLine> {
    diff.lines()
        .map(|line| {
            let kind = if is_diff_header(line) {
                DiffLineKind::Header
            } else if line.starts_with("@@") {
                DiffLineKind::HunkHeader
            } else if line.starts_with('+') {
                DiffLineKind::Added
            } else if line.starts_with('-') {
                DiffLineKind::Deleted
            } else {
                DiffLineKind::Context
            };

            ParsedDiffLine {
                kind,
                text: line.to_string(),
            }
        })
        .collect()
}

fn is_diff_header(line: &str) -> bool {
    const PREFIXES: [&str; 7] = [
        "diff --git",
        "index ",
        "--- ",
        "+++ ",
        "similarity index",
        "rename from",
        "rename to",
    ];
    PREFIXES.iter().any(|prefix| line.starts_with(prefix))
}

/// v0.2.44: consulta `git -C <repo_root> diff [--cached] [-- <rel_path>]`.
///
/// Devuelve el stdout tal cual; un diff vacío produce `Ok(String::new())` para
/// que el visor pueda mostrar que no hay cambios.
pub fn query_git_diff(
    repo_root: &Path,
    rel_path: Option<&Path>,
    staged: bool,
) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_root).arg("diff");

    if staged {
        command.arg("--cached");
    }
    if let Some(rel_path) = rel_path {
        command.arg("--").arg(rel_path);
    }

    let output = command
        .output()
        .map_err(|error| format!("Failed to run git diff: {error}"))?;

    if !output.status.success() {
        return Err(git_error("git diff", &output));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_error(action: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("{action} failed")
    } else {
        format!("{action} failed: {detail}")
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

    // v0.2.42: parseo de `git status --porcelain=v1` para Source Control.

    #[test]
    fn porcelain_parses_untracked_files() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status("?? nuevo.txt\n", root);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rel_path, "nuevo.txt");
        assert_eq!(entries[0].path, root.join("nuevo.txt"));
        assert!(!entries[0].staged);
        assert_eq!(entries[0].kind, GitStatusKind::Untracked);
    }

    #[test]
    fn porcelain_parses_modified_staged_and_unstaged() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status(" M src/main.rs\nM  src/lib.rs\n", root);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, GitStatusKind::Modified);
        assert!(!entries[0].staged, "el cambio vive sólo en el worktree");
        assert_eq!(entries[1].kind, GitStatusKind::Modified);
        assert!(entries[1].staged, "el cambio vive en el índice");
    }

    #[test]
    fn porcelain_parses_added_staged_and_unstaged() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status("A  src/added.rs\n A src/worktree.rs\n", root);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, GitStatusKind::Added);
        assert!(entries[0].staged);
        assert_eq!(entries[1].kind, GitStatusKind::Added);
        assert!(!entries[1].staged);
    }

    #[test]
    fn porcelain_parses_deleted_staged_and_unstaged() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status("D  viejo.rs\n D borrado.rs\n", root);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, GitStatusKind::Deleted);
        assert!(entries[0].staged);
        assert_eq!(entries[1].kind, GitStatusKind::Deleted);
        assert!(!entries[1].staged);
        assert_eq!(entries[1].rel_path, "borrado.rs");
    }

    #[test]
    fn porcelain_parses_renames_with_the_new_path() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status("R  viejo.rs -> nuevo.rs\n", root);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, GitStatusKind::Renamed);
        assert!(entries[0].staged);
        assert_eq!(entries[0].rel_path, "nuevo.rs");
        assert_eq!(entries[0].path, root.join("nuevo.rs"));
    }

    #[test]
    fn porcelain_staged_and_unstaged_in_the_same_file() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status("MM mixto.rs\n", root);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, GitStatusKind::Modified);
        assert!(entries[0].staged, "el primer código marca el índice");
    }

    #[test]
    fn porcelain_keeps_other_codes_and_quoted_paths() {
        let root = Path::new("repo");
        let entries = parse_porcelain_status("UU conflicto.rs\n?? \"con espacio.txt\"\n", root);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, GitStatusKind::Other("U".to_string()));
        assert!(!entries[0].staged);
        assert_eq!(entries[1].rel_path, "con espacio.txt");
        assert_eq!(entries[1].path, root.join("con espacio.txt"));
    }

    #[test]
    fn porcelain_ignores_empty_output_and_ignored_files() {
        let root = Path::new("repo");
        assert!(parse_porcelain_status("", root).is_empty());
        assert!(parse_porcelain_status("!! target/\n", root).is_empty());
    }

    #[test]
    fn execute_git_commit_rejects_a_blank_message_before_spawning_git() {
        let error = execute_git_commit(Path::new("."), "   ").unwrap_err();
        assert_eq!(error, "Commit message cannot be empty");
    }

    // v0.2.44: motor del visor de diff.

    #[test]
    fn test_parse_unified_diff() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index 1234567..89abcde 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"hola\");
+    println!(\"mundo\");
+    let extra = 1;
 }
";
        let parsed = parse_unified_diff(diff);
        let kinds: Vec<DiffLineKind> = parsed.iter().map(|line| line.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::HunkHeader,
                DiffLineKind::Context,
                DiffLineKind::Deleted,
                DiffLineKind::Added,
                DiffLineKind::Added,
                DiffLineKind::Context,
            ]
        );

        assert_eq!(parsed[0].text, "diff --git a/src/main.rs b/src/main.rs");
        assert_eq!(parsed[5].text, " fn main() {");
        assert_eq!(parsed[6].text, "-    println!(\"hola\");");
        assert_eq!(parsed[7].text, "+    println!(\"mundo\");");
        assert!(parse_unified_diff("").is_empty());
    }

    #[test]
    fn test_parse_unified_diff_classifies_rename_headers() {
        let diff = "\
diff --git a/viejo.rs b/nuevo.rs
similarity index 95%
rename from viejo.rs
rename to nuevo.rs
@@ -1 +1 @@
-let a = 1;
+let a = 2;
";
        let kinds: Vec<DiffLineKind> = parse_unified_diff(diff)
            .iter()
            .map(|line| line.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::HunkHeader,
                DiffLineKind::Deleted,
                DiffLineKind::Added,
            ]
        );
    }

    #[test]
    fn test_query_git_diff_in_temp_repo() {
        let dir = temp_repo_dir("query");
        run_git(&dir, &["init", "-q"]);
        std::fs::write(dir.join("nota.txt"), "hola\n").unwrap();
        run_git(&dir, &["add", "nota.txt"]);
        run_git(&dir, &["commit", "-q", "-m", "initial"]);

        let rel_path = Path::new("nota.txt");
        assert_eq!(
            query_git_diff(&dir, Some(rel_path), false).unwrap(),
            "",
            "un repo limpio no produce diff"
        );

        std::fs::write(dir.join("nota.txt"), "mundo\n").unwrap();

        let unstaged = query_git_diff(&dir, Some(rel_path), false).expect("diff del worktree");
        assert!(unstaged.contains("-hola\n"), "{unstaged}");
        assert!(unstaged.contains("+mundo\n"), "{unstaged}");
        assert!(unstaged.contains("--- "), "{unstaged}");
        assert!(unstaged.contains("+++ "), "{unstaged}");

        let parsed = parse_unified_diff(&unstaged);
        assert!(parsed
            .iter()
            .any(|line| line.kind == DiffLineKind::Deleted && line.text == "-hola"));
        assert!(parsed
            .iter()
            .any(|line| line.kind == DiffLineKind::Added && line.text == "+mundo"));
        assert!(parsed
            .iter()
            .any(|line| line.kind == DiffLineKind::HunkHeader));

        run_git(&dir, &["add", "nota.txt"]);
        assert_eq!(
            query_git_diff(&dir, Some(rel_path), false).unwrap(),
            "",
            "el cambio ya está en el índice"
        );

        let staged = query_git_diff(&dir, Some(rel_path), true).expect("diff del índice");
        assert!(staged.contains("+mundo\n"), "{staged}");

        let whole_repo = query_git_diff(&dir, None, true).expect("diff sin filtro de ruta");
        assert!(whole_repo.contains("nota.txt"), "{whole_repo}");

        std::fs::remove_dir_all(&dir).ok();
    }

    fn temp_repo_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_diff_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Git aislado de la configuración global que pueda romper el test
    /// (firma de commits, nombre de rama, hooks).
    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=Karkinos Test",
                "-c",
                "user.email=test@karkinos.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("no se pudo lanzar git");

        assert!(
            output.status.success(),
            "git {args:?} falló: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
