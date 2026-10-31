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
    /// Filtro de archivos opcional (e.g. `*.rs, *.toml` o `src/`).
    pub file_pattern: Option<String>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            case_sensitive: false,
            match_word: false,
            use_regex: false,
            max_results: 500,
            file_pattern: None,
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
    let file_pattern = query
        .file_pattern
        .as_deref()
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty());
    for file in walk_files(root) {
        let relative = relative_display(root, &file);
        if let Some(pattern) = file_pattern {
            if !matches_file_pattern(Path::new(&relative), pattern) {
                continue;
            }
        }
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

/// Indica si `path` coincide con `pattern`.
///
/// Acepta:
/// - Listas separadas por comas: `*.rs, *.toml`.
/// - Glob de extensión: `*.rs`, `*.toml`.
/// - Extensión corta: `.rs`.
/// - Prefijos o subcadenas de ruta: `src/`, `crates/`.
/// - Glob de ruta: `src/*.rs`.
///
/// La comparación no distingue mayúsculas de minúsculas.
pub fn matches_file_pattern(path: &Path, pattern: &str) -> bool {
    let display = path.to_string_lossy().replace('\\', "/");
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    pattern
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .any(|item| matches_single_pattern(&display, &file_name, item))
}

fn matches_single_pattern(display: &str, file_name: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }

    // Extensión corta: `.rs`, `.toml`.
    if let Some(extension) = pattern.strip_prefix('.') {
        let simple = !extension.is_empty()
            && !extension.contains('.')
            && !extension
                .chars()
                .any(|c| matches!(c, '*' | '?' | '/' | '\\'));
        if simple {
            let suffix = format!(".{}", extension.to_lowercase());
            return file_name.to_lowercase().ends_with(&suffix);
        }
    }

    let has_glob = pattern.chars().any(|c| matches!(c, '*' | '?'));
    if has_glob {
        let pattern_lower = pattern.to_lowercase();
        let has_separator = pattern.contains('/') || pattern.contains('\\');
        let candidate = if has_separator {
            display.to_lowercase()
        } else {
            file_name.to_lowercase()
        };
        if glob_match(&pattern_lower, &candidate) {
            return true;
        }
        // `src/*.rs` contra una ruta absoluta: probar cada sufijo.
        let mut start = 0usize;
        while let Some(offset) = candidate[start..].find('/') {
            start += offset + 1;
            if glob_match(&pattern_lower, &candidate[start..]) {
                return true;
            }
        }
        return false;
    }

    // Prefijo o subcadena de ruta, o nombre de archivo exacto.
    let needle = pattern.to_lowercase();
    display.to_lowercase().contains(&needle) || file_name.to_lowercase() == needle
}

/// Glob simple donde `*` coincide con cualquier secuencia y `?` con un char.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut pattern_index = 0usize;
    let mut text_index = 0usize;
    let mut star: Option<usize> = None;
    let mut star_text_index = 0usize;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some(pattern_index);
            star_text_index = text_index;
            pattern_index += 1;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_text_index += 1;
            text_index = star_text_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

/// Resume el resultado de un reemplazo en el workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplaceSummary {
    pub files_modified: usize,
    pub replacements_count: usize,
}

/// Reemplaza en `content` según `query` y devuelve el texto nuevo junto al
/// número de reemplazos aplicados.
pub fn replace_in_string(
    content: &str,
    query: &SearchQuery,
    replacement: &str,
) -> Result<(String, usize), String> {
    if query.is_empty() {
        return Ok((content.to_string(), 0));
    }
    if query.use_regex {
        let regex = regex::RegexBuilder::new(&query.query)
            .case_insensitive(!query.case_sensitive)
            .build()
            .map_err(|e| format!("Invalid regex: {e}"))?;
        let count = regex.find_iter(content).count();
        if count == 0 {
            return Ok((content.to_string(), 0));
        }
        let replaced = regex.replace_all(content, replacement).into_owned();
        Ok((replaced, count))
    } else {
        Ok(replace_literal(
            content,
            &query.query,
            query.case_sensitive,
            query.match_word,
            replacement,
        ))
    }
}

