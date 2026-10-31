//! F6 (Roadmap v0.2.32): búsqueda global de texto en el workspace.
//!
//! El motor corre en un hilo propio y emite resultados por archivo en
//! streaming, así la UI nunca se bloquea. El recorrido usa `std::fs` (sin
//! dependencias extra) y salta directorios pesados como `target/` o `.git/`.
//! Soporta mayúsculas/minúsculas, palabra completa y expresiones regulares.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

/// Directorios que el recorrido nunca entra.
pub const DEFAULT_EXCLUDED_DIRS: &[&str] = &[
    "target",
    ".git",
    ".codegraph",
    "node_modules",
    "dist",
    "build",
    ".idea",
    ".vscode",
];

/// Tope por archivo: no tiene sentido buscar dentro de binarios o dumps.
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// Parámetros de una búsqueda.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub case_sensitive: bool,
    pub match_word: bool,
    pub use_regex: bool,
    /// Tope de coincidencias por archivo (500 por defecto en la UI).
    pub max_results: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            case_sensitive: false,
            match_word: false,
            use_regex: false,
            max_results: 500,
        }
    }
}

impl SearchQuery {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_string(),
            max_results: 500,
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
    }

    /// Valida la expresión regular, si el modo regex está activo.
    pub fn validate(&self) -> Result<(), String> {
        if !self.use_regex || self.is_empty() {
            return Ok(());
        }
        regex::RegexBuilder::new(&self.query)
            .case_insensitive(!self.case_sensitive)
            .build()
            .map(|_| ())
            .map_err(|e| format!("Invalid regex: {e}"))
    }

    fn compiled(&self) -> Option<regex::Regex> {
        if !self.use_regex || self.is_empty() {
            return None;
        }
        regex::RegexBuilder::new(&self.query)
            .case_insensitive(!self.case_sensitive)
            .build()
            .ok()
    }
}

/// Una coincidencia dentro de un renglón.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// 1-based: es el número que se muestra en la UI.
    pub line_number: usize,
    /// 0-based, en CARACTERES (el editor indexa por char, no por byte).
    pub col_start: usize,
    pub col_end: usize,
    pub line_content: String,
}

/// Todas las coincidencias de un archivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    pub path: PathBuf,
    /// Ruta relativa a la raíz, con separadores `/`, para mostrar.
    pub relative_display: String,
    pub matches: Vec<SearchMatch>,
}

/// Evento emitido por el worker mientras recorre.
#[derive(Debug, Clone)]
pub enum SearchEvent {
    File(FileMatches),
    Done {
        files_scanned: usize,
        matches: usize,
    },
}

struct SearchRequest {
    generation: u64,
    root: PathBuf,
    query: SearchQuery,
}

/// Motor de búsqueda en segundo plano. Un pedido nuevo descarta la cola
/// pendiente: tipear rápido no acumula recorridos.
pub struct WorkspaceSearch {
    request_tx: Option<mpsc::Sender<SearchRequest>>,
    result_rx: Option<mpsc::Receiver<(u64, SearchEvent)>>,
    _handle: Option<thread::JoinHandle<()>>,
}

impl Default for WorkspaceSearch {
    fn default() -> Self {
        Self::spawn()
    }
}

impl WorkspaceSearch {
    pub fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<SearchRequest>();
        let (result_tx, result_rx) = mpsc::channel::<(u64, SearchEvent)>();

        let handle = thread::Builder::new()
            .name("workspace-search".to_string())
            .spawn(move || {
                for request in request_rx {
                    let generation = request.generation;
                    run_search(&request.root, &request.query, generation, &result_tx);
                }
            })
            .ok();

        Self {
            request_tx: Some(request_tx),
            result_rx: Some(result_rx),
            _handle: handle,
        }
    }

    pub fn request(&self, generation: u64, root: &Path, query: SearchQuery) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(SearchRequest {
                generation,
                root: root.to_path_buf(),
                query,
            });
        }
    }

    pub fn try_recv(&self) -> Option<(u64, SearchEvent)> {
        self.result_rx.as_ref()?.try_recv().ok()
    }
}

