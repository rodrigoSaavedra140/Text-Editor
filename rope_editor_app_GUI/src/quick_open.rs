//! S7 (Etapa 2): Búsqueda rápida de archivos (Ctrl+P / Quick Open).
//! Escanea archivos del proyecto, filtra en tiempo real por coincidencia difusa (fuzzy)
//! y permite abrir archivos mediante navegación de teclado o click.

use std::path::{Path, PathBuf};

/// Ponderación y filtrado difuso (fuzzy) para candidatos de ruta.
/// Retorna `Some(score)` si `pattern` coincide de forma difusa con `candidate`,
/// donde un puntaje mayor indica mejor relevancia.
pub fn fuzzy_match(pattern: &str, candidate: &str) -> Option<i64> {
    if pattern.is_empty() {
        return Some(0);
    }

    let p_lower: Vec<char> = pattern.to_lowercase().chars().collect();
    let c_lower: Vec<char> = candidate.to_lowercase().chars().collect();

    let mut p_idx = 0;
    let mut c_idx = 0;
    let mut score = 0i64;
    let mut consecutive = 0i64;
    let mut first_match_idx = None;

    while p_idx < p_lower.len() && c_idx < c_lower.len() {
        if p_lower[p_idx] == c_lower[c_idx] {
            if first_match_idx.is_none() {
                first_match_idx = Some(c_idx);
            }

            // Bono por coincidencias consecutivas
            consecutive += 1;
            score += 20 * consecutive;

            // Bono por coincidencia en límite de palabra (inicio, tras '/', '_', '-', '.')
            if c_idx == 0
                || c_lower[c_idx - 1] == '/'
                || c_lower[c_idx - 1] == '\\'
                || c_lower[c_idx - 1] == '_'
                || c_lower[c_idx - 1] == '-'
                || c_lower[c_idx - 1] == '.'
            {
                score += 50;
            }

            p_idx += 1;
        } else {
            consecutive = 0;
        }
        c_idx += 1;
    }

    // Si no se consumieron todos los caracteres del patrón, no hay coincidencia
    if p_idx < p_lower.len() {
        return None;
    }

    // Bono si coincide exactamente con el nombre de archivo final
    let file_name = Path::new(candidate)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(candidate)
        .to_lowercase();
    if file_name.starts_with(&pattern.to_lowercase()) {
        score += 100;
    }

    // Penalización leve por longitud total del camino para preferir rutas más directas
    score -= candidate.len() as i64;

    Some(score)
}

/// Escanea de forma recursiva archivos de texto dentro de `root`, omitiendo
/// carpetas ocultas (`.git`, etc.) y directorios de compilación (`target`, `node_modules`).
pub fn scan_workspace_files(root: &Path, max_files: usize) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut subdirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            // Ignorar archivos y directorios ocultos o de build
            if file_name.starts_with('.')
                || file_name == "target"
                || file_name == "node_modules"
                || file_name == "build"
                || file_name == "dist"
            {
                continue;
            }

            if path.is_dir() {
                subdirs.push(path);
            } else if path.is_file() {
                // Almacenar ruta relativa a `root` si es posible
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                files.push(rel);
            }
        }

        // Ordenar para reproducibilidad
        subdirs.sort();
        files.sort();

        for f in files {
            result.push(f);
            if result.len() >= max_files {
                return result;
            }
        }

        // Empujar en orden inverso para procesar en orden alfabético
        for d in subdirs.into_iter().rev() {
            stack.push(d);
        }
    }

    result
}

/// Estado del diálogo modal de búsqueda rápida de archivos (Ctrl+P).
#[derive(Debug, Default)]
pub struct QuickOpenState {
    pub open: bool,
    pub query: String,
    pub selected_idx: usize,
    pub scanned_files: Vec<PathBuf>,
    pub filtered: Vec<(PathBuf, i64)>,
    pub request_focus: bool,
}

impl QuickOpenState {
    /// Abre el modal de búsqueda rápida escaneando la carpeta `root`.
    pub fn open(&mut self, root: &Path) {
        self.open = true;
        self.query.clear();
        self.selected_idx = 0;
        self.scanned_files = scan_workspace_files(root, 1500);
        self.filtered = self
            .scanned_files
            .iter()
            .map(|p| (p.clone(), 0))
            .collect();
        self.request_focus = true;
    }

    /// Cierra el modal.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected_idx = 0;
        self.request_focus = false;
    }

    /// Actualiza el filtro difuso ante cambios en la consulta del usuario.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.selected_idx = 0;

        if self.query.trim().is_empty() {
            self.filtered = self
                .scanned_files
                .iter()
                .map(|p| (p.clone(), 0))
                .collect();
        } else {
            let mut matches: Vec<(PathBuf, i64)> = self
                .scanned_files
                .iter()
                .filter_map(|p| {
                    let s = p.to_string_lossy();
                    fuzzy_match(&self.query, &s).map(|score| (p.clone(), score))
                })
                .collect();

            // Orden descendente por puntaje (mayor relevancia primero)
            matches.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered = matches;
        }
    }

    /// Mueve la selección hacia el elemento anterior.
    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    /// Mueve la selección hacia el elemento siguiente.
    pub fn select_next(&mut self) {
        if !self.filtered.is_empty() && self.selected_idx + 1 < self.filtered.len() {
            self.selected_idx += 1;
        }
    }

    /// Retorna la ruta seleccionada actual, si existe.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.filtered.get(self.selected_idx).map(|(p, _)| p.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact_and_subsequence() {
        assert!(fuzzy_match("main", "src/main.rs").is_some());
        assert!(fuzzy_match("smr", "src/main.rs").is_some());
        assert!(fuzzy_match("xyz", "src/main.rs").is_none());
        assert_eq!(fuzzy_match("", "any_file.txt"), Some(0));
    }

    #[test]
    fn test_fuzzy_match_boundary_ranking() {
        let score_direct = fuzzy_match("main", "src/main.rs").unwrap();
        let score_nested = fuzzy_match("main", "some_long_path_to_a_domain_file.rs").unwrap();
        // src/main.rs debe tener mejor puntaje por coincidencia de nombre exacto y brevedad
        assert!(score_direct > score_nested);
    }

    #[test]
    fn test_quick_open_state_navigation() {
        let mut state = QuickOpenState::default();
        state.scanned_files = vec![
            PathBuf::from("src/app.rs"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/ui.rs"),
        ];
        state.set_query(String::new());
        assert_eq!(state.filtered.len(), 3);
        assert_eq!(state.selected_idx, 0);

        state.select_next();
        assert_eq!(state.selected_idx, 1);
        assert_eq!(state.selected_path(), Some(PathBuf::from("src/main.rs")));

        state.select_next();
        assert_eq!(state.selected_idx, 2);

        // Clampeo al final
        state.select_next();
        assert_eq!(state.selected_idx, 2);

        state.select_prev();
        assert_eq!(state.selected_idx, 1);

        // Filtrado por 'ui'
        state.set_query("ui".to_string());
        assert_eq!(state.filtered.len(), 1);
        assert_eq!(state.selected_path(), Some(PathBuf::from("src/ui.rs")));
    }
}