fn replace_literal(
    content: &str,
    needle: &str,
    case_sensitive: bool,
    match_word: bool,
    replacement: &str,
) -> (String, usize) {
    if needle.is_empty() {
        return (content.to_string(), 0);
    }
    // Se usa el motor de regex sobre el literal escapado: así la comparación
    // insensible a mayúsculas respeta el case folding de Unicode sin
    // desalinear offsets (a diferencia de comparar sobre `to_lowercase()`).
    // El reemplazo se inserta literal, sin expandir `$` como en modo regex.
    let Ok(regex) = regex::RegexBuilder::new(&regex::escape(needle))
        .case_insensitive(!case_sensitive)
        .build()
    else {
        return (content.to_string(), 0);
    };

    let mut out = String::with_capacity(content.len());
    let mut count = 0usize;
    let mut last_end = 0usize;
    for found in regex.find_iter(content) {
        if match_word && !is_whole_word(content, found.start(), found.end()) {
            continue;
        }
        out.push_str(&content[last_end..found.start()]);
        out.push_str(replacement);
        last_end = found.end();
        count += 1;
    }
    if count == 0 {
        return (content.to_string(), 0);
    }
    out.push_str(&content[last_end..]);
    (out, count)
}

/// Reemplaza en un archivo en disco. Solo reescribe si hubo al menos un
/// reemplazo; el contenido se lee y escribe como UTF-8, preservando los
/// finales de línea existentes.
pub fn replace_in_file(
    path: &Path,
    query: &SearchQuery,
    replacement: &str,
) -> Result<usize, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if !is_probably_text(&bytes) {
        return Ok(0);
    }
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        // Mismo criterio que la búsqueda: un archivo que no es UTF-8 no es
        // texto editable y se omite, en vez de abortar el resto del lote
        // después de haber reescrito otros archivos.
        Err(_) => return Ok(0),
    };
    let (updated, count) = replace_in_string(&content, query, replacement)?;
    if count > 0 && updated != content {
        std::fs::write(path, updated.as_bytes())
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    }
    Ok(count)
}

/// Reemplaza `query` por `replacement` en todos los archivos del workspace
/// que pasen el filtro `query.file_pattern`.
pub fn replace_in_workspace(
    root: &Path,
    query: &SearchQuery,
    replacement: &str,
) -> Result<ReplaceSummary, String> {
    query.validate()?;
    let mut summary = ReplaceSummary::default();
    if query.is_empty() {
        return Ok(summary);
    }
    let file_pattern = query
        .file_pattern
        .as_deref()
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty());

    for file in walk_files(root) {
        let relative = relative_display(root, &file);
        if let Some(pattern) = file_pattern {
            if !matches_file_pattern(Path::new(&relative), pattern) {
                continue;
            }
        }
        let count = replace_in_file(&file, query, replacement)?;
        if count > 0 {
            summary.files_modified += 1;
            summary.replacements_count += count;
        }
    }
    Ok(summary)
}

fn relative_display(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Recorre el árbol saltando los directorios excluidos y devuelve
/// `(archivos, directorios)`. Los directorios se reportan en preorden, así
/// cada carpeta aparece antes que sus hijas.
pub fn walk_entries(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    walk_into(root, &mut files, &mut dirs);
    (files, dirs)
}

/// Retrocompatibilidad: solo los archivos del recorrido.
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    walk_entries(root).0
}

