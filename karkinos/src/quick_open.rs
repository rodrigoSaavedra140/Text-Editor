//! S7 (Etapa 2): Búsqueda rápida de archivos (Ctrl+P / Quick Open).
//! Escanea archivos del proyecto, filtra en tiempo real por coincidencia difusa (fuzzy)
//! y permite abrir archivos mediante navegación de teclado o click.
//!
//! S4 (Roadmap v0.2.9): en un monorepo de ~20.000 archivos, el escaneo
//! y el scoring fuzzy corren en hilos de fondo — nunca en el hilo de
//! dibujo. Listas chicas (≤ `INLINE_SCORE_THRESHOLD`) siguen siendo
//! puntuadas inline (cuesta <2 ms y evita la maquinaria async).

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

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

// -----------------------------------------------------------------------------
// S4 (Roadmap v0.2.9): indexación y scoring en background
// -----------------------------------------------------------------------------

/// Techo del índice de archivos (S4: monorepos con ~20.000 archivos).
pub const INDEX_MAX_FILES: usize = 20_000;
/// Por debajo de este tamaño el scoring sigue INLINE (<2 ms): repos
/// chicos no necesitan async y quedan igual de nítidos.
pub const INLINE_SCORE_THRESHOLD: usize = 1024;
/// Matches puntuados guardados por query (la UI solo muestra 50).
const SCORED_CAP: usize = 512;

/// Pedido de indexado de una carpeta.
pub struct IndexJob {
    pub root: PathBuf,
    pub generation: u64,
}

/// Pedido de puntuación de una query sobre una lista congelada (Arc):
/// con el `Arc` no se copian 20k rutas por tecla.
pub struct ScoreJob {
    pub generation: u64,
    pub query: String,
    pub files: Arc<Vec<PathBuf>>,
}

/// Resultado de cualquiera de los dos workers: datos planos `Send`.
pub enum QuickOpenResult {
    Indexed {
        root: PathBuf,
        generation: u64,
        files: Vec<PathBuf>,
    },
    Scored {
        generation: u64,
        matches: Vec<(PathBuf, i64)>,
    },
}

/// Worker genérico de fondo para jobs coalescibles (S4). Es el patrón
/// reducido de `ast_worker::AstWorker` (T3): drena la cola de jobs
/// conservando el de generación MAYOR, saltea revisiones ya hechas y
/// devuelve por canal de capacidad 1 (backpressure natural).
/// El `Drop` solta los canales ANTES del `join` (lección de T3:
/// al revés el worker suerte de `recv` y el join se colgaba).
pub struct BackgroundWorker<J, R> {
    job_tx: Option<mpsc::Sender<J>>,
    result_rx: Option<mpsc::Receiver<R>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl<J: Send + 'static, R: Send + 'static> BackgroundWorker<J, R> {
    /// Lanza el hilo worker. `key_of` da la generación de cada job
    /// (monótona creciente) y `process` materia un `R` por job fresco.
    pub fn spawn(
        name: &str,
        key_of: impl Fn(&J) -> u64 + Send + 'static,
        mut process: impl FnMut(J) -> R + Send + 'static,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<J>();
        let (result_tx, result_rx) = mpsc::sync_channel::<R>(1);
        let thread_name = name.to_string();
        let handle = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let key_of = key_of;
                let mut last_done: u64 = 0;
                while let Ok(first) = job_rx.recv() {
                    // Coalescing: al drenar, cubri el job de generación mayor.
                    let mut latest = first;
                    while let Ok(next) = job_rx.try_recv() {
                        if key_of(&next) > key_of(&latest) {
                            latest = next;
                        }
                    }
                    if key_of(&latest) <= last_done {
                        continue;
                    }
                    let done_key = key_of(&latest);
                    let _ = result_tx.send(process(latest));
                    last_done = done_key;
                }
            })
            .expect("debe poder crearse el hilo del worker de fondo");
        Self {
            job_tx: Some(job_tx),
            result_rx: Some(result_rx),
            handle: Some(handle),
        }
    }

    /// Variante sin hilos para tests de estado puro: `submit` es no-op
    /// y nunca llega un resultado.
    #[cfg(test)]
    pub fn noop() -> Self {
        Self {
            job_tx: None,
            result_rx: None,
            handle: None,
        }
    }

    /// Encola un job (no bloquea).
    pub fn submit(&self, job: J) {
        if let Some(job_tx) = &self.job_tx {
            let _ = job_tx.send(job);
        }
    }

    /// Consume los resultados vivos; quedándose con el ÚLTIMO (los
    /// anteriores son generaciones supersadas).
    pub fn try_recv(&self) -> Option<R> {
        let result_rx = self.result_rx.as_ref()?;
        let mut best = result_rx.try_recv().ok()?;
        while let Ok(next) = result_rx.try_recv() {
            best = next;
        }
        Some(best)
    }
}