fn run_search(
    root: &Path,
    query: &SearchQuery,
    generation: u64,
    tx: &mpsc::Sender<(u64, SearchEvent)>,
) {
    if query.is_empty() || query.validate().is_err() {
        let _ = tx.send((
            generation,
            SearchEvent::Done {
                files_scanned: 0,
                matches: 0,
            },
        ));
        return;
    }

    let mut files_scanned = 0usize;
    let mut total_matches = 0usize;
    for file in walk_files(root) {
        let relative = relative_display(root, &file);
        let Some(matches) = search_file_matches(&file, query) else {
            continue;
        };
        files_scanned += 1;
        if matches.is_empty() {
            continue;
        }
        total_matches += matches.len();
        let payload = FileMatches {
            path: file,
            relative_display: relative,
            matches,
        };
        if tx.send((generation, SearchEvent::File(payload))).is_err() {
            return;
        }
    }
    let _ = tx.send((generation, SearchEvent::Done { files_scanned, matches: total_matches }));
}

/// Coincidencias dentro del texto completo de un archivo.
pub fn search_in_text(text: &str, query: &SearchQuery) -> Vec<SearchMatch> {
    if query.is_empty() || query.validate().is_err() {
        return Vec::new();
    }
    let regex = query.compiled();
    let needle_lower = query.query.to_lowercase();
    let mut out = Vec::new();

    for (idx, line) in text.lines().enumerate() {
        let ranges: Vec<(usize, usize)> = match &regex {
            Some(re) => re
                .find_iter(line)
                .filter(|m| !query.match_word || is_whole_word(line, m.start(), m.end()))
                .map(|m| (m.start(), m.end()))
                .collect(),
            None => find_literal_ranges(line, &needle_lower, query),
        };
        for (start_byte, end_byte) in ranges {
            if out.len() >= query.max_results {
                return out;
            }
            let col_start = line[..start_byte].chars().count();
            let col_end = col_start + line[start_byte..end_byte].chars().count();
            out.push(SearchMatch {
                line_number: idx + 1,
                col_start,
                col_end,
                line_content: line.to_string(),
            });
        }
    }
    out
}

fn find_literal_ranges(line: &str, needle_lower: &str, query: &SearchQuery) -> Vec<(usize, usize)> {
    if needle_lower.is_empty() {
        return Vec::new();
    }
    let haystack = if query.case_sensitive {
        line.to_string()
    } else {
        line.to_lowercase()
    };
    let mut ranges = Vec::new();
    let mut from = 0usize;
    while let Some(byte_pos) = haystack[from..].find(needle_lower) {
        let start = from + byte_pos;
        let end = start + needle_lower.len();
        let accepted = !query.match_word || is_whole_word(line, start, end);
        if accepted {
            ranges.push((start, end));
        }
        // Avanzar al menos un byte para no quedar en loop con una coincidencia vacía.
        from = end.max(start + 1);
        if from >= haystack.len() {
            break;
        }
    }
    ranges
}

/// Palabra completa: los bordes no son alfanuméricos ni `_`. Aplica en bytes
/// pero solo mira fronteras ASCII/Unicode de un carácter, lo cual es seguro
/// porque los límites de una coincidencia siempre caen en fronteras de char.
fn is_whole_word(line: &str, start: usize, end: usize) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[end..].chars().next();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    !before.map_or(false, is_word) && !after.map_or(false, is_word)
}

fn search_file_matches(path: &Path, query: &SearchQuery) -> Option<Vec<SearchMatch>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if !is_probably_text(&bytes) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(search_in_text(&text, query))
}

/// Heurística de texto: si hay un NUL en el primer bloque, es binario.
pub fn is_probably_text(bytes: &[u8]) -> bool {
    !bytes.iter().take(8192).any(|&b| b == 0)
}

fn relative_display(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Recorre el árbol saltando los directorios excluidos.
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_into(root, &mut out);
    out
}