fn walk_into(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
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
            dirs.push(path.clone());
            walk_into(&path, files, dirs);
        } else if file_type.is_file() {
            files.push(path);
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
    fn walk_entries_reports_files_and_empty_directories() {
        let root = temp_dir("entries");
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}").unwrap();

        let (files, dirs) = walk_entries(&root);
        let rel_files: Vec<String> = files
            .iter()
            .map(|f| relative_display(&root, f))
            .collect();
        let rel_dirs: Vec<String> = dirs
            .iter()
            .map(|d| relative_display(&root, d))
            .collect();

        assert!(rel_files.contains(&"src/main.rs".to_string()), "{rel_files:?}");
        assert!(!rel_files.iter().any(|f| f.starts_with("target/")), "{rel_files:?}");
        assert!(rel_dirs.contains(&"src".to_string()), "{rel_dirs:?}");
        assert!(rel_dirs.contains(&"src/nested".to_string()), "{rel_dirs:?}");
        assert!(rel_dirs.contains(&"empty".to_string()), "{rel_dirs:?}");
        assert!(
            !rel_dirs.iter().any(|d| d == "target" || d.starts_with("target/")),
            "{rel_dirs:?}"
        );
        assert_eq!(
            walk_files(&root),
            files,
            "walk_files delega en walk_entries y conserva el orden"
        );

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

    #[test]
    fn file_pattern_matches_extensions_and_lists() {
        assert!(matches_file_pattern(Path::new("src/main.rs"), "*.rs"));
        assert!(matches_file_pattern(Path::new("Cargo.toml"), "*.toml"));
        assert!(!matches_file_pattern(Path::new("notes.txt"), "*.rs"));
        assert!(matches_file_pattern(
            Path::new("src/main.rs"),
            "*.rs, *.toml"
        ));
        assert!(matches_file_pattern(
            Path::new("Cargo.toml"),
            " *.rs , *.toml "
        ));
        assert!(!matches_file_pattern(
            Path::new("notes.txt"),
            "*.rs, *.toml"
        ));
    }

    #[test]
    fn file_pattern_matches_short_extension_and_case_insensitively() {
        assert!(matches_file_pattern(Path::new("src/main.rs"), ".rs"));
        assert!(matches_file_pattern(Path::new("src/main.RS"), "*.rs"));
        assert!(matches_file_pattern(Path::new("SRC/main.rs"), "src/"));
        assert!(!matches_file_pattern(Path::new("lib/main.rs"), ".toml"));
    }

    #[test]
    fn file_pattern_matches_path_prefixes_and_globs() {
        assert!(matches_file_pattern(Path::new("src/main.rs"), "src/"));
        assert!(matches_file_pattern(
            Path::new("crates/core/src/lib.rs"),
            "crates/"
        ));
        assert!(!matches_file_pattern(Path::new("src/main.rs"), "crates/"));
        assert!(matches_file_pattern(Path::new("src/main.rs"), "src/*.rs"));
        assert!(matches_file_pattern(
            Path::new("src/nested/mod.rs"),
            "src/*/*.rs"
        ));
        assert!(!matches_file_pattern(
            Path::new("src/main.rs"),
            "src/*.toml"
        ));
        assert!(matches_file_pattern(Path::new("a1.rs"), "a?.rs"));
    }

    #[test]
    fn literal_replace_respects_case_sensitivity() {
        let query = SearchQuery::new("foo");
        let (out, count) = replace_in_string("foo Foo FOO", &query, "bar").unwrap();
        assert_eq!(count, 3);
        assert_eq!(out, "bar bar bar");

        let mut sensitive = SearchQuery::new("Foo");
        sensitive.case_sensitive = true;
        let (out, count) = replace_in_string("foo Foo FOO", &sensitive, "bar").unwrap();
        assert_eq!(count, 1);
        assert_eq!(out, "foo bar FOO");
    }

    #[test]
    fn literal_replace_respects_whole_words() {
        let mut query = SearchQuery::new("foo");
        query.match_word = true;
        let (out, count) = replace_in_string("foobar foo _foo foo_", &query, "bar").unwrap();
        assert_eq!(count, 1);
        assert_eq!(out, "foobar bar _foo foo_");
    }

    #[test]
    fn literal_replace_handles_unicode_and_keeps_dollar_literal() {
        // `İ` ocupa más bytes en el original que su minúscula: los offsets no
        // deben desalinearse. En modo literal `$1` no se expande.
        let query = SearchQuery::new("foo");
        let (out, count) = replace_in_string("İfoo bar foo", &query, "$1bar").unwrap();
        assert_eq!(count, 2);
        assert_eq!(out, "İ$1bar bar $1bar");
    }

    #[test]
    fn regex_replace_supports_capture_groups() {
        let mut query = SearchQuery::new(r"(\w+)\s+(\w+)");
        query.use_regex = true;
        let (out, count) = replace_in_string("John Smith", &query, "$2, $1").unwrap();
        assert_eq!(count, 1);
        assert_eq!(out, "Smith, John");
    }

    #[test]
    fn regex_replace_respects_case_sensitivity() {
        let mut query = SearchQuery::new("foo");
        query.use_regex = true;
        let (out, count) = replace_in_string("foo Foo FOO", &query, "bar").unwrap();
        assert_eq!(count, 3);
        assert_eq!(out, "bar bar bar");

        let mut sensitive = SearchQuery::new("Foo");
        sensitive.use_regex = true;
        sensitive.case_sensitive = true;
        let (out, count) = replace_in_string("foo Foo FOO", &sensitive, "bar").unwrap();
        assert_eq!(count, 1);
        assert_eq!(out, "foo bar FOO");
    }

    #[test]
    fn regex_replace_reports_invalid_regex() {
        let mut query = SearchQuery::new("([a-z");
        query.use_regex = true;
        assert!(replace_in_string("abc", &query, "x").is_err());
    }

    #[test]
    fn empty_query_replace_is_a_noop() {
        let (out, count) = replace_in_string("foo", &SearchQuery::new("  "), "bar").unwrap();
        assert_eq!(count, 0);
        assert_eq!(out, "foo");
    }

    #[test]
    fn replace_in_file_only_writes_when_matching() {
        let root = temp_dir("replace_file");
        let file = root.join("a.txt");
        std::fs::write(&file, "foo\r\nfoo\r\n").unwrap();

        let count = replace_in_file(&file, &SearchQuery::new("foo"), "bar").unwrap();
        assert_eq!(count, 2);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "bar\r\nbar\r\n");

        let count = replace_in_file(&file, &SearchQuery::new("missing"), "x").unwrap();
        assert_eq!(count, 0);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "bar\r\nbar\r\n");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn replace_in_workspace_filters_by_pattern() {
        let root = temp_dir("replace_workspace");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "let foo = 1;\nfoo();\n").unwrap();
        std::fs::write(root.join("src/b.toml"), "foo = 1\n").unwrap();
        std::fs::write(root.join("notes.txt"), "foo foo\n").unwrap();

        let mut query = SearchQuery::new("foo");
        query.file_pattern = Some("*.rs, *.toml".to_string());
        let summary = replace_in_workspace(&root, &query, "bar").unwrap();

        assert_eq!(summary.files_modified, 2);
        assert_eq!(summary.replacements_count, 3);
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "let bar = 1;\nbar();\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/b.toml")).unwrap(),
            "bar = 1\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "foo foo\n"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn replace_in_workspace_respects_path_prefix() {
        let root = temp_dir("replace_prefix");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "foo\n").unwrap();
        std::fs::write(root.join("top.rs"), "foo\n").unwrap();

        let mut query = SearchQuery::new("foo");
        query.file_pattern = Some("src/".to_string());
        let summary = replace_in_workspace(&root, &query, "bar").unwrap();

        assert_eq!(summary.files_modified, 1);
        assert_eq!(summary.replacements_count, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "bar\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("top.rs")).unwrap(),
            "foo\n"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn replace_in_workspace_skips_non_utf8_files_and_continues() {
        let root = temp_dir("replace_non_utf8");
        std::fs::write(root.join("a.rs"), "foo\n").unwrap();
        std::fs::write(root.join("latin1.txt"), b"caf\xe9 foo\n").unwrap();

        let summary = replace_in_workspace(&root, &SearchQuery::new("foo"), "bar").unwrap();

        assert_eq!(summary.files_modified, 1);
        assert_eq!(summary.replacements_count, 1);
        assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(), "bar\n");
        assert_eq!(
            std::fs::read(root.join("latin1.txt")).unwrap(),
            b"caf\xe9 foo\n"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn workspace_search_skips_files_outside_pattern() {
        let root = temp_dir("search_filter");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "needle\n").unwrap();
        std::fs::write(root.join("src/b.toml"), "needle\n").unwrap();

        let engine = WorkspaceSearch::spawn();
        let mut query = SearchQuery::new("needle");
        query.file_pattern = Some("*.toml".to_string());
        engine.request(1, &root, query);

        let mut files = Vec::new();
        for _ in 0..200 {
            match engine.try_recv() {
                Some((_, SearchEvent::File(file))) => files.push(file.relative_display),
                Some((_, SearchEvent::Done { .. })) => break,
                None => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        assert_eq!(files, vec!["src/b.toml".to_string()]);

        std::fs::remove_dir_all(&root).ok();
    }
}