impl<J, R> Drop for BackgroundWorker<J, R> {
    fn drop(&mut self) {
        // Orden crítico (lección T3): 1) soltar el receiver de resultados
        // para despertar un send() bloqueado, 2) soltar el sender de
        // jobs para que recv() enormemente Err y el worker salga,
        // 3) recién entonces join.
        self.result_rx.take();
        self.job_tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Los dos servicios de fondo del Quick Open (indexador + scorer).
/// Suelen instanciarse con `default()` (hilos reales) y en tests de
/// estado puro con `noop()` (sin hilos).
pub struct QuickOpenServices {
    indexer: BackgroundWorker<IndexJob, QuickOpenResult>,
    scorer: BackgroundWorker<ScoreJob, QuickOpenResult>,
}

impl Default for QuickOpenServices {
    fn default() -> Self {
        Self {
            indexer: BackgroundWorker::spawn(
                "karkinos-qo-index",
                |job: &IndexJob| job.generation,
                |job: IndexJob| {
                    let files = scan_workspace_files(&job.root, INDEX_MAX_FILES);
                    QuickOpenResult::Indexed {
                        root: job.root,
                        generation: job.generation,
                        files,
                    }
                },
            ),
            scorer: BackgroundWorker::spawn(
                "karkinos-qo-score",
                |job: &ScoreJob| job.generation,
                |job: ScoreJob| {
                    let mut matches: Vec<(PathBuf, i64)> = job
                        .files
                        .iter()
                        .filter_map(|p| {
                            let s = p.to_string_lossy();
                            fuzzy_match(&job.query, &s).map(|score| (p.clone(), score))
                        })
                        .collect();
                    matches.sort_by(|a, b| b.1.cmp(&a.1));
                    matches.truncate(SCORED_CAP);
                    QuickOpenResult::Scored {
                        generation: job.generation,
                        matches,
                    }
                },
            ),
        }
    }
}

impl QuickOpenServices {
    /// Variante sin hilos (tests de estado puro).
    #[cfg(test)]
    pub fn noop() -> Self {
        Self {
            indexer: BackgroundWorker::noop(),
            scorer: BackgroundWorker::noop(),
        }
    }

    /// Pide indexar la carpeta `root` para la generación dada.
    pub fn request_index(&self, root: &Path, generation: u64) {
        self.indexer.submit(IndexJob {
            root: root.to_path_buf(),
            generation,
        });
    }

    /// Pide puntuar `query` sobre la lista congelada (generación dada).
    pub fn request_score(&self, query: String, generation: u64, files: Arc<Vec<PathBuf>>) {
        self.scorer.submit(ScoreJob {
            generation,
            query,
            files,
        });
    }

    /// Siguiente resultado disponible (indexador primero, scorer después).
    pub fn poll_next(&self) -> Option<QuickOpenResult> {
        self.indexer
            .try_recv()
            .or_else(|| self.scorer.try_recv())
    }
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
    // S4 (Roadmap v0.2.9):
    /// Raíz de la lista cacheada (reabrir la misma carpeta = cero I/O).
    pub indexed_root: Option<PathBuf>,
    /// Escaneo de carpeta en curso en el worker.
    pub indexing: bool,
    /// Scoring de query grande en curso en el worker.
    pub scoring: bool,
    /// Generación del open (un escaneo por raíz nueva).
    pub open_generation: u64,
    /// Generación del scoring (una query por generación).
    pub score_generation: u64,
}

impl QuickOpenState {
    ///Abre el modal. Si hay índice cacheado de esa raíz lo reutiliza
    /// (cero bloqueo); si no, pide el escaneo al worker y muestra
    /// "Scanning…" hasta que llegue — NUNCA escanea en el hilo de UI.
    pub fn request_open(&mut self, root: &Path, services: &mut QuickOpenServices) {
        self.open = true;
        self.query.clear();
        self.selected_idx = 0;
        self.request_focus = true;

        if self.indexed_root.as_deref() == Some(root) && !self.scanned_files.is_empty() {
            // Cache caliente de esta raíz: lista completa, cero trabajo.
            self.indexing = false;
            self.scoring = false;
            self.filtered = self
                .scanned_files
                .iter()
                .map(|p| (p.clone(), 0))
                .collect();
            return;
        }

        // Carpeta nueva: limpiar y delegar el escaneo al worker.
        self.scanned_files.clear();
        self.filtered.clear();
        self.indexed_root = None;
        self.indexing = true;
        self.open_generation = self.open_generation.wrapping_add(1);
        services.request_index(root, self.open_generation);
    }

    /// Cierra el modal.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected_idx = 0;
        self.request_focus = false;
    }

    /// Actualiza el filtro difuso ante cambios en la consulta del usuario.
    /// Listas grandes (> `INLINE_SCORE_THRESHOLD`) delegan el scoring al
    /// worker (#generación por query); el resultado llega por `poll`.
    pub fn set_query(&mut self, query: String, services: &mut QuickOpenServices) {
        self.query = query;
        self.selected_idx = 0;

        if self.query.trim().is_empty() {
            self.scoring = false;
            self.filtered = self
                .scanned_files
                .iter()
                .map(|p| (p.clone(), 0))
                .collect();
            return;
        }

        if self.scanned_files.len() <= INLINE_SCORE_THRESHOLD {
            // Scoring inline (repos chicos): igual que siempre.
            self.scoring = false;
            let mut matches: Vec<(PathBuf, i64)> = self
                .scanned_files
                .iter()
                .filter_map(|p| {
                    let s = p.to_string_lossy();
                    fuzzy_match(&self.query, &s).map(|score| (p.clone(), score))
                })
                .collect();
            matches.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered = matches;
        } else {
            // Scoring en fondo: query congelada + lista congelada (Arc).
            self.scoring = true;
            self.score_generation = self.score_generation.wrapping_add(1);
            let files = Arc::new(self.scanned_files.clone());
            services.request_score(self.query.clone(), self.score_generation, files);
        }
    }

    /// Aplica los matches puntuados del worker si corresponden a la
    /// query ACTUAL. Devuelve `true` cuando se aplicaron.
    pub fn apply_scored(&mut self, generation: u64, matches: Vec<(PathBuf, i64)>) -> bool {
        if generation != self.score_generation {
            return false; // resultado de una query vieja: no contamina
        }
        self.filtered = matches;
        self.scoring = false;
        self.selected_idx = 0;
        true
    }

    /// Consume los resultados de los workers (por frame). Devuelve
    /// `true` cuando aplicó algo nuevo (la UI repinta suave).
    pub fn poll(&mut self, services: &mut QuickOpenServices) -> bool {
        let mut changed = false;
        while let Some(result) = services.poll_next() {
            match result {
                QuickOpenResult::Indexed { root, generation, files } => {
                    if generation == self.open_generation {
                        self.indexing = false;
                        self.scanned_files = files;
                        self.indexed_root = Some(root);
                        // Reaplicar la query activa sobre el índice novo
                        // (puede re-enconcarar el scoring al worker).
                        let query = self.query.clone();
                        self.set_query(query, services);
                        changed = true;
                    }
                }
                QuickOpenResult::Scored { generation, matches } => {
                    if self.apply_scored(generation, matches) {
                        changed = true;
                    }
                }
            }
        }
        changed
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

    // ---------------------------------------------------------------
    // S4 (Roadmap v0.2.9): indexación y scoring en background
    // ---------------------------------------------------------------

    /// Builder de un árbol temp FUERA del worktree (tmpdir del sistema)
    /// para los tests de indexación; se limpia al final.
    fn temp_workspace(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("karkinos-t4-{}-{}", tag, unique));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).expect("crear el árbol temporáneo");
        std::fs::write(dir.join("main.rs"), "fn main() {}\n").expect("escribir main.rs");
        std::fs::write(dir.join("hidden.txt"), "nope\n").expect("escribir hidden.txt");
        std::fs::create_dir_all(dir.join(".git")).expect("crear .git");
        std::fs::write(dir.join(".git/config"), "falso\n").expect("escribir config");
        std::fs::write(sub.join("helper.rs"), "pub fn helper() {}\n").expect("escribir helper.rs");
        dir
    }