fn walk_into(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if DEFAULT_EXCLUDED_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_into(&path, out);
        } else if file_type.is_file() {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_search_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn finds_every_occurrence_in_a_file() {
        let text = "let foo = 1;\nfoo();\n// foo\n";
        let matches = search_in_text(text, &SearchQuery::new("foo"));
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].line_number, 1);
        assert_eq!((matches[0].col_start, matches[0].col_end), (4, 7));
        assert_eq!(matches[1].line_number, 2);
        assert_eq!(matches[2].line_content, "// foo");
    }

    #[test]
    fn case_insensitive_by_default_and_sensitive_on_request() {
        let text = "Foo foo FOO\n";
        let insensitive = search_in_text(text, &SearchQuery::new("foo"));
        assert_eq!(insensitive.len(), 3);

        let mut query = SearchQuery::new("foo");
        query.case_sensitive = true;
        assert_eq!(search_in_text(text, &query).len(), 1);
    }

    #[test]
    fn whole_word_skips_substrings() {
        let text = "foobar foo _foo foo_\n";
        let mut query = SearchQuery::new("foo");
        query.match_word = true;
        let matches = search_in_text(text, &query);
        assert_eq!(matches.len(), 1, "{matches:?}");
        assert_eq!(matches[0].col_start, 7);
    }

    #[test]
    fn regex_mode_matches_patterns() {
        let text = "let x = 42;\nlet y = abc;\n";
        let mut query = SearchQuery::new(r"\d+");
        query.use_regex = true;
        let matches = search_in_text(text, &query);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_content, "let x = 42;");
        assert_eq!((matches[0].col_start, matches[0].col_end), (8, 10));
    }

    #[test]
    fn invalid_regex_is_rejected_without_matches() {
        let mut query = SearchQuery::new("([a-z");
        query.use_regex = true;
        assert!(query.validate().is_err());
        assert!(search_in_text("([a-z", &query).is_empty());
    }

    #[test]
    fn max_results_caps_the_output() {
        let text = "a\na\na\na\na\n";
        let mut query = SearchQuery::new("a");
        query.max_results = 2;
        assert_eq!(search_in_text(text, &query).len(), 2);
    }

    #[test]
    fn binary_content_is_not_text() {
        assert!(!is_probably_text(b"abc\0def"));
        assert!(is_probably_text(b"abc\ndef"));
    }

    #[test]
    fn walker_skips_excluded_directories() {
        let root = temp_dir("walk");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}").unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main").unwrap();

        let files = walk_files(&root);
        let relative: Vec<String> = files
            .iter()
            .map(|f| relative_display(&root, f))
            .collect();
        assert!(relative.contains(&"src/main.rs".to_string()), "{relative:?}");
        assert!(!relative.iter().any(|r| r.starts_with("target/")), "{relative:?}");
        assert!(!relative.iter().any(|r| r.starts_with(".git/")), "{relative:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn worker_streams_matches_for_the_workspace() {
        let root = temp_dir("worker");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn alpha() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "fn beta() {}\n").unwrap();
        std::fs::write(root.join("notes.bin"), b"alph\0a").unwrap();

        let engine = WorkspaceSearch::spawn();
        engine.request(7, &root, SearchQuery::new("fn a"));

        let mut files = Vec::new();
        let mut done = None;
        for _ in 0..200 {
            if let Some((generation, event)) = engine.try_recv() {
                assert_eq!(generation, 7, "la generación viaja con el evento");
                match event {
                    SearchEvent::File(f) => files.push(f),
                    SearchEvent::Done { .. } => {
                        done = Some(());
                        break;
                    }
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        assert!(done.is_some(), "el worker tiene que cerrar con Done");
        let a = files
            .iter()
            .find(|f| f.relative_display == "src/a.rs")
            .expect("coincidencia en src/a.rs");
        assert_eq!(a.matches[0].line_number, 1);
        assert!(
            !files.iter().any(|f| f.relative_display == "notes.bin"),
            "los binarios se descartan"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_query_returns_nothing() {
        assert!(search_in_text("foo", &SearchQuery::new("   ")).is_empty());
    }
}