    fn poll_services(
        services: &mut QuickOpenServices,
        state: &mut QuickOpenState,
        tries: usize,
    ) -> bool {
        for _ in 0..tries {
            if state.poll(services) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        state.poll(services)
    }

    #[test]
    fn quick_open_index_en_fondo_devuelve_archivos_rel_y_saltea_ocultos() {
        let dir = temp_workspace("index");
        let mut services = QuickOpenServices::default();
        let mut state = QuickOpenState::default();

        state.request_open(&dir, &mut services);
        assert!(state.indexing, "el primer open arranca un indexado en fondo");
        assert!(state.scanned_files.is_empty(), "nada se escaneó inline");

        assert!(
            poll_services(&mut services, &mut state, 100),
            "el worker debe responder con el índice"
        );
        assert!(!state.indexing);
        assert!(!state.scanned_files.is_empty());
        assert!(state.scanned_files.contains(&PathBuf::from("main.rs")));
        assert!(state.scanned_files.contains(&PathBuf::from("sub/helper.rs")));
        assert!(
            !state.scanned_files.iter().any(|p| p.starts_with(".git")),
            "los ocultos no entran al índice"
        );
        // La query vacía reaplica el listado: filtered = todo con score 0.
        assert_eq!(state.filtered.len(), state.scanned_files.len());
        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_open_reabrir_reusa_el_cache_de_la_misma_carpeta() {
        let dir = temp_workspace("cache");
        let mut services = QuickOpenServices::default();
        let mut state = QuickOpenState::default();

        state.request_open(&dir, &mut services);
        assert!(poll_services(&mut services, &mut state, 100), "primer index llega");

        state.close();
        state.request_open(&dir, &mut services);
        // Reabrir en la MISMA carpeta NO debe reindexar: instantáneo.
        assert!(!state.indexing, "reabrir con cache caliente no reindexa");
        assert!(!state.scanned_files.is_empty(), "la lista cacheada está viva");
        assert_eq!(state.filtered.len(), state.scanned_files.len());
        assert_eq!(state.indexed_root, Some(dir.clone()));

        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_open_scoring_grande_va_al_worker_chico_queda_inline() {
        let dir = temp_workspace("score");
        let mut services = QuickOpenServices::default();
        let mut state = QuickOpenState::default();

        // Lista chica: scoring INLINE (sincrónico, como siempre).
        state.scanned_files = vec![
            PathBuf::from("src/app.rs"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/ui.rs"),
        ];
        state.set_query("main".to_string(), &mut services);
        assert!(!state.scoring, "listas chicas se puntuaban inline");
        assert_eq!(state.filtered.len(), 1);
        assert_eq!(state.selected_path(), Some(PathBuf::from("src/main.rs")));

        // Lista grande (THRESHOLD+): se manda al worker de fondo.
        state.scanned_files = (0..INLINE_SCORE_THRESHOLD + 50)
            .map(|i| PathBuf::from(format!("dir/file_{}.rs", i)))
            .chain([PathBuf::from("src/main.rs"), PathBuf::from("src/ui.rs")])
            .collect();
        state.set_query("main".to_string(), &mut services);
        assert!(state.scoring, "lista grande → scoring en fondo");

        assert!(
            poll_services(&mut services, &mut state, 100),
            "el scorer debe responder y aplicar los matches"
        );
        assert!(!state.scoring);
        assert!(!state.filtered.is_empty());
        // "src/main.rs" es el nombre exacto → debe quedar primero.
        assert_eq!(state.filtered[0].0, PathBuf::from("src/main.rs"));

        drop(services);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_open_resultado_viejo_no_pisa_query_nueva() {
        let mut services = QuickOpenServices::noop();
        let mut state = QuickOpenState::default();
        state.scanned_files = vec![PathBuf::from("a/main.rs"), PathBuf::from("b/other.rs")];
        state.set_query("main".to_string(), &mut services);
        assert_eq!(state.selected_path(), Some(PathBuf::from("a/main.rs")));

        // Resultado con la generación ANTERIOR (query ya superada): ignorado.
        let stale_gen = state.score_generation.wrapping_sub(1);
        assert!(
            !state.apply_scored(stale_gen, vec![(PathBuf::from("b/other.rs"), 999)]),
            "un resultado de generación vieja NO debe aplicar"
        );
        assert_eq!(state.selected_path(), Some(PathBuf::from("a/main.rs")));

        // Con la generación ACTUAL aplica y actualiza la selección.
        assert!(state.apply_scored(state.score_generation, vec![(PathBuf::from("b/other.rs"), 5)]));
        assert_eq!(state.selected_path(), Some(PathBuf::from("b/other.rs")));
    }

    #[test]
    fn test_quick_open_state_navigation() {
        let mut state = QuickOpenState::default();
        let mut services = QuickOpenServices::noop();
        state.scanned_files = vec![
            PathBuf::from("src/app.rs"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/ui.rs"),
        ];
        state.set_query(String::new(), &mut services);
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
        state.set_query("ui".to_string(), &mut services);
        assert_eq!(state.filtered.len(), 1);
        assert_eq!(state.selected_path(), Some(PathBuf::from("src/ui.rs")));
    }
}
