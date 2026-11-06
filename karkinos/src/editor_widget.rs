use eframe::egui;

use karkinos_core::TextBuffer;
use karkinos_core::text_ops::{
    closing_delimiter, col_from_visual_col, is_closing_delimiter, is_empty_pair_around,
    visual_col_at, DEFAULT_TAB_SIZE,
};

/// Posición en el documento (línea 0-based, columna 0-based).
pub type TextPos = (usize, usize);
/// Rango de texto delimitado por dos posiciones (inicio, fin).
pub type TextRange = (TextPos, TextPos);

/// S10: límites y tamaño por defecto de la fuente del editor.
pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// S10: acota el tamaño de fuente al rango permitido [8, 32] px.
pub fn clamp_font_size(size: f32) -> f32 {
    size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

/// OPT-3 (v0.2.26 / v0.2.57): fracción de acercamiento al destino por frame. Con 0.42
/// la animación reacciona de inmediato en el primer frame y asienta con suavidad en ~7-8 frames,
/// eliminando la sensación de arrastre o retraso al scrollear archivos.
pub const SCROLL_EASING: f32 = 0.42;

/// OPT-3: un paso de interpolación exponencial hacia `target`. Devuelve el
/// scroll nuevo y si la animación sigue en curso (todavía hay diferencia
/// visible) o ya se puede asentar en el destino.
pub fn smooth_scroll_step(scroll: f32, target: f32) -> (f32, bool) {
    let diff = target - scroll;
    if diff.abs() > 0.5 {
        (scroll + diff * SCROLL_EASING, true)
    } else {
        (target, false)
    }
}

/// S11: Parámetros del cursor smear (animación fluida con estela deformable).
pub const SMEAR_HEAD_EASING: f32 = 0.72;
pub const SMEAR_TAIL_EASING: f32 = 0.42;
pub const SMEAR_MAX_DISTANCE: f32 = 300.0;

/// Paso de interpolación para el cursor smear: acerca `current` hacia `target`.
/// Retorna la nueva posición y si la animación continúa activa.
pub fn smear_step(current: egui::Pos2, target: egui::Pos2, easing: f32) -> (egui::Pos2, bool) {
    let diff = target - current;
    let dist_sq = diff.length_sq();
    if dist_sq > 0.25 {
        (current + diff * easing, true)
    } else {
        (target, false)
    }
}

/// Producto vectorial 2D para orientación de giros en el cálculo del convex hull.
fn cross_2d(o: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Calcula la envolvente convexa (convex hull) de un conjunto pequeño de puntos
/// (algoritmo Monotone Chain de Andrew) para generar la malla de la estela deformada.
pub fn convex_hull(mut pts: Vec<egui::Pos2>) -> Vec<egui::Pos2> {
    pts.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup_by(|a, b| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
    if pts.len() <= 2 {
        return pts;
    }

    let mut lower = Vec::with_capacity(pts.len());
    for &p in &pts {
        while lower.len() >= 2 && cross_2d(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }

    let mut upper = Vec::with_capacity(pts.len());
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross_2d(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }

    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// F3 (Roadmap v0.2.29): cursor adicional. El primario sigue siendo
/// `cursor_line`/`cursor_col`/`sel_anchor` del estado; estos son los extra,
/// así la edición de un solo cursor no cambia de comportamiento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caret {
    pub line: usize,
    pub col: usize,
    pub anchor: Option<(usize, usize)>,
}

impl Caret {
    /// Rango normalizado del cursor, si tiene selección.
    pub fn range(&self) -> Option<((usize, usize), (usize, usize))> {
        let end = (self.line, self.col);
        let anchor = self.anchor?;
        if anchor == end {
            return None;
        }
        Some(if anchor <= end { (anchor, end) } else { (end, anchor) })
    }
}

/// S8 (T7): vista de líneas que consume el widget del editor.
///
/// En producción la implementa `TextBuffer` (el único dueño del
/// texto, en el Rope). Los tests siguen usando `Vec<String>`, que
/// implementa el mismo contrato para no duplicar los casos.
pub trait LineBuffer {
    fn line_count(&self) -> usize;
    fn get_line(&self, i: usize) -> String;
    /// Cantidad de caracteres de la línea `i` sin el salto final,
    /// sin crear un `String`.
    fn line_char_len(&self, i: usize) -> usize;
    /// Indica si la línea `i` contiene algún tab, sin crear un `String`.
    fn line_has_tab(&self, i: usize) -> bool;
    /// Recalcula las filas visuales de todas las líneas. El default usa
    /// `line_char_len`/`line_has_tab`; `TextBuffer` lo sobreescribe para
    /// recorrer el Rope secuencialmente, sin navegar el árbol por línea.
    fn rebuild_row_counts(&self, wrap_chars: usize, state: &EditorState) -> Vec<usize> {
        (0..self.line_count())
            .map(|i| {
                if state.is_line_folded(i) {
                    0
                } else {
                    let len = self.line_char_len(i);
                    if self.line_has_tab(i) {
                        row_count_for(&self.get_line(i), wrap_chars)
                    } else {
                        row_count_for_len(len, wrap_chars)
                    }
                }
            })
            .collect()
    }
    /// Recalcula las largos de carácter de todas las líneas, sin asignar
    /// un `String` por línea.
    fn rebuild_char_lens(&self) -> Vec<usize> {
        (0..self.line_count())
            .map(|i| self.line_char_len(i))
            .collect()
    }
    /// Recalcula ambas cachés en una sola pasada. El default delega en
    /// los dos métodos; `TextBuffer` lo sobreescribe para recorrer el
    /// Rope una única vez (recorrerlo es la parte cara).
    fn rebuild_caches(
        &self,
        wrap_chars: usize,
        state: &EditorState,
    ) -> (Vec<usize>, Vec<usize>) {
        (
            self.rebuild_row_counts(wrap_chars, state),
            self.rebuild_char_lens(),
        )
    }
    fn full_text(&self) -> String;
    fn set_line(&mut self, i: usize, text: &str);
    fn insert_line(&mut self, i: usize, text: String);
    fn remove_lines(&mut self, range: std::ops::Range<usize>);
}

impl LineBuffer for Vec<String> {
    fn line_count(&self) -> usize {
        self.len()
    }

    fn get_line(&self, i: usize) -> String {
        self.get(i).cloned().unwrap_or_default()
    }

    fn line_char_len(&self, i: usize) -> usize {
        self.get(i).map(|l| l.chars().count()).unwrap_or(0)
    }

    fn line_has_tab(&self, i: usize) -> bool {
        self.get(i).is_some_and(|l| l.contains('\t'))
    }

    fn full_text(&self) -> String {
        self.join("\n")
    }

    fn set_line(&mut self, i: usize, text: &str) {
        if let Some(line) = self.get_mut(i) {
            line.clear();
            line.push_str(text);
        }
    }

    fn insert_line(&mut self, i: usize, text: String) {
        self.insert(i.min(self.len()), text);
    }

    fn remove_lines(&mut self, range: std::ops::Range<usize>) {
        let start = range.start.min(self.len());
        let end = range.end.min(self.len());
        if start < end {
            self.drain(start..end);
        }
    }
}

impl LineBuffer for TextBuffer {
    fn line_count(&self) -> usize {
        self.len_lines()
    }

    fn get_line(&self, i: usize) -> String {
        self.line_without_newline(i)
    }

    fn line_char_len(&self, i: usize) -> usize {
        self.line_char_count(i)
    }

    fn line_has_tab(&self, i: usize) -> bool {
        self.line(i).is_some_and(|slice| slice.chars().any(|c| c == '\t'))
    }

    /// Recorre el Rope de punta a punta (O(n) total) en vez de navegar el
    /// árbol con `line(i)` una vez por caché y por línea.
    fn rebuild_row_counts(&self, wrap_chars: usize, state: &EditorState) -> Vec<usize> {
        let mut rows = Vec::with_capacity(self.line_count());
        for (i, slice) in self.lines().enumerate() {
            if state.is_line_folded(i) {
                rows.push(0);
                continue;
            }
            let (len, has_tab) = slice_line_info(slice);
            rows.push(if has_tab {
                row_count_for_len(slice_visual_len(slice, len), wrap_chars)
            } else {
                row_count_for_len(len, wrap_chars)
            });
        }
        rows
    }

    fn rebuild_char_lens(&self) -> Vec<usize> {
        let mut lens = Vec::with_capacity(self.line_count());
        for slice in self.lines() {
            lens.push(slice_char_len(slice));
        }
        lens
    }

    /// Una sola pasada sobre el Rope para las dos cachés: recorrerlo es
    /// la parte cara, el cálculo por línea es O(1) o O(largo de línea).
    fn rebuild_caches(&self, wrap_chars: usize, state: &EditorState) -> (Vec<usize>, Vec<usize>) {
        let mut rows = Vec::with_capacity(self.line_count());
        let mut lens = Vec::with_capacity(self.line_count());
        for (i, slice) in self.lines().enumerate() {
            let (len, has_tab) = slice_line_info(slice);
            lens.push(len);
            if state.is_line_folded(i) {
                rows.push(0);
            } else if has_tab {
                rows.push(row_count_for_len(
                    slice_visual_len(slice, len),
                    wrap_chars,
                ));
            } else {
                rows.push(row_count_for_len(len, wrap_chars));
            }
        }
        (rows, lens)
    }

    fn full_text(&self) -> String {
        self.to_string()
    }

    fn set_line(&mut self, i: usize, text: &str) {
        let current_len = self.line_without_newline(i).chars().count();
        let start = self.line_col_to_char(i, 0);
        let end = start + current_len;
        if start < end {
            self.remove(start..end);
        }
        if !text.is_empty() {
            self.insert(start, text);
        }
    }

    fn insert_line(&mut self, i: usize, text: String) {
        // Inserta una línea LÓGICA completa. Al final del buffer hay
        // que agregar el salto que separa la nueva línea de la última.
        if i < self.len_lines() {
            let at = self.line_col_to_char(i, 0);
            self.insert(at, &format!("{}\n", text));
        } else {
            let at = self.len_chars();
            self.insert(at, &format!("\n{}", text));
        }
    }

    fn remove_lines(&mut self, range: std::ops::Range<usize>) {
        let count = self.len_lines();
        let start = range.start.min(count);
        let end = range.end.min(count);
        if start >= end {
            return;
        }
        let mut start_char = self.line_col_to_char(start, 0);
        let end_char = if end < count {
            self.line_col_to_char(end, 0)
        } else {
            self.len_chars()
        };
        // Al borrar hasta la última línea también se va el salto que
        // la separaba de la anterior (si no, quedaría una línea vacía).
        if end == count && start > 0 {
            start_char -= 1;
        }
        if start_char < end_char {
            self.remove(start_char..end_char);
        }
    }
}

/// F3: próxima aparición de `needle` después de `after`, en orden de
/// documento. Solo busca dentro de una línea (Ctrl+D selecciona una palabra).
pub fn find_next_occurrence(
    lines: &dyn LineBuffer,
    needle: &str,
    after: (usize, usize),
) -> Option<((usize, usize), (usize, usize))> {
    if needle.is_empty() || needle.contains('\n') {
        return None;
    }
    let needle_chars: Vec<char> = needle.chars().collect();
    for line_idx in after.0..lines.line_count() {
        let line = lines.get_line(line_idx);
        let chars: Vec<char> = line.chars().collect();
        let from = if line_idx == after.0 { after.1 } else { 0 };
        if from >= chars.len() {
            continue;
        }
        let mut col = from;
        while col + needle_chars.len() <= chars.len() {
            if chars[col..col + needle_chars.len()] == needle_chars[..] {
                return Some(((line_idx, col), (line_idx, col + needle_chars.len())));
            }
            col += 1;
        }
    }
    None
}

/// F3: inserta el mismo texto en cada posición, de abajo hacia arriba y de
/// derecha a izquierda, para que las posiciones anteriores no se corran.
/// Devuelve las posiciones nuevas en el mismo orden que `positions`.
pub fn insert_text_multi(
    lines: &mut dyn LineBuffer,
    positions: &[(usize, usize)],
    text: &str,
) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..positions.len()).collect();
    order.sort_by(|&a, &b| positions[b].cmp(&positions[a]));
    let mut result = positions.to_vec();
    for idx in order {
        result[idx] = insert_text(lines, positions[idx], text);
    }
    result
}

/// F3: borra cada rango (en orden descendente) y devuelve, por cada uno, la
/// posición donde queda su cursor (el inicio del rango borrado).
pub fn delete_ranges_multi(
    lines: &mut dyn LineBuffer,
    ranges: &[TextRange],
) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by(|&a, &b| ranges[b].0.cmp(&ranges[a].0));
    for idx in order {
        let (start, end) = ranges[idx];
        delete_range(lines, start, end);
    }
    ranges.iter().map(|(start, _)| *start).collect()
}

/// F3: quita cursores repetidos entre sí y los que coinciden con el primario.
fn dedup_extra_carets(state: &mut EditorState) {
    let mut seen: Vec<(usize, usize)> = vec![(state.cursor_line, state.cursor_col)];
    state.extra_carets.retain(|caret| {
        let pos = (caret.line, caret.col);
        if seen.contains(&pos) {
            false
        } else {
            seen.push(pos);
            true
        }
    });
}

/// Estado del cursor/selección/scroll del editor — vive en EditorApp,
/// no acá, porque tiene que sobrevivir entre frames.
#[derive(Debug, Clone)]
pub struct EditorState {
    pub cursor_line: usize,
    pub cursor_col: usize, // índice de CARÁCTER (no byte) dentro de la línea LÓGICA
    // S12: Columna preferida / fija al navegar verticalmente entre líneas de distinta longitud
    pub preferred_col: Option<usize>,
    pub sel_anchor: Option<(usize, usize)>,
    // F3 (Roadmap v0.2.29): cursores extra de Ctrl+D / Alt+Click. El
    // primario son los tres campos de arriba; esto son solo los adicionales.
    pub extra_carets: Vec<Caret>,
    pub scroll: f32, // vertical, en píxeles (ya no hay scroll horizontal: el texto se ajusta de línea)
    // OPT-3 (v0.2.26): el scroll de rueda escribe acá y el render interpola
    // hacia este destino. `applied_scroll` guarda lo último que escribió el
    // render para detectar cambios externos (pestaña, restore, Ctrl+G) y
    // resincronizar el target en vez de pelear con la animación.
    pub target_scroll: f32,
    applied_scroll: f32,
    was_focused: bool,
    // Caché de cuántas FILAS VISUALES ocupa cada línea lógica (una
    // línea larga puede ocupar varias filas si se ajusta). Se
    // reconstruye entera solo cuando cambia la cantidad de líneas o
    // el ancho disponible — en el caso común (escribir un carácter
    // dentro de una línea) solo se actualiza la entrada de ESA línea,
    // sin tocar el resto (por eso sigue siendo rápido con archivos
    // grandes).
    row_counts: Vec<usize>,
    // Cantidad de CARACTERES por línea lógica, misma vida útil que
    // row_counts: se refrescan juntas (a pares) para no recontar los
    // chars de cada línea visible en cada frame — con una línea de
    // 1MB eso era O(tamaño) por frame.
    char_lens: Vec<usize>,
    // Sumatorias acumuladas: cum_rows[i] = filas visuales que ocupan
    // las líneas [0..i). Con esto el total de filas es O(1) y ubicar
    // la línea que arranca en una fila dada es O(log n) por búsqueda
    // binaria, en vez de O(n) sumando línea por línea cada frame.
    cum_rows: Vec<usize>,
    last_wrap_chars: usize,
    // S4: ancho del gutter con el que se construyó la caché (con
    // dígitos distintos el wrap cambia igual que al redimensionar).
    last_gutter: f32,
    // S4: aviso de "llevá el cursor a la vista" (Ctrl+G /
    // buscar y reemplazar): el autoscroll de teclado solo corre
    // cuando HAY eventos; saltos externos necesitan un flag propio.
    pub needs_autoscroll: bool,
    // S10: tamaño de fuente del editor (clamp 8..32 px)
    pub font_size: f32,
    last_font_size: f32,
    // S3 (Etapa 3): Rangos de plegado de código + spans del AST parseado
    // (S3 Roadmap v0.2.9: los spans ahora son datos planos que el worker
    // de fondo entrega; el motor tree-sitter ya NO vive en la UI).
    pub ast_spans: Vec<karkinos_core::syntax_ast::AstHighlightSpan>,
    pub folding_ranges: Vec<karkinos_core::syntax_ast::FoldingRange>,
    // F4 (Roadmap v0.2.30): líneas de inicio de bloques AST que están plegados (colapsados).
    pub collapsed_folds: Vec<usize>,
    pub last_collapsed_folds: Vec<usize>,
    // F2 (Roadmap v0.2.28): contenedores activos (impl/fn/struct...) que se
    // pegan arriba del editor al scrollear adentro. Los llena el mismo
    // parseo del worker AST que los spans.
    pub sticky_scopes: Vec<karkinos_core::syntax_ast::ScopeHeader>,
    // F7 (Roadmap v0.2.34): símbolos del documento (fn, struct, enum, trait,
    // método, const) extraídos por el worker AST para el selector.
    pub symbols: Vec<karkinos_core::syntax_ast::DocumentSymbol>,
    // F8 (Roadmap v0.2.35): ghost text del blame para la línea activa.
    pub blame_text: Option<String>,
    // UI-7 (v0.2.38): minimap apagado por defecto para que el editor use el
    // ancho completo sin franjas muertas; Ctrl+Shift+M lo alterna.
    pub show_minimap: bool,
    // S3 & S4 (Etapa 4): Diagnósticos en vivo y autocompletado LSP
    pub diagnostics: Vec<karkinos_core::lsp::LspDiagnostic>,
    pub hover_text: Option<String>,
    pub completions: Vec<karkinos_core::lsp::LspCompletionItem>,
    pub show_completions: bool,
    pub selected_completion: usize,
    pub inlay_hints: Vec<karkinos_core::lsp::LspInlayHint>,
    // OPT-2 (v0.2.26): galleys ya tipografiados de los inlay hints, por
    // (etiqueta, centésimas del tamaño de fuente). Evita re-tipografiar
    // ": i32" o ": &str" en cada frame por cada hint visible.
    hint_galleys: std::collections::HashMap<(String, u32), std::sync::Arc<egui::Galley>>,
    // F1 (Roadmap v0.2.27): marcas de diff contra HEAD para el gutter. Lo
    // llena el worker de fondo; `None` = archivo sin repo, sin versión o
    // todavía sin resultado.
    pub git_diff: Option<std::sync::Arc<karkinos_core::git_diff::FileDiffHunks>>,
    pub breakpoints: std::collections::HashSet<usize>,
    pub stopped_line: Option<usize>,
    // S1 (Roadmap v0.2.9): galleys ya tipografiados por (hash del chunk,
    // tema). Vive acá porque el state sobrevive entre frames y viaja
    // clonado por pestaña: cada tab cachea sus propias líneas.
    pub layout_cache: crate::syntax::LineLayoutCache<std::sync::Arc<egui::Galley>>,
    // S1 (Roadmap v0.2.9): revisión del AST con la que se armó la caché.
    // `update_ast` la incrementa; el render llama `sync_ast` y la caché
    // se vacía sola cuando el árbol cambió (las líneas sin editar pueden
    // cambiar de color semántico).
    pub ast_rev: u64,
    // S2 (Roadmap v0.2.9): instante (egui time, segundos) de la última
    // interacción de teclado/mouse/botón — reinicia la fase del blink.
    pub last_activity_time: f64,
    // Factor de escala (pixels_per_point) del frame anterior: si cambia
    // (DPI o escala de ventana), se invalidan galleys para evitar UVs rotos.
    pub last_pixels_per_point: f32,
    // Puntero del atlas de texturas de epaint: si epaint recrea el atlas
    // (por overflow o fill_ratio > 0.8), se invalidan todos los galleys cacheados.
    pub last_texture_atlas_ptr: usize,
    // S11: Estado de la animación del cursor smear (en espacio de documento)
    pub smear_head: Option<egui::Pos2>,
    pub smear_tail: Option<egui::Pos2>,
    pub smear_last_target: Option<(usize, usize)>,
    pub smear_enabled: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            cursor_line: 0,
            cursor_col: 0,
            preferred_col: None,
            sel_anchor: None,
            extra_carets: Vec::new(),
            scroll: 0.0,
            target_scroll: 0.0,
            applied_scroll: 0.0,
            was_focused: false,
            row_counts: Vec::new(),
            char_lens: Vec::new(),
            cum_rows: Vec::new(),
            last_wrap_chars: 0,
            last_gutter: 0.0,
            needs_autoscroll: false,
            font_size: DEFAULT_FONT_SIZE,
            last_font_size: DEFAULT_FONT_SIZE,
            ast_spans: Vec::new(),
            folding_ranges: Vec::new(),
            collapsed_folds: Vec::new(),
            last_collapsed_folds: Vec::new(),
            sticky_scopes: Vec::new(),
            symbols: Vec::new(),
            blame_text: None,
            show_minimap: false,
            diagnostics: Vec::new(),
            hover_text: None,
            completions: Vec::new(),
            show_completions: false,
            selected_completion: 0,
            inlay_hints: Vec::new(),
            hint_galleys: std::collections::HashMap::new(),
            git_diff: None,
            breakpoints: std::collections::HashSet::new(),
            stopped_line: None,
            layout_cache: crate::syntax::LineLayoutCache::new(),
            ast_rev: 0,
            last_activity_time: 0.0,
            last_pixels_per_point: 0.0,
            last_texture_atlas_ptr: 0,
            smear_head: None,
            smear_tail: None,
            smear_last_target: None,
            smear_enabled: true,
        }
    }
}

impl EditorState {
    /// F4 (Roadmap v0.2.30): determina si una línea lógica está oculta dentro de un bloque plegado.
    pub fn is_line_folded(&self, line: usize) -> bool {
        self.folding_ranges.iter().any(|r| {
            self.collapsed_folds.contains(&r.start_line)
                && line > r.start_line
                && line <= r.end_line
        })
    }

    /// F4 (Roadmap v0.2.30): pliega o despliega el bloque que inicia en `start_line`.
    pub fn toggle_fold(&mut self, start_line: usize) {
        if let Some(pos) = self.collapsed_folds.iter().position(|&l| l == start_line) {
            self.collapsed_folds.remove(pos);
        } else {
            self.collapsed_folds.push(start_line);
        }
        self.request_autoscroll();
    }

    /// Pide desplazar la vista hasta el cursor en el próximo frame.
    pub fn request_autoscroll(&mut self) {
        self.needs_autoscroll = true;
    }

    /// S10: zoom in (+1px).
    pub fn zoom_in(&mut self) {
        self.set_font_size(self.font_size + 1.0);
    }

    /// S10: zoom out (-1px).
    pub fn zoom_out(&mut self) {
        self.set_font_size(self.font_size - 1.0);
    }

    /// S10: fija el tamaño de fuente acotado al rango válido e invalida
    /// la caché de layouts para evitar usar galleys con coordenadas de atlas desactualizadas.
    pub fn set_font_size(&mut self, size: f32) {
        let new_size = clamp_font_size(size);
        if (self.font_size - new_size).abs() > f32::EPSILON {
            self.font_size = new_size;
            self.layout_cache.clear();
            self.hint_galleys.clear();
            self.smear_head = None;
            self.smear_tail = None;
            self.request_autoscroll();
        }
    }
}

pub struct EditorResponse {
    pub changed: bool,
    pub lost_focus: bool,
    pub pre_break_snapshot: Option<String>,
    pub force_snapshot: bool,
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    if s.is_ascii() {
        return char_idx.min(s.len());
    }
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

/// Public wrapper for char_to_byte (used by app.rs find/replace).
pub fn char_to_byte_pub(s: &str, char_idx: usize) -> usize {
    char_to_byte(s, char_idx)
}

fn line_char_count(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().count()
    }
}

/// S4: ancho de la banda de números: dígitos del total de líneas +
/// padding fijo. Crecer del documento = ancho estable en frames.
fn gutter_width_for(total_lines: usize, char_width: f32) -> f32 {
    let digits = total_lines.max(1).to_string().len();
    digits as f32 * char_width + GUTTER_PAD * 2.0
}

const GUTTER_PAD: f32 = 4.0;

// Margen para que el texto no arranque pegado al borde del panel.
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
// Ancho de la franja de la barra de scroll vertical, a la derecha.
const SCROLLBAR_W: f32 = 12.0;
// F9 (Roadmap v0.2.36): ancho del minimap lateral.
const MINIMAP_W: f32 = 64.0;

/// F9: franja visible del minimap (top, alto) dentro de su altura total.
/// `scroll_frac` y `visible_frac` van en 0.0..=1.0.
pub fn minimap_viewport(scroll_frac: f32, visible_frac: f32, height: f32) -> (f32, f32) {
    let h = (visible_frac.clamp(0.0, 1.0) * height).max(6.0).min(height);
    let top = (scroll_frac.clamp(0.0, 1.0) * (height - h)).max(0.0);
    (top, h)
}

/// F9: mapea cada píxel vertical del minimap a la línea fuente que muestrea.
/// Un documento corto (que entra entero en la altura) devuelve una entrada por
/// línea; uno largo devuelve una entrada por píxel, cubriendo el 0%..100% del
/// archivo sin el truncado del antiguo `step.max(1.0)` + `break`.
fn minimap_line_mapping(total_lines: usize, mm_height: usize) -> Vec<usize> {
    if total_lines == 0 {
        return Vec::new();
    }
    let mm_height = mm_height.max(1);
    if total_lines <= mm_height {
        (0..total_lines).collect()
    } else {
        (0..mm_height)
            .map(|py| py * total_lines / mm_height)
            .collect()
    }
}

// S2 (Roadmap v0.2.9): período del parpadeo del caret, en segundos.
// El pesto SOLIDO durante CARET_BLINK_PERIOD_S tras cada interacción y
// alterna ON/OFF cada medio período después: 1 Hz visible clásico de editor.
pub const CARET_BLINK_PERIOD_S: f64 = 0.5;

/// S2 (Roadmap v0.2.9): ¿ debería verse el caret en el instante `now`?
/// Sólido durante el primer período tras `last_activity` y parpadeando
/// después: en [P, 2P) apagado, en [2P, 3P) encendido, etc. (P = período).
fn caret_blink_visible(now: f64, last_activity: f64) -> bool {
    let idle = (now - last_activity).max(0.0);
    if idle < CARET_BLINK_PERIOD_S {
        return true;
    }
    ((idle / CARET_BLINK_PERIOD_S).floor() as u64).is_multiple_of(2)
}

/// S2 (Roadmap v0.2.9): segundos hasta el próximo cruce de fase (para
/// `request_repaint_after`): el parpadeo vive con eventos, nunca en
/// loop continuo — reposo efectivo ~0% de CPU salvo el blink a 2 Hz.
fn caret_blink_next_wake(now: f64, last_activity: f64) -> f32 {
    let idle = (now - last_activity).max(0.0);
    (CARET_BLINK_PERIOD_S - (idle % CARET_BLINK_PERIOD_S)) as f32
}

fn clamp_col(lines: &dyn LineBuffer, line: usize, col: usize) -> usize {
    col.min(lines.line_char_len(line))
}

/// S12: Desplaza el cursor una línea hacia arriba, respetando líneas plegadas
/// y manteniendo la columna preferida a través de líneas vacías o más cortas.
pub fn move_cursor_up(lines: &dyn LineBuffer, state: &mut EditorState, shift: bool) {
    if state.cursor_line > 0 {
        let mut target_line = state.cursor_line - 1;
        while target_line > 0 && state.is_line_folded(target_line) {
            target_line -= 1;
        }
        if !state.is_line_folded(target_line) {
            state.cursor_line = target_line;
            let pref = *state.preferred_col.get_or_insert(state.cursor_col);
            state.cursor_col = clamp_col(lines, state.cursor_line, pref);
        }
    }
    if !shift {
        state.sel_anchor = Some((state.cursor_line, state.cursor_col));
    }
}

/// S12: Desplaza el cursor una línea hacia abajo, respetando líneas plegadas
/// y manteniendo la columna preferida a través de líneas vacías o más cortas.
pub fn move_cursor_down(lines: &dyn LineBuffer, state: &mut EditorState, shift: bool) {
    if state.cursor_line + 1 < lines.line_count() {
        let mut target_line = state.cursor_line + 1;
        while target_line + 1 < lines.line_count() && state.is_line_folded(target_line) {
            target_line += 1;
        }
        if !state.is_line_folded(target_line) {
            state.cursor_line = target_line;
            let pref = *state.preferred_col.get_or_insert(state.cursor_col);
            state.cursor_col = clamp_col(lines, state.cursor_line, pref);
        }
    }
    if !shift {
        state.sel_anchor = Some((state.cursor_line, state.cursor_col));
    }
}

fn selection_range(state: &EditorState) -> Option<((usize, usize), (usize, usize))> {
    let anchor = state.sel_anchor?;
    let cursor = (state.cursor_line, state.cursor_col);
    if anchor == cursor {
        return None;
    }
    Some(if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) })
}

fn selected_text(lines: &dyn LineBuffer, start: (usize, usize), end: (usize, usize)) -> String {
    let (sl, sc) = start;
    let (el, ec) = end;
    if sl == el {
        let line = lines.get_line(sl);
        let sb = char_to_byte(&line, sc);
        let eb = char_to_byte(&line, ec);
        line[sb..eb].to_string()
    } else {
        let mut out = String::new();
        let first = lines.get_line(sl);
        let sb = char_to_byte(&first, sc);
        out.push_str(&first[sb..]);
        for line_idx in sl + 1..el {
            out.push('\n');
            out.push_str(&lines.get_line(line_idx));
        }
        out.push('\n');
        let last = lines.get_line(el);
        let eb = char_to_byte(&last, ec);
        out.push_str(&last[..eb]);
        out
    }
}

fn delete_range(lines: &mut dyn LineBuffer, start: (usize, usize), end: (usize, usize)) {
    let (sl, sc) = start;
    let (el, ec) = end;
    if sl == el {
        let line = lines.get_line(sl);
        let sb = char_to_byte(&line, sc);
        let eb = char_to_byte(&line, ec);
        let mut new_line = line;
        new_line.replace_range(sb..eb, "");
        lines.set_line(sl, &new_line);
    } else {
        let first = lines.get_line(sl);
        let sb = char_to_byte(&first, sc);
        let last = lines.get_line(el);
        let eb = char_to_byte(&last, ec);
        let mut merged = first[..sb].to_string();
        merged.push_str(&last[eb..]);
        lines.set_line(sl, &merged);
        lines.remove_lines(sl + 1..el + 1);
    }
}

/// El portapapeles del sistema (especialmente en Windows/WSL) llega
/// con finales CRLF; el editor trabaja solo con '\n', así que quedan
/// caracteres '\r' invisibles si no se normaliza antes de insertar.
fn normalize_pasted(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// S6: sangría (espacios/tabs iniciales) de una línea, copiada tal
/// cual para la línea nueva al apretar Enter — convención de
/// editores como VS Code. Solo el whitespace INICIAL: el de la
/// derecha es contenido del texto, no sangría.
fn inherited_indent(prev_line: &str) -> String {
    prev_line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// S4 (Etapa 2): desindenta una línea quitando un '\t' inicial o hasta 4 espacios iniciales.
/// Devuelve la cantidad de caracteres de sangría eliminados.
fn dedent_line(line: &mut String) -> usize {
    if line.starts_with('\t') {
        line.remove(0);
        1
    } else if line.starts_with(' ') {
        let spaces = line.chars().take(4).take_while(|&c| c == ' ').count();
        line.drain(0..spaces);
        spaces
    } else {
        0
    }
}

/// S4 (Etapa 2): indenta una línea insertando 4 espacios al inicio (soft tab).
fn indent_line(line: &mut String) {
    line.insert_str(0, "    ");
}

/// S6: Comenta o descomenta el rango de líneas [start_line..=end_line] usando el prefijo indicado.
pub fn toggle_comment_range(
    lines: &mut dyn LineBuffer,
    start_line: usize,
    end_line: usize,
    prefix: &str,
) {
    let count = lines.line_count();
    if count == 0 {
        return;
    }
    let end_l = end_line.min(count - 1);
    let start_l = start_line.min(end_l);

    let all_commented = (start_l..=end_l).all(|l| {
        let line = lines.get_line(l);
        let trimmed = line.trim_start();
        trimmed.is_empty() || trimmed.starts_with(prefix)
    });

    for l in start_l..=end_l {
        let mut line = lines.get_line(l);
        if line.trim().is_empty() {
            continue;
        }
        if all_commented {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(&line, indent_len);
            let rest = &line[indent_bytes..];
            if let Some(stripped) = rest.strip_prefix(prefix) {
                let strip_len = if stripped.starts_with(' ') {
                    prefix.len() + 1
                } else {
                    prefix.len()
                };
                line.replace_range(indent_bytes..indent_bytes + strip_len, "");
            }
        } else {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(&line, indent_len);
            let insert = format!("{} ", prefix);
            line.insert_str(indent_bytes, &insert);
        }
        lines.set_line(l, &line);
    }
}

/// Texto que Ctrl+C/Ctrl+X copiarian: la selección si hay, y si no
/// la línea LÓGICA completa bajo el cursor (convención de editores
/// tipo VS Code). egui-winit consume Ctrl+C/X y solo emite
/// Event::Copy / Event::Cut sin payload, así que la decisión vive
/// en este helper puro.
fn copy_text(
    lines: &dyn LineBuffer,
    sel: Option<((usize, usize), (usize, usize))>,
    cursor_line: usize,
) -> String {
    match sel {
        Some((s, e)) => selected_text(lines, s, e),
        None => lines.get_line(cursor_line.min(lines.line_count().saturating_sub(1))),
    }
}

/// Inserta `text` (puede tener saltos de línea, ej. al pegar) en
/// `at`, y devuelve la posición del cursor después de insertar.
pub(crate) fn insert_text(
    lines: &mut dyn LineBuffer,
    at: (usize, usize),
    text: &str,
) -> (usize, usize) {
    let (line, col) = at;
    let content = lines.get_line(line);
    let byte = char_to_byte(&content, col);

    if !text.contains('\n') {
        let mut new_line = content;
        new_line.insert_str(byte, text);
        lines.set_line(line, &new_line);
        return (line, col + text.chars().count());
    }

    let tail = content[byte..].to_string();
    let mut parts: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
    let first = parts.remove(0);
    let mut head = content[..byte].to_string();
    head.push_str(&first);
    let head_len = head.chars().count();
    lines.set_line(line, &head);

    let last_idx = parts.len().saturating_sub(1);
    let mut insert_at = line + 1;
    let mut result = (line, head_len);
    for (i, part) in parts.into_iter().enumerate() {
        if i == last_idx {
            let col_after = part.chars().count();
            let mut final_line = part;
            final_line.push_str(&tail);
            lines.insert_line(insert_at, final_line);
            result = (insert_at, col_after);
        } else {
            lines.insert_line(insert_at, part);
            insert_at += 1;
        }
    }
    result
}

/// S5: runa alfanumérica unicode (á, ñ, dígitos, etc.) define los límites de palabra.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// S5: salto por palabra adelante (Ctrl+→). Si está en una palabra, salta al fin de la palabra.
/// Si está en separadores, salta al inicio de la siguiente palabra.
fn next_word_pos(lines: &dyn LineBuffer, line: usize, col: usize) -> (usize, usize) {
    let count = lines.line_count();
    if count == 0 {
        return (0, 0);
    }
    let line = line.min(count - 1);
    let chars: Vec<char> = lines.get_line(line).chars().collect();
    let len = chars.len();
    if col >= len {
        if line + 1 < count {
            (line + 1, 0)
        } else {
            (line, len)
        }
    } else {
        let mut c = col;
        if is_word_char(chars[c]) {
            while c < len && is_word_char(chars[c]) {
                c += 1;
            }
        } else {
            while c < len && !is_word_char(chars[c]) {
                c += 1;
            }
        }
        (line, c)
    }
}

/// S5: salto por palabra atrás (Ctrl+←). Retrocede sobre la palabra o separador actual
/// hasta el límite anterior.
fn prev_word_pos(lines: &dyn LineBuffer, line: usize, col: usize) -> (usize, usize) {
    let count = lines.line_count();
    if count == 0 {
        return (0, 0);
    }
    if col == 0 {
        if line > 0 {
            (line - 1, lines.line_char_len(line - 1))
        } else {
            (0, 0)
        }
    } else {
        let line = line.min(count - 1);
        let chars: Vec<char> = lines.get_line(line).chars().collect();
        let mut c = col.min(chars.len());
        while c > 0 {
            c -= 1;
            if !is_word_char(chars[c]) {
                break;
            }
        }
        (line, c)
    }
}

/// S5: rango [start, end) de la palabra bajo `col` (para doble click).
fn word_range_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let c = if col >= chars.len() {
        chars.len() - 1
    } else {
        col
    };
    if is_word_char(chars[c]) {
        let mut start = c;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = c + 1;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }
        (start, end)
    } else {
        (c, c + 1)
    }
}

/// S5: destino de fila visual al paginar con PageUp / PageDown, acotado a [0, total-1].
fn page_target_row(current_row: usize, total_rows: usize, page_rows: usize, down: bool) -> usize {
    if total_rows == 0 {
        return 0;
    }
    let max_row = total_rows.saturating_sub(1);
    if down {
        (current_row + page_rows).min(max_row)
    } else {
        current_row.saturating_sub(page_rows)
    }
}

/// S3 (Etapa 2): resultado de búsqueda de delimitador coincidente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BracketMatch {
    pub origin: (usize, usize),
    pub partner: Option<(usize, usize)>,
}

fn matching_partner(c: char) -> Option<(char, bool)> {
    match c {
        '(' => Some((')', true)),
        '[' => Some((']', true)),
        '{' => Some(('}', true)),
        ')' => Some(('(', false)),
        ']' => Some(('[', false)),
        '}' => Some(('{', false)),
        _ => None,
    }
}

pub fn find_matching_bracket(
    lines: &dyn LineBuffer,
    cursor_line: usize,
    cursor_col: usize,
) -> Option<BracketMatch> {
    let count = lines.line_count();
    if count == 0 || cursor_line >= count {
        return None;
    }
    let cur_line = lines.get_line(cursor_line);
    let cur_chars: Vec<char> = cur_line.chars().collect();

    let (origin_col, ch) = if cursor_col < cur_chars.len()
        && matching_partner(cur_chars[cursor_col]).is_some()
    {
        (cursor_col, cur_chars[cursor_col])
    } else if cursor_col > 0
        && cursor_col - 1 < cur_chars.len()
        && matching_partner(cur_chars[cursor_col - 1]).is_some()
    {
        (cursor_col - 1, cur_chars[cursor_col - 1])
    } else {
        return None;
    };

    let (partner_ch, forward) = matching_partner(ch)?;
    let origin = (cursor_line, origin_col);

    let max_lines = 2000;
    if forward {
        let mut depth = 1usize;
        let mut start_c = origin_col + 1;
        let end_line = (cursor_line + max_lines).min(count);
        for l in cursor_line..end_line {
            let line = lines.get_line(l);
            for (c_idx, c) in line.chars().enumerate().skip(start_c) {
                if c == ch {
                    depth += 1;
                } else if c == partner_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some(BracketMatch {
                            origin,
                            partner: Some((l, c_idx)),
                        });
                    }
                }
            }
            start_c = 0;
        }
    } else {
        let mut depth = 1usize;
        let start_c = origin_col;
        let min_line = cursor_line.saturating_sub(max_lines);
        for l in (min_line..=cursor_line).rev() {
            let line_chars: Vec<char> = lines.get_line(l).chars().collect();
            let from = if l == cursor_line { start_c } else { line_chars.len() };
            for c_idx in (0..from).rev() {
                let c = line_chars[c_idx];
                if c == ch {
                    depth += 1;
                } else if c == partner_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some(BracketMatch {
                            origin,
                            partner: Some((l, c_idx)),
                        });
                    }
                }
            }
        }
    }

    Some(BracketMatch {
        origin,
        partner: None,
    })
}

// ============================================================
// Smart delimiters & auto-closing pairs (Roadmap v0.2.50)
// ============================================================

/// Resultado de procesar un único carácter de delimitador en el cursor primario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelimiterOutcome {
    /// Se insertó o borró texto: el llamador debe marcar el documento como modificado.
    TextMutated,
    /// Solo se movió el cursor (overtyping): no hubo cambio de texto.
    CursorMoved,
    /// No era un caso especial de delimitador: seguir con la inserción normal.
    Unhandled,
}

/// Maneja la escritura de un único carácter `c` en el cursor primario.
///
/// Aplica, en orden:
/// 1. **Selection wrap**: con selección y `c` de apertura, envuelve el texto
///    seleccionado entre `c` y su cierre.
/// 2. **Overtyping**: sin selección y `c` de cierre, si el carácter a la derecha
///    es exactamente `c`, avanza el cursor sin duplicarlo.
/// 3. **Auto-cierre**: sin selección y `c` de apertura, si a la derecha hay fin
///    de línea, espacio o delimitador de cierre, inserta `c`+cierre y deja el
///    cursor en el medio.
fn handle_delimiter_char(
    lines: &mut dyn LineBuffer,
    state: &mut EditorState,
    c: char,
) -> DelimiterOutcome {
    // 1. Selection wrap.
    if let Some((s, e)) = selection_range(state) {
        if let Some(close) = closing_delimiter(c) {
            let sel_text = selected_text(lines, s, e);
            delete_range(lines, s, e);
            let wrapped = format!("{}{}{}", c, sel_text, close);
            let (nl, nc) = insert_text(lines, s, &wrapped);
            state.cursor_line = nl;
            state.cursor_col = nc;
            state.sel_anchor = Some((nl, nc));
            return DelimiterOutcome::TextMutated;
        }
        return DelimiterOutcome::Unhandled;
    }

    // 2. Overtyping (sin selección).
    if is_closing_delimiter(c) {
        let (l, col) = (state.cursor_line, state.cursor_col);
        let chars: Vec<char> = lines.get_line(l).chars().collect();
        if chars.get(col) == Some(&c) {
            let is_quote = matches!(c, '"' | '\'' | '`');
            let should_overtype = if is_quote {
                if col == 0 {
                    false
                } else {
                    let prev = chars[col - 1];
                    prev == c || (!prev.is_whitespace() && !matches!(prev, '(' | '[' | '{' | '=' | ',' | ':' | '<'))
                }
            } else {
                true
            };

            if should_overtype {
                state.cursor_col = col + 1;
                state.sel_anchor = Some((l, col + 1));
                return DelimiterOutcome::CursorMoved;
            }
        }
    }

    // 3. Auto-cierre (sin selección).
    if let Some(close) = closing_delimiter(c) {
        let (l, col) = (state.cursor_line, state.cursor_col);
        let after = lines.get_line(l).chars().nth(col);
        let should_auto_close = match after {
            None => true, // fin de línea
            Some(ch) => ch.is_whitespace() || is_closing_delimiter(ch),
        };
        if should_auto_close {
            let pair = format!("{}{}", c, close);
            let (nl, nc) = insert_text(lines, (l, col), &pair);
            state.cursor_line = nl;
            state.cursor_col = nc - 1; // en el medio de la pareja
            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
            return DelimiterOutcome::TextMutated;
        }
    }

    DelimiterOutcome::Unhandled
}

/// Borra un par vacío `()`, `[]`, `{}`, `""`, `''` o `` `` `` alrededor del
/// cursor primario. Devuelve `true` si borró ambos caracteres.
fn backspace_empty_pair(lines: &mut dyn LineBuffer, state: &mut EditorState) -> bool {
    if selection_range(state).is_some() {
        return false;
    }
    let (l, c) = (state.cursor_line, state.cursor_col);
    if !is_empty_pair_around(&lines.get_line(l), c) {
        return false;
    }
    delete_range(lines, (l, c - 1), (l, c + 1));
    state.cursor_col = c - 1;
    state.sel_anchor = Some((l, c - 1));
    true
}

/// Expande un bloque `{|}` en tres líneas con indentación expandida y deja el
/// cursor en el renglón intermedio. Devuelve `true` si lo hizo.
fn enter_between_braces(lines: &mut dyn LineBuffer, state: &mut EditorState) -> bool {
    let line_idx = state.cursor_line;
    let col = state.cursor_col;
    let between_braces = {
        let chars: Vec<char> = lines.get_line(line_idx).chars().collect();
        col > 0 && col < chars.len() && chars[col - 1] == '{' && chars[col] == '}'
    };
    if !between_braces {
        return false;
    }
    let base = inherited_indent(&lines.get_line(line_idx));
    let base_len = line_char_count(&base);
    let insert = format!("\n{}    \n{}", base, base);
    let (closing_line, _) = insert_text(lines, (line_idx, col), &insert);
    state.cursor_line = closing_line - 1;
    state.cursor_col = base_len + 4;
    state.sel_anchor = Some((state.cursor_line, state.cursor_col));
    true
}

// ============================================================
// Ajuste de línea (word wrap) — todo lo de acá para abajo asume
// fuente monoespaciada, así que "wrap_chars" caracteres entran
// exactos en el ancho disponible.
// ============================================================

fn wrap_chars_for(usable_width: f32, char_width: f32) -> usize {
    ((usable_width / char_width).floor() as usize).max(1)
}

/// x (relativa al inicio del texto) del borde izquierdo de la columna
/// visual `col`, según el galley que realmente se dibuja. egui redondea
/// la posición de cada glifo a píxeles físicos, así que multiplicar la
/// columna por el ancho nominal de la fuente acumula un desfase de
/// fracciones de píxel por carácter. Esta es la única fuente de verdad
/// para caret, selección y clics.
fn x_for_col(galley: &egui::Galley, col: usize) -> f32 {
    galley
        .pos_from_ccursor(egui::text::CCursor::new(col))
        .min
        .x
}

/// Columna visual más cercana a `x` según el galley (para clics y
/// arrastre). Egui ya redondea a la frontera de carácter más próxima,
/// así que un clic en la mitad izquierda de un glifo cae en su columna.
fn col_for_x(galley: &egui::Galley, x: f32) -> usize {
    match galley.rows.first() {
        Some(row) => row.char_at(x),
        None => 0,
    }
}

fn row_count_for_len(len: usize, wrap_chars: usize) -> usize {
    if len == 0 {
        1
    } else {
        len.div_ceil(wrap_chars)
    }
}

fn row_count_for(line: &str, wrap_chars: usize) -> usize {
    row_count_for_len(
        karkinos_core::text_ops::line_visual_char_count(line, DEFAULT_TAB_SIZE),
        wrap_chars,
    )
}

/// Largo de una línea del Rope sin su salto final y si contiene tabs, con
/// la misma regla que `TextBuffer::line_char_count`, sin materializar un
/// `String`. `as_str()` evita navegar el árbol por cada carácter cuando la
/// línea cae en un único chunk (el caso habitual).
fn slice_line_info(slice: ropey::RopeSlice<'_>) -> (usize, bool) {
    let total = slice.len_chars();
    if let Some(s) = slice.as_str() {
        let trailing = if s.ends_with("\r\n") {
            2
        } else if s.ends_with('\n') {
            1
        } else {
            0
        };
        (total - trailing, s.contains('\t'))
    } else {
        let mut len = total;
        if len > 0 && slice.char(len - 1) == '\n' {
            len -= 1;
            if len > 0 && slice.char(len - 1) == '\r' {
                len -= 1;
            }
        }
        (len, slice.chars().any(|c| c == '\t'))
    }
}

fn slice_char_len(slice: ropey::RopeSlice<'_>) -> usize {
    slice_line_info(slice).0
}

/// Ancho visual de la línea (sin salto) expandiendo los tabs igual que
/// `line_visual_char_count`.
fn slice_visual_len(slice: ropey::RopeSlice<'_>, len: usize) -> usize {
    if let Some(s) = slice.as_str() {
        let trailing = if s.ends_with("\r\n") {
            2
        } else if s.ends_with('\n') {
            1
        } else {
            0
        };
        karkinos_core::text_ops::line_visual_char_count(&s[..s.len() - trailing], DEFAULT_TAB_SIZE)
    } else {
        let mut vcol = 0usize;
        for c in slice.chars().take(len) {
            if c == '\t' {
                vcol += DEFAULT_TAB_SIZE - (vcol % DEFAULT_TAB_SIZE);
            } else {
                vcol += 1;
            }
        }
        vcol
    }
}

fn rebuild_row_counts(lines: &dyn LineBuffer, wrap_chars: usize, state: &EditorState) -> Vec<usize> {
    lines.rebuild_row_counts(wrap_chars, state)
}

fn rebuild_caches(
    lines: &dyn LineBuffer,
    wrap_chars: usize,
    state: &EditorState,
) -> (Vec<usize>, Vec<usize>) {
    lines.rebuild_caches(wrap_chars, state)
}

/// cum_rows[0] = 0 y cum_rows[i] = suma de row_counts[0..i).
fn build_cum_rows(row_counts: &[usize]) -> Vec<usize> {
    let mut cum = Vec::with_capacity(row_counts.len() + 1);
    let mut acc = 0usize;
    cum.push(0);
    for &rc in row_counts {
        acc += rc;
        cum.push(acc);
    }
    cum
}

/// El tamaño del documento (en filas visuales) cambió en `delta`: aplica
/// ese shift a todas las sumatorias posteriores a la línea editada.
fn adjust_cum_rows(cum_rows: &mut [usize], line: usize, delta: isize) {
    if delta == 0 {
        return;
    }
    let from = (line + 1).min(cum_rows.len());
    for c in &mut cum_rows[from..] {
        *c = (*c as isize + delta).max(0) as usize;
    }
}

/// Índice de la línea lógica que contiene (o de donde arrancaría) la
/// fila visual absoluta `row`, por búsqueda binaria sobre cum_rows.
/// Requiere cum_rows.len() >= 2 (siempre hay al menos 1 línea).
fn line_at_visual_row(cum_rows: &[usize], row: usize) -> usize {
    let idx = cum_rows.partition_point(|&c| c <= row).saturating_sub(1);
    idx.min(cum_rows.len().saturating_sub(2))
}

/// Dado un Y en "espacio de documento" (ya sumado el scroll), devuelve
/// (línea lógica, sub-fila dentro de esa línea).
fn line_and_subrow_at_doc_y(cum_rows: &[usize], doc_y: f32, row_height: f32) -> (usize, usize) {
    let target_row = (doc_y / row_height).floor().max(0.0) as usize;
    let line = line_at_visual_row(cum_rows, target_row);
    // Un click muy por debajo del final del documento cae acá con un
    // sub_row fuera de rango: el llamador acota la columna contra la
    // longitud de la línea, igual que antes.
    (line, target_row - cum_rows[line])
}

/// Fila visual absoluta (contando desde el principio del documento)
/// en la que cae (line, col).
fn visual_row_of(cum_rows: &[usize], line: usize, col: usize, wrap_chars: usize) -> usize {
    cum_rows[line.min(cum_rows.len().saturating_sub(1))] + col / wrap_chars
}

/// S2 (Etapa 2): datos de coincidencias de búsqueda para resaltar en el viewport.
pub struct SearchHighlight<'a> {
    pub matches: &'a [(usize, usize)],
    pub query_len: usize,
    pub current_idx: usize,
}

/// Dibuja y maneja el editor de texto, mostrando SOLO las filas que
/// entran en pantalla (a diferencia de egui::TextEdit, que relayoutea
/// el texto entero en cada tecla). Ajusta de línea automáticamente:
/// una línea lógica larga ocupa varias filas visuales, así nunca se
/// sale de la pantalla hacia los costados.
pub fn show(
    ui: &mut egui::Ui,
    lines: &mut TextBuffer,
    state: &mut EditorState,
    theme: &crate::theme::Theme,
    language: crate::syntax::Language,
    search: Option<SearchHighlight<'_>>,
) -> EditorResponse {
    let mut changed = false;
    let mut force_snapshot = false;
    let mut pre_break_snapshot: Option<String> = None;

    // Invalida galleys si cambió pixels_per_point o si epaint recreó el atlas de texturas
    // (por ejemplo por fill_ratio > 0.8 o overflow), evitando que UVs viejos apunten
    // a glifos corruptos en el nuevo atlas.
    let ppp = ui.ctx().pixels_per_point();
    let atlas_ptr = ui.fonts(|f| std::sync::Arc::as_ptr(&f.texture_atlas()) as usize);
    if (state.last_pixels_per_point - ppp).abs() > 0.001 || state.last_texture_atlas_ptr != atlas_ptr {
        state.last_pixels_per_point = ppp;
        state.last_texture_atlas_ptr = atlas_ptr;
        state.layout_cache.clear();
        state.hint_galleys.clear();
    }

    // S10: el FontId sale del estado (zoomable), no de un literal:
    // medir char_width/row_height con este FontId hace que wrap_chars,
    // el gutter y el autoscroll se re-adjusten solos al zoomear.
    let font_id = egui::FontId::monospace(state.font_size);
    let row_height = ui.fonts(|f| f.row_height(&font_id));
    let char_width = ui.fonts(|f| f.glyph_width(&font_id, ' '));
    let text_color = theme.text;
    let sel_color = theme.selection;

    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());

    let id = response.id;
    let has_focus_before = ui.memory(|m| m.has_focus(id));
    let lost_focus = state.was_focused && !has_focus_before;
    state.was_focused = has_focus_before;

    // S4: banda de números a la izquierda del texto — ancho según
    // los dígitos del total de líneas (crece con el documento).
    let gutter_w = gutter_width_for(lines.line_count(), char_width);
    // Origen del texto: después de la banda (y del PAD_X de siempre).
    let text_left = rect.left() + gutter_w + PAD_X;

    // Reservamos SIEMPRE el ancho de la scrollbar y del gutter para
    // calcular el wrap — así el punto de corte de las líneas no
    // "salta" cuando la banda gana un dígito o aparece la barra.
    // UI-7 (v0.2.38): sin minimap no se reserva ningún píxel a la derecha.
    let minimap_w = if state.show_minimap { MINIMAP_W } else { 0.0 };
    let usable_width =
        (rect.width() - gutter_w - PAD_X - SCROLLBAR_W - minimap_w).max(10.0);
    let wrap_chars = wrap_chars_for(usable_width, char_width);

    // Reconstruir la caché de filas por línea SOLO si cambió la
    // cantidad de líneas o el ancho disponible (abrir archivo,
    // deshacer/rehacer, redimensionar ventana, gutter que crece) —
    // en el caso común (tipear un carácter) esto se saltea, y más
    // abajo parcheamos solo la línea que cambió.
    if state.row_counts.len() != lines.line_count()
        || state.last_wrap_chars != wrap_chars
        || state.last_gutter != gutter_w
        || state.last_collapsed_folds != state.collapsed_folds
    {
        let (row_counts, char_lens) = rebuild_caches(lines, wrap_chars, state);
        state.row_counts = row_counts;
        state.char_lens = char_lens;
        state.cum_rows = build_cum_rows(&state.row_counts);
        state.last_wrap_chars = wrap_chars;
        state.last_gutter = gutter_w;
        state.last_collapsed_folds = state.collapsed_folds.clone();
    }

    let total_rows: usize = *state.cum_rows.last().unwrap_or(&0);
    let total_height = total_rows as f32 * row_height;
    let max_scroll = (total_height - rect.height()).max(0.0);
    let show_scrollbar = max_scroll > 0.0;
    let scrollbar_track = egui::Rect::from_min_max(
        egui::pos2(rect.right() - SCROLLBAR_W - minimap_w, rect.top()),
        egui::pos2(rect.right() - minimap_w, rect.bottom()),
    );
    // F9/UI-7: tira del minimap, a la derecha de la barra (ancho 0 si está apagado).
    let minimap_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - minimap_w, rect.top()),
        egui::pos2(rect.right(), rect.bottom()),
    );

    let mut cursor_moved = false;

    // --- barra de scroll vertical: se maneja ANTES que el click de
    // texto, para que un click ahí no mueva el cursor del texto ---
    let mut over_scrollbar = false;
    if show_scrollbar {
        let scrollbar_response =
            ui.interact(scrollbar_track, id.with("vscrollbar"), egui::Sense::click_and_drag());
        if let Some(pos) = scrollbar_response.interact_pointer_pos() {
            over_scrollbar = true;
            if scrollbar_response.dragged() || scrollbar_response.clicked() {
                let thumb_h = (rect.height() / total_height * rect.height()).max(24.0);
                let usable = (rect.height() - thumb_h).max(1.0);
                let rel = ((pos.y - rect.top() - thumb_h / 2.0) / usable).clamp(0.0, 1.0);
                // OPT-3: arrastrar la barra es control directo, sin animación.
                state.scroll = rel * max_scroll;
                state.target_scroll = state.scroll;
            }
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if pos.x >= scrollbar_track.left() {
                    over_scrollbar = true;
                }
            }
        }
    }

    // --- click / arrastre sobre el TEXTO ---
    if !over_scrollbar {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x < scrollbar_track.left() || !show_scrollbar {
                if response.clicked() || response.drag_started() {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                let doc_y = pos.y - rect.top() - PAD_Y + state.scroll;
                let (clicked_line, sub_row) =
                    line_and_subrow_at_doc_y(&state.cum_rows, doc_y.max(0.0), row_height);

                if pos.x < text_left {
                    // F4 (Roadmap v0.2.30): clic en el gutter sobre una línea con bloque colapsable pliega o despliega
                    if response.clicked()
                        && state
                            .folding_ranges
                            .iter()
                            .any(|r| r.start_line == clicked_line)
                    {
                        state.toggle_fold(clicked_line);
                        state.row_counts = rebuild_row_counts(lines, wrap_chars, state);
                        state.cum_rows = build_cum_rows(&state.row_counts);
                        state.last_collapsed_folds = state.collapsed_folds.clone();
                    } else {
                        // S8: Clic en el gutter (banda de números) selecciona la línea completa
                        let line_end = lines.line_char_len(clicked_line);
                        state.sel_anchor = Some((clicked_line, 0));
                        state.cursor_line = clicked_line;
                        state.cursor_col = line_end;
                        state.preferred_col = None;
                    }
                } else {
                    let rel_x = (pos.x - text_left).max(0.0);
                    let target_line = lines.get_line(clicked_line);
                    // T4 (S3): la columna se resuelve con el MISMO layout
                    // que se dibuja (tabs expandidos incluidos), no con
                    // `rel_x / char_width`: el redondeo a píxeles de cada
                    // glifo hace que la fórmula fija se corra una columna.
                    let start = sub_row * wrap_chars;
                    let end = (start + wrap_chars).min(line_char_count(&target_line));
                    let sb = if target_line.is_ascii() {
                        start
                    } else {
                        char_to_byte(&target_line, start)
                    };
                    let eb = if target_line.is_ascii() {
                        end
                    } else {
                        char_to_byte(&target_line, end)
                    };
                    let vstart = visual_col_at(&target_line, start, DEFAULT_TAB_SIZE);
                    let chunk = &target_line[sb.min(eb)..eb];
                    let galley = ui.fonts(|f| {
                        f.layout_no_wrap(
                            crate::syntax::expand_tabs(chunk, vstart, DEFAULT_TAB_SIZE),
                            font_id.clone(),
                            text_color,
                        )
                    });
                    let target_vcol = col_for_x(&galley, rel_x);
                    let clicked_col = col_from_visual_col(
                        &target_line,
                        vstart + target_vcol,
                        DEFAULT_TAB_SIZE,
                    );

                    // F3: Alt+Click agrega un cursor extra sin mover el primario.
                    if ui.input(|i| i.modifiers.alt) {
                        state.extra_carets.push(Caret {
                            line: clicked_line,
                            col: clicked_col,
                            anchor: None,
                        });
                        dedup_extra_carets(state);
                    // S5: doble click selecciona palabra, triple click selecciona línea lógica
                    } else if response.triple_clicked() {
                        state.sel_anchor = Some((clicked_line, 0));
                        state.cursor_line = clicked_line;
                        state.cursor_col = lines.line_char_len(clicked_line);
                        state.preferred_col = None;
                    } else if response.double_clicked() {
                        let (ws, we) = word_range_at(&lines.get_line(clicked_line), clicked_col);
                        state.sel_anchor = Some((clicked_line, ws));
                        state.cursor_line = clicked_line;
                        state.cursor_col = we;
                        state.preferred_col = None;
                    } else {
                        let shift = ui.input(|i| i.modifiers.shift);
                        if !shift && !response.dragged() && !response.drag_stopped() {
                            state.sel_anchor = Some((clicked_line, clicked_col));
                        } else if shift && state.sel_anchor.is_none() {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                        state.cursor_line = clicked_line;
                        state.cursor_col = clicked_col;
                        state.preferred_col = None;
                    }
                }
                cursor_moved = true;
            }
        }
    }
    // S4: saltos EXTERNOS al widget (Ctrl+G, y luego buscar):
    // colocan el cursor por código, sin teclado, así que piden
    // su propio autoscroll (el de teclado corre solo con eventos).
    if state.needs_autoscroll {
        cursor_moved = true;
        state.needs_autoscroll = false;
    }
    let has_focus = ui.memory(|m| m.has_focus(id));

    // Después de cada edición de texto, si la CANTIDAD de líneas no
    // cambió, alcanza con recalcular la fila de la línea editada
    // (rápido). Si cambió (Enter, unir líneas con Backspace/Delete,
    // pegar texto con saltos de línea), reconstruimos toda la caché —
    // más caro, pero son las ediciones menos frecuentes.
    macro_rules! resync_rows {
        () => {
            if state.row_counts.len() != lines.line_count() {
                let (row_counts, char_lens) = rebuild_caches(lines, wrap_chars, state);
                state.row_counts = row_counts;
                state.char_lens = char_lens;
                state.cum_rows = build_cum_rows(&state.row_counts);
            } else {
                // La línea editada es cursor_line (todas las mutaciones
                // dejan el cursor sobre la línea que cambió): parcheamos
                // SOLO su entrada de las tres cachés, en O(n) solo si
                // cambió su cantidad de filas (ajuste del sumatorio).
                let old_rows = state.row_counts[state.cursor_line];
                let new_rows = row_count_for(&lines.get_line(state.cursor_line), wrap_chars);
                state.row_counts[state.cursor_line] = new_rows;
                state.char_lens[state.cursor_line] = lines.line_char_len(state.cursor_line);
                adjust_cum_rows(
                    &mut state.cum_rows,
                    state.cursor_line,
                    new_rows as isize - old_rows as isize,
                );
            }
        };
    }

    // --- teclado ---
    if has_focus {
        // S6: mientras el editor tiene foco, egui NO usa Tab ni las flechas para
        // cambiar de widget enfocado — así las flechas (arriba/abajo/izq/der) y Tab
        // pertenecen exclusivamente al editor de texto y jamás saltan el foco
        // a la barra de herramientas o al input de archivo.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        let cursor_before = (state.cursor_line, state.cursor_col);
        let has_events = ui.input(|i| !i.events.is_empty());
        if has_events {
            let events = ui.input(|i| i.events.clone());
            if events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::PointerButton { .. })) {
                state.hover_text = None;
            }
            for event in events {
            match event {
                egui::Event::Text(t) => {
                    cursor_moved = true;
                    state.preferred_col = None;
                    // F3 (Roadmap v0.2.29): el texto entra en el cursor
                    // primario y en todos los extra; si alguno tiene
                    // selección, primero se descarta en bloque.
                    // Roadmap v0.2.50: si `t` es un único delimitador, el
                    // cursor primario aplica auto-cierre / overtyping / wrap.
                    let single_char = {
                        let mut it = t.chars();
                        match (it.next(), it.next()) {
                            (Some(c), None) => Some(c),
                            _ => None,
                        }
                    };
                    let primary_outcome = match single_char {
                        Some(c) => handle_delimiter_char(lines, state, c),
                        None => DelimiterOutcome::Unhandled,
                    };

                    if matches!(primary_outcome, DelimiterOutcome::Unhandled) {
                        // Inserción normal multi-cursor.
                        let primary_range = selection_range(state);
                        let mut ranges = Vec::new();
                        if let Some(r) = primary_range {
                            ranges.push(r);
                        }
                        for c in &state.extra_carets {
                            if let Some(r) = c.range() {
                                ranges.push(r);
                            }
                        }
                        let mut positions =
                            vec![primary_range.map_or((state.cursor_line, state.cursor_col), |r| r.0)];
                        for c in &state.extra_carets {
                            positions.push(c.range().map_or((c.line, c.col), |r| r.0));
                        }
                        if !ranges.is_empty() {
                            delete_ranges_multi(lines, &ranges);
                        }
                        let news = insert_text_multi(lines, &positions, &t);
                        state.cursor_line = news[0].0;
                        state.cursor_col = news[0].1;
                        state.sel_anchor = Some(news[0]);
                        for (i, caret) in state.extra_carets.iter_mut().enumerate() {
                            if let Some(p) = news.get(i + 1) {
                                caret.line = p.0;
                                caret.col = p.1;
                                caret.anchor = Some(*p);
                            }
                        }
                        changed = true;
                        resync_rows!();
                    } else {
                        // El cursor primario ya aplicó el caso especial de
                        // delimitador; los cursores extra reciben `t` normal.
                        let text_mutated =
                            matches!(primary_outcome, DelimiterOutcome::TextMutated);
                        let mut ranges = Vec::new();
                        let mut positions = Vec::new();
                        for caret in &state.extra_carets {
                            if let Some(r) = caret.range() {
                                ranges.push(r);
                            }
                            positions.push(caret.range().map_or((caret.line, caret.col), |r| r.0));
                        }
                        if !ranges.is_empty() {
                            delete_ranges_multi(lines, &ranges);
                        }
                        let news = insert_text_multi(lines, &positions, &t);
                        for (caret, p) in state.extra_carets.iter_mut().zip(news) {
                            caret.line = p.0;
                            caret.col = p.1;
                            caret.anchor = Some(p);
                        }
                        if text_mutated || !state.extra_carets.is_empty() {
                            changed = true;
                        }
                        resync_rows!();
                    }
                }
                egui::Event::Paste(t) => {
                    cursor_moved = true;
                    state.preferred_col = None;
                    pre_break_snapshot = Some(lines.full_text());
                    if let Some((s, e)) = selection_range(state) {
                        delete_range(lines, s, e);
                        state.cursor_line = s.0;
                        state.cursor_col = s.1;
                    }
                    let (nl, nc) = insert_text(lines, (state.cursor_line, state.cursor_col), &normalize_pasted(&t));
                    state.cursor_line = nl;
                    state.cursor_col = nc;
                    state.sel_anchor = Some((nl, nc));
                    changed = true;
                    force_snapshot = true;
                    resync_rows!();
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Tab if !(modifiers.ctrl || modifiers.command || modifiers.alt) => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        let sel = selection_range(state);
                        let multiline = sel.and_then(|(s, e)| {
                            let last_l = if e.1 == 0 && e.0 > s.0 { e.0 - 1 } else { e.0 };
                            if last_l > s.0 {
                                Some((s.0, last_l, s, e))
                            } else {
                                None
                            }
                        });

                        if let Some((from_l, to_l, s, e)) = multiline {
                            if modifiers.shift {
                                // S4 (Etapa 2): Shift+Tab desindenta bloque multilínea
                                let mut removed_first = 0;
                                let mut removed_last = 0;
                                for l in from_l..=to_l.min(lines.line_count().saturating_sub(1)) {
                                    let mut line = lines.get_line(l);
                                    let rem = dedent_line(&mut line);
                                    lines.set_line(l, &line);
                                    if l == s.0 {
                                        removed_first = rem;
                                    }
                                    if l == e.0 {
                                        removed_last = rem;
                                    }
                                }
                                let new_s_col = s.1.saturating_sub(removed_first);
                                let new_e_col = e.1.saturating_sub(removed_last);
                                if state.cursor_line == s.0 {
                                    state.cursor_col = new_s_col;
                                    state.sel_anchor = Some((e.0, new_e_col));
                                } else {
                                    state.cursor_col = new_e_col;
                                    state.sel_anchor = Some((s.0, new_s_col));
                                }
                            } else {
                                // S4 (Etapa 2): Tab indenta bloque multilínea con 4 espacios
                                for l in from_l..=to_l.min(lines.line_count().saturating_sub(1)) {
                                    let mut line = lines.get_line(l);
                                    indent_line(&mut line);
                                    lines.set_line(l, &line);
                                }
                                let new_s_col = s.1 + 4;
                                let new_e_col = if e.1 > 0 || e.0 == to_l { e.1 + 4 } else { e.1 };
                                if state.cursor_line == s.0 {
                                    state.cursor_col = new_s_col;
                                    state.sel_anchor = Some((e.0, new_e_col));
                                } else {
                                    state.cursor_col = new_e_col;
                                    state.sel_anchor = Some((s.0, new_s_col));
                                }
                            }
                            changed = true;
                            force_snapshot = true;
                            let (row_counts, char_lens) = rebuild_caches(lines, wrap_chars, state);
                            state.row_counts = row_counts;
                            state.char_lens = char_lens;
                            state.cum_rows = build_cum_rows(&state.row_counts);
                        } else if modifiers.shift {
                            // S4 (Etapa 2): Shift+Tab en línea simple desindenta la línea actual
                            let mut line = lines.get_line(state.cursor_line);
                            let rem = dedent_line(&mut line);
                            if rem > 0 {
                                lines.set_line(state.cursor_line, &line);
                                state.cursor_col = state.cursor_col.saturating_sub(rem);
                                if let Some(anchor) = state.sel_anchor {
                                    if anchor.0 == state.cursor_line {
                                        state.sel_anchor = Some((anchor.0, anchor.1.saturating_sub(rem)));
                                    }
                                }
                                changed = true;
                                force_snapshot = true;
                                resync_rows!();
                            }
                        } else {
                            // S6: Tab en línea simple alinea a tab stops de 4 espacios (soft tab)
                            if let Some((s, e)) = sel {
                                delete_range(lines, s, e);
                                state.cursor_line = s.0;
                                state.cursor_col = s.1;
                            }
                            let spaces = DEFAULT_TAB_SIZE - (state.cursor_col % DEFAULT_TAB_SIZE);
                            let indent_str = " ".repeat(spaces);
                            let (nl, nc) =
                                insert_text(lines, (state.cursor_line, state.cursor_col), &indent_str);
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                            state.sel_anchor = Some((nl, nc));
                            changed = true;
                            force_snapshot = true;
                            resync_rows!();
                        }
                    }
                    egui::Key::Enter => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        pre_break_snapshot = Some(lines.full_text());
                        if let Some((s, e)) = selection_range(state) {
                            delete_range(lines, s, e);
                            state.cursor_line = s.0;
                            state.cursor_col = s.1;
                        }
                        // Roadmap v0.2.50: Enter entre `{` y `}` expande el
                        // bloque en 3 líneas con indentación +4 en el medio.
                        if enter_between_braces(lines, state) {
                            changed = true;
                            force_snapshot = true;
                            resync_rows!();
                        } else {
                            // S6: la línea nueva hereda la sangría de la
                            // línea en la que estaba el cursor. (se lee
                            // DESPUÉS de descartar la selección y ANTES de
                            // partir la línea en dos).
                            let indenta = inherited_indent(&lines.get_line(state.cursor_line));
                            let insert = if indenta.is_empty() {
                                "\n".to_string()
                            } else {
                                format!("\n{}", indenta)
                            };
                            let (nl, nc) =
                                insert_text(lines, (state.cursor_line, state.cursor_col), &insert);
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                            state.sel_anchor = Some((nl, nc));
                            changed = true;
                            force_snapshot = true;
                            resync_rows!();
                        }
                    }
                    egui::Key::Backspace => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        // F3: borra la selección, o el carácter anterior, en cada cursor.
                        let primary_range = selection_range(state);

                        // Roadmap v0.2.50: el cursor primario borra un par vacío
                        // si está justo entre `(`, `[`, `{`, comillas, etc. El
                        // helper reposiciona el cursor primario y deja los
                        // cursores extra para el flujo de backspace normal.
                        let primary_handled =
                            primary_range.is_none() && backspace_empty_pair(lines, state);

                        let mut ranges = Vec::new();
                        let mut single_positions = Vec::new();
                        if !primary_handled {
                            match primary_range {
                                Some(r) => ranges.push(r),
                                None => single_positions.push((state.cursor_line, state.cursor_col)),
                            }
                        }
                        for c in &state.extra_carets {
                            match c.range() {
                                Some(r) => ranges.push(r),
                                None => single_positions.push((c.line, c.col)),
                            }
                        }
                        for (l, c) in &single_positions {
                            if *c > 0 {
                                ranges.push(((*l, *c - 1), (*l, *c)));
                            } else if *l > 0 {
                                ranges.push(((*l - 1, lines.line_char_len(*l - 1)), (*l, 0)));
                            }
                        }

                        // Las posiciones nuevas se calculan con las líneas ya en
                        // su forma definitiva (el helper del par vacío, si actuó,
                        // ya borró su rango del primario).
                        let new_pos_after = |line: usize, col: usize, had_selection: bool| {
                            if had_selection {
                                // el inicio de la selección ya es la posición final
                                None
                            } else if col > 0 {
                                Some((line, col - 1))
                            } else if line > 0 {
                                Some((line - 1, lines.line_char_len(line - 1)))
                            } else {
                                Some((line, col))
                            }
                        };
                        let primary_new = if primary_handled {
                            // el helper ya dejó el cursor en el inicio del par borrado
                            (state.cursor_line, state.cursor_col)
                        } else {
                            match primary_range {
                                Some(r) => r.0,
                                None => new_pos_after(state.cursor_line, state.cursor_col, false)
                                    .unwrap(),
                            }
                        };
                        let extras_new: Vec<(usize, usize)> = state
                            .extra_carets
                            .iter()
                            .map(|c| match c.range() {
                                Some(r) => r.0,
                                None => new_pos_after(c.line, c.col, false).unwrap(),
                            })
                            .collect();

                        if !ranges.is_empty() {
                            delete_ranges_multi(lines, &ranges);
                        }
                        state.cursor_line = primary_new.0;
                        state.cursor_col = primary_new.1;
                        for (caret, pos) in state.extra_carets.iter_mut().zip(extras_new) {
                            caret.line = pos.0;
                            caret.col = pos.1;
                            caret.anchor = Some(pos);
                        }
                        dedup_extra_carets(state);
                        state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        changed = true;
                        resync_rows!();
                    }
                    egui::Key::Delete => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        // F3: borra la selección, o el carácter siguiente, en cada cursor.
                        let mut ranges = Vec::new();
                        let mut single_positions = Vec::new();
                        let primary_range = selection_range(state);
                        match primary_range {
                            Some(r) => ranges.push(r),
                            None => single_positions.push((state.cursor_line, state.cursor_col)),
                        }
                        for c in &state.extra_carets {
                            match c.range() {
                                Some(r) => ranges.push(r),
                                None => single_positions.push((c.line, c.col)),
                            }
                        }
                        for (l, c) in &single_positions {
                            if *c < lines.line_char_len(*l) {
                                ranges.push(((*l, *c), (*l, *c + 1)));
                            } else if *l + 1 < lines.line_count() {
                                ranges.push(((*l, *c), (*l + 1, 0)));
                            }
                        }

                        let new_pos_after = |line: usize, col: usize| (line, col);
                        let primary_new = match primary_range {
                            Some(r) => r.0,
                            None => new_pos_after(state.cursor_line, state.cursor_col),
                        };
                        let extras_new: Vec<(usize, usize)> = state
                            .extra_carets
                            .iter()
                            .map(|c| c.range().map_or(new_pos_after(c.line, c.col), |r| r.0))
                            .collect();

                        if !ranges.is_empty() {
                            delete_ranges_multi(lines, &ranges);
                        }
                        state.cursor_line = primary_new.0;
                        state.cursor_col = primary_new.1;
                        for (caret, pos) in state.extra_carets.iter_mut().zip(extras_new) {
                            caret.line = pos.0;
                            caret.col = pos.1;
                            caret.anchor = Some(pos);
                        }
                        dedup_extra_carets(state);
                        state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        changed = true;
                        resync_rows!();
                    }
                    egui::Key::ArrowLeft => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        if modifiers.ctrl || modifiers.command {
                            let (nl, nc) = prev_word_pos(lines, state.cursor_line, state.cursor_col);
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                        } else if state.cursor_col > 0 {
                            state.cursor_col -= 1;
                        } else if state.cursor_line > 0 {
                            state.cursor_line -= 1;
                            state.cursor_col = lines.line_char_len(state.cursor_line);
                        }
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::ArrowRight => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        if modifiers.ctrl || modifiers.command {
                            let (nl, nc) = next_word_pos(lines, state.cursor_line, state.cursor_col);
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                        } else if state.cursor_col < lines.line_char_len(state.cursor_line) {
                            state.cursor_col += 1;
                        } else if state.cursor_line + 1 < lines.line_count() {
                            state.cursor_line += 1;
                            state.cursor_col = 0;
                        }
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::PageUp => {
                        cursor_moved = true;
                        let page_rows = ((rect.height() / row_height).floor() as usize).max(1);
                        let total_rows = *state.cum_rows.last().unwrap_or(&1);
                        let cur_row = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
                        let target_row = page_target_row(cur_row, total_rows, page_rows, false);
                        let target_line = line_at_visual_row(&state.cum_rows, target_row);
                        let sub_row = target_row.saturating_sub(state.cum_rows[target_line]);
                        state.cursor_line = target_line;
                        let pref = *state.preferred_col.get_or_insert(state.cursor_col);
                        state.cursor_col = clamp_col(lines, target_line, sub_row * wrap_chars + (pref % wrap_chars.max(1)));
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::PageDown => {
                        cursor_moved = true;
                        let page_rows = ((rect.height() / row_height).floor() as usize).max(1);
                        let total_rows = *state.cum_rows.last().unwrap_or(&1);
                        let cur_row = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
                        let target_row = page_target_row(cur_row, total_rows, page_rows, true);
                        let target_line = line_at_visual_row(&state.cum_rows, target_row);
                        let sub_row = target_row.saturating_sub(state.cum_rows[target_line]);
                        state.cursor_line = target_line;
                        let pref = *state.preferred_col.get_or_insert(state.cursor_col);
                        state.cursor_col = clamp_col(lines, target_line, sub_row * wrap_chars + (pref % wrap_chars.max(1)));
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::ArrowUp => {
                        cursor_moved = true;
                        move_cursor_up(lines, state, modifiers.shift);
                    }
                    egui::Key::ArrowDown => {
                        cursor_moved = true;
                        move_cursor_down(lines, state, modifiers.shift);
                    }
                    egui::Key::Home => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        state.cursor_col = 0;
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::End => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        state.cursor_col = lines.line_char_len(state.cursor_line);
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::A if modifiers.command || modifiers.ctrl => {
                        cursor_moved = true;
                        state.preferred_col = None;
                        state.sel_anchor = Some((0, 0));
                        state.cursor_line = lines.line_count() - 1;
                        state.cursor_col = lines.line_char_len(state.cursor_line);
                    }
                    egui::Key::Escape if !state.extra_carets.is_empty() => {
                        // F3: Escape colapsa todos los cursores al primario.
                        state.extra_carets.clear();
                    }
                    egui::Key::D if (modifiers.command || modifiers.ctrl) && modifiers.shift => {
                        state.preferred_col = None;
                        // S7 (movido en F3): Duplicar línea o bloque seleccionado
                        let sel = selection_range(state);
                        if let Some((s, e)) = sel {
                            let from_l = s.0;
                            let to_l = if e.1 == 0 && e.0 > s.0 { e.0 - 1 } else { e.0 };
                            let cloned_lines: Vec<String> =
                                (from_l..=to_l).map(|l| lines.get_line(l)).collect();
                            let insert_idx = to_l + 1;
                            let num_cloned = cloned_lines.len();
                            for (offset, line) in cloned_lines.into_iter().enumerate() {
                                lines.insert_line(insert_idx + offset, line);
                            }
                            state.cursor_line += num_cloned;
                            state.sel_anchor = Some((s.0 + num_cloned, s.1));
                        } else {
                            let cur_line = lines.get_line(state.cursor_line);
                            lines.insert_line(state.cursor_line + 1, cur_line);
                            state.cursor_line += 1;
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                        changed = true;
                        force_snapshot = true;
                        let (row_counts, char_lens) = rebuild_caches(lines, wrap_chars, state);
                        state.row_counts = row_counts;
                        state.char_lens = char_lens;
                        state.cum_rows = build_cum_rows(&state.row_counts);
                    }
                    egui::Key::D if modifiers.command || modifiers.ctrl => {
                        state.preferred_col = None;
                        // F3 (Roadmap v0.2.29): Ctrl+D selecciona la palabra
                        // bajo el cursor; las pulsaciones siguientes agregan la
                        // próxima aparición del texto seleccionado como cursor
                        // extra (deduplicado por posición).
                        if let Some((s, e)) = selection_range(state) {
                            if s.0 == e.0 && e.1 > s.1 {
                                let needle: String =
                                    lines.get_line(s.0).chars().skip(s.1).take(e.1 - s.1).collect();
                                if let Some((ns, ne)) = find_next_occurrence(lines, &needle, e) {
                                    state.extra_carets.push(Caret {
                                        line: ne.0,
                                        col: ne.1,
                                        anchor: Some(ns),
                                    });
                                    dedup_extra_carets(state);
                                }
                            }
                        } else {
                            let (ws, we) = word_range_at(&lines.get_line(state.cursor_line), state.cursor_col);
                            if ws != we {
                                state.sel_anchor = Some((state.cursor_line, ws));
                                state.cursor_col = we;
                            }
                        }
                    }
                    egui::Key::Slash if modifiers.command || modifiers.ctrl => {
                        state.preferred_col = None;
                        // S6: Comentar / Descomentar
                        if let Some(prefix) = language.line_comment_prefix() {
                            let sel = selection_range(state);
                            let (start_l, end_l) = if let Some((s, e)) = sel {
                                (s.0, if e.1 == 0 && e.0 > s.0 { e.0 - 1 } else { e.0 })
                            } else {
                                (state.cursor_line, state.cursor_line)
                            };
                            toggle_comment_range(lines, start_l, end_l, prefix);
                            changed = true;
                            force_snapshot = true;
                            let (row_counts, char_lens) = rebuild_caches(lines, wrap_chars, state);
                            state.row_counts = row_counts;
                            state.char_lens = char_lens;
                            state.cum_rows = build_cum_rows(&state.row_counts);
                        }
                    }
                    // NOTA: Ctrl+C y Ctrl+X NO llegan como Key::C/Key::X
                    // (egui-winit los consume y los reenvia como
                    // Event::Copy / Event::Cut, ver los brazos de abajo).
                    _ => {}
                },
                // egui-winit consume Ctrl+C y Ctrl+X (modifiers.command:
                // tambien Cmd en macOS) y los reenvia aca SIN payload,
                // así que la decisión de qué copiar la toma este widget
                // con su propio estado: selección, o la línea bajo el
                // cursor (convención VS Code).
                egui::Event::Copy => {
                    let sel = selection_range(state);
                    ui.output_mut(|o| o.copied_text = copy_text(lines, sel, state.cursor_line));
                }
                // Cortar = copiar y borrar la selección; sin selección
                // corta la línea entera bajo el cursor.
                egui::Event::Cut => {
                    cursor_moved = true;
                    state.preferred_col = None;
                    let sel = selection_range(state);
                    let text = copy_text(lines, sel, state.cursor_line);
                    ui.output_mut(|o| o.copied_text = text);
                    if let Some((s, e)) = sel {
                        delete_range(lines, s, e);
                        state.cursor_line = s.0;
                        state.cursor_col = s.1;
                        state.sel_anchor = Some((s.0, s.1));
                        changed = true;
                        resync_rows!();
                    }
                }
                _ => {}
            }
        }
        if (state.cursor_line, state.cursor_col) != cursor_before {
            cursor_moved = true;
        }
        }
    }

    // OPT-3: si otro módulo movió `scroll` (cambio de pestaña, restore,
    // Ctrl+G), el target se alinea para no pelear con la animación.
    if (state.scroll - state.applied_scroll).abs() > 0.01 {
        state.target_scroll = state.scroll;
    }

    // --- autoscroll vertical: que el cursor nunca quede fuera de
    // vista, SOLO cuando se acaba de mover ---
    if cursor_moved {
        let cursor_row = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
        let cursor_y = cursor_row as f32 * row_height;
        if cursor_y < state.scroll {
            state.scroll = cursor_y;
        } else if cursor_y + row_height > state.scroll + rect.height() {
            state.scroll = cursor_y + row_height - rect.height();
        }
        // OPT-3: el cursor manda; el target acompaña sin animación.
        state.target_scroll = state.scroll;
    }

    // --- scroll y zoom con la rueda del mouse / trackpad ---
    // S10: Si hay gesto de zoom (egui-winit sintetiza Event::Zoom / i.zoom_delta())
    // o si el usuario hace scroll con la tecla Ctrl presionada, zoomea la fuente.
    if response.hovered() {
        let (scroll_delta, raw_delta, zoom_delta, ctrl) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.raw_scroll_delta,
                i.zoom_delta(),
                i.modifiers.ctrl,
            )
        });
        if (zoom_delta - 1.0).abs() > 0.005 {
            if zoom_delta > 1.005 {
                state.zoom_in();
            } else if zoom_delta < 0.995 {
                state.zoom_out();
            }
        } else if ctrl && (scroll_delta.y.abs() > 0.5 || raw_delta.y.abs() > 0.5) {
            let dy = if scroll_delta.y.abs() > 0.5 { scroll_delta.y } else { raw_delta.y };
            if dy > 0.5 {
                state.zoom_in();
            } else if dy < -0.5 {
                state.zoom_out();
            }
        } else if !ctrl {
            // OPT-3: la rueda mueve el destino; el render interpola.
            state.target_scroll -= scroll_delta.y;
        }
        // S10: el tamaño puede haber cambiado por zoom o desde afuera
        // (botones +/-): pedir re-layout/autoscroll cuando difiere del
        // frame anterior.
        if state.last_font_size != state.font_size {
            state.last_font_size = state.font_size;
            state.request_autoscroll();
        }
    }
    // OPT-3: interpolación suave hacia el destino, con asentamiento exacto.
    state.target_scroll = state.target_scroll.clamp(0.0, max_scroll);
    let (next_scroll, animating) = smooth_scroll_step(state.scroll, state.target_scroll);
    state.scroll = next_scroll.clamp(0.0, max_scroll);
    if animating {
        ui.ctx().request_repaint();
    }
    state.applied_scroll = state.scroll;

    // --- dibujar SOLO las filas visibles ---
    let clip_rect = if show_scrollbar {
        egui::Rect::from_min_max(rect.min, egui::pos2(scrollbar_track.left(), rect.max.y))
    } else {
        rect
    };
    let painter = ui.painter_at(clip_rect);
    let sel = selection_range(state);

    // S2 (Roadmap v0.2.9): timing del parpadeo del caret. La fase se
    // reinicia con cualquier entrada real del usuario (teclas, botones,
    // rueda, texto); el wake a ~2 Hz solo se pide cuando el editor TIENE
    // el foco (abajo, tras dibujar) — en reposo la ventana duerme.
    let now_time = ui.input(|i| i.time);
    let user_activity = ui.input(|i| {
        i.events.iter().any(|e| matches!(e,
            egui::Event::Text(_)
            | egui::Event::Key { pressed: true, .. }
            | egui::Event::PointerButton { .. }
            | egui::Event::MouseWheel { .. }))
    });
    if user_activity {
        state.last_activity_time = now_time;
    }
    let caret_on = caret_blink_visible(now_time, state.last_activity_time);

    // S4: banda de números de línea, pintada ANTES que el texto
    // (mismo layer: orden de llamadas = orden de pintura).
    let gutter_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + gutter_w, rect.max.y));
    let gutter_painter = ui.painter_at(gutter_rect);
    gutter_painter.rect_filled(gutter_rect, 0.0, theme.gutter_bg);

    // S3 (Etapa 2): matching de delimitadores () [] {} bajo el cursor
    let bracket_match = if has_focus && sel.is_none() {
        find_matching_bracket(lines, state.cursor_line, state.cursor_col)
    } else {
        None
    };

    // La primera línea visible sale O(log n) de la búsqueda binaria
    // sobre cum_rows: el loop de abajo solo recorre las filas que
    // entran en pantalla (más la primera, que puede verse partida),
    // no el documento entero como antes.
    let first_row = ((state.scroll / row_height).floor().max(0.0)) as usize;
    let start_line = line_at_visual_row(&state.cum_rows, first_row).min(lines.line_count().saturating_sub(1));
    // S1 (Roadmap v0.2.9): se alinea la estampa del cache con la revisión
    // del AST antes de recorrer las líneas visibles. Si `update_ast`
    // regeneró el árbol, acá se vacía la cache de una sola vez.
    state.layout_cache.sync_ast(state.ast_rev);
    let mut visual_row = state.cum_rows[start_line];
    // T4 (S3): x del caret capturada del galley real de su fila visual, y
    // layout de la última fila dibujada (para píldora de fold y blame).
    let mut caret_rel_x: Option<f32> = None;
    let mut last_row_layout: Option<(std::sync::Arc<egui::Galley>, usize, usize)>;
    for i in start_line..lines.line_count() {
        let line = lines.get_line(i);
        let rows_here = state.row_counts.get(i).copied().unwrap_or(1);
        if rows_here == 0 {
            continue;
        }
        last_row_layout = None;
        let line_top_y = rect.top() + PAD_Y + visual_row as f32 * row_height - state.scroll;

        if line_top_y > rect.bottom() {
            break;
        }

        // S1 (Etapa 2) & UI-5 (v0.2.23): Resaltado de la línea activa bajo el cursor
        if has_focus && i == state.cursor_line {
            let active_line_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left() + gutter_w, line_top_y),
                egui::pos2(clip_rect.right(), line_top_y + rows_here as f32 * row_height),
            );
            painter.rect_filled(active_line_rect, 0.0, theme.active_line);

            // UI-5: Indicador vertical de acento de 2 px en el gutter pegado a los números
            let gutter_indicator = egui::Rect::from_min_max(
                egui::pos2(gutter_rect.right() - 2.0, line_top_y),
                egui::pos2(gutter_rect.right(), line_top_y + rows_here as f32 * row_height),
            );
            gutter_painter.rect_filled(gutter_indicator, 0.0, theme.border_focus);
        }

        // F1 (Roadmap v0.2.27): marca de estado git en el borde izquierdo del
        // gutter — verde línea nueva, ámbar modificada, rojo borrado debajo.
        if let Some(diff) = state.git_diff.as_ref() {
            if let Some(status) = diff.status_of(i) {
                let mark_color = match status {
                    karkinos_core::git_diff::DiffStatus::Added => egui::Color32::from_rgb(80, 200, 120),
                    karkinos_core::git_diff::DiffStatus::Modified => {
                        egui::Color32::from_rgb(230, 180, 80)
                    }
                    karkinos_core::git_diff::DiffStatus::DeletedBelow => {
                        egui::Color32::from_rgb(220, 80, 80)
                    }
                };
                let mark_rect = egui::Rect::from_min_max(
                    egui::pos2(gutter_rect.left() + 1.0, line_top_y),
                    egui::pos2(gutter_rect.left() + 4.0, line_top_y + row_height),
                );
                gutter_painter.rect_filled(mark_rect, 1.0, mark_color);
            }
        }

        // S4: el número se alinea a la FILA VISUAL primera de la
        // línea lógica (una línea envuelta muestra su número una
        // sola vez) y right-align , para que los dígitos no bailen.
        gutter_painter.text(
            egui::pos2(gutter_rect.right() - GUTTER_PAD, line_top_y),
            egui::Align2::RIGHT_TOP,
            (i + 1).to_string(),
            font_id.clone(),
            theme.gutter_text,
        );

        // F4 (Roadmap v0.2.30): Indicador interactivo de plegado AST en el gutter
        if state.folding_ranges.iter().any(|r| r.start_line == i) {
            let is_folded = state.collapsed_folds.contains(&i);
            let chevron = if is_folded { "▸" } else { "▾" };
            let chevron_color = if is_folded { theme.syntax_type } else { theme.syntax_keyword };
            gutter_painter.text(
                egui::pos2(gutter_rect.left() + 7.0, line_top_y),
                egui::Align2::LEFT_TOP,
                chevron,
                egui::FontId::monospace(state.font_size * 0.8),
                chevron_color,
            );
        }

        // S3 (Etapa 4): Indicadores de diagnósticos LSP en el gutter
        if let Some(diag) = state.diagnostics.iter().find(|d| d.range.start.line == i) {
            let (marker, color) = match diag.severity {
                Some(karkinos_core::lsp::DiagnosticSeverity::Error) => {
                    ("●", egui::Color32::from_rgb(240, 70, 70))
                }
                Some(karkinos_core::lsp::DiagnosticSeverity::Warning) => {
                    ("▲", egui::Color32::from_rgb(240, 180, 50))
                }
                _ => ("ℹ", egui::Color32::from_rgb(100, 180, 240)),
            };
            gutter_painter.text(
                egui::pos2(gutter_rect.left() + 8.0, line_top_y),
                egui::Align2::LEFT_TOP,
                marker,
                egui::FontId::monospace(state.font_size * 0.75),
                color,
            );
        }

        // S4 (Rust IDE) & UI-5 (v0.2.23): Indicador geométrico de Breakpoint en el gutter
        let bp_center = egui::pos2(gutter_rect.left() + 7.0, line_top_y + row_height * 0.5);
        if state.breakpoints.contains(&i) {
            gutter_painter.circle_filled(bp_center, 4.5, egui::Color32::from_rgb(235, 60, 60));
            gutter_painter.circle_stroke(
                bp_center,
                4.5,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 120, 120)),
            );
        } else if let Some(pos) = ui.input(|inp| inp.pointer.hover_pos()) {
            if gutter_rect.contains(pos) && pos.y >= line_top_y && pos.y < line_top_y + rows_here as f32 * row_height {
                gutter_painter.circle_filled(
                    bp_center,
                    4.0,
                    egui::Color32::from_rgba_unmultiplied(235, 60, 60, 80),
                );
            }
        }

        // S4 (Rust IDE) & UI-5 (v0.2.23): Indicador de línea detenida en debugger (execution pointer)
        if state.stopped_line == Some(i) {
            let arrow_center = egui::pos2(gutter_rect.left() + 8.0, line_top_y + row_height * 0.5);
            let h = 4.5;
            let points = [
                arrow_center + egui::vec2(-h, -h),
                arrow_center + egui::vec2(h, 0.0),
                arrow_center + egui::vec2(-h, h),
            ];
            gutter_painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                egui::Color32::from_rgb(255, 215, 0),
                egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
            ));
        }

        let char_len = state
            .char_lens
            .get(i)
            .copied()
            .unwrap_or_else(|| line_char_count(&line));
        let is_pure_ascii = line.len() == char_len;

        // S6 (Etapa 2) + S3 (Etapa 3), S1 (Roadmap v0.2.9): el tokenizado
        // ahora es PEREZOSO — se corre solo si algún fragmento de esta
        // línea NO está cacheado (o sea: solo la 1ª vez que la línea se
        // ve, o cuando cambió). Antes cada frame re-tokenizaba e
        // invalidaba `highlight_spans()` del árbol entero por línea.
        let mut line_tokens: Option<Vec<(std::ops::Range<usize>, crate::syntax::TokenType)>> = None;

        // S2 (Etapa 2): Coincidencias de búsqueda en la línea i
        let line_matches = if let Some(ref s) = search {
            let start_idx = s.matches.partition_point(|&(line, _)| line < i);
            let end_idx = s.matches.partition_point(|&(line, _)| line <= i);
            Some((start_idx, &s.matches[start_idx..end_idx]))
        } else {
            None
        };

        for sub in 0..rows_here {
            let y = line_top_y + sub as f32 * row_height;
            if y + row_height < rect.top() || y > rect.bottom() {
                continue;
            }
            let start = sub * wrap_chars;
            let end = (start + wrap_chars).min(char_len);
            let sb = if is_pure_ascii { start } else { char_to_byte(&line, start) };
            let eb = if is_pure_ascii { end } else { char_to_byte(&line, end) };
            let chunk = &line[sb..eb];
            // T4 (S3): columna visual donde arranca este fragmento. Los
            // tabs se expanden contra esta base, no contra el inicio del
            // chunk (una línea puede partirse en medio de una parada).
            let vstart = visual_col_at(&line, start, DEFAULT_TAB_SIZE);
            let vend = visual_col_at(&line, end, DEFAULT_TAB_SIZE);

            // S1 (Roadmap v0.2.9) + T4 (S3): el galley sale del cache si
            // este fragmento ya se tipografió (mismo texto/lenguaje/tema/
            // fuente/fase de tab). Se obtiene ANTES de pintar los fondos
            // porque caret, selección y overlays se miden con él: egui
            // redondea cada glifo a píxeles físicos, así que la aritmética
            // de ancho fijo acumulaba desfase.
            let galley = if chunk.is_empty() {
                None
            } else {
                let tab_phase = if chunk.contains('\t') {
                    vstart % DEFAULT_TAB_SIZE
                } else {
                    0
                };
                let key = if tab_phase == 0 {
                    crate::syntax::line_layout_key(chunk, language, state.font_size)
                } else {
                    crate::syntax::line_layout_key_with_tab_phase(
                        chunk,
                        language,
                        state.font_size,
                        tab_phase,
                    )
                };
                Some(state.layout_cache.get_or_insert(key, theme.mode, || {
                    let tokens = line_tokens.get_or_insert_with(|| {
                        if language == crate::syntax::Language::PlainText {
                            Vec::new()
                        } else {
                            crate::syntax::tokenize_line_semantic(
                                &line,
                                i,
                                language,
                                Some(state.ast_spans.as_slice()),
                            )
                        }
                    });
                    if language == crate::syntax::Language::PlainText {
                        ui.fonts(|f| {
                            f.layout_no_wrap(
                                crate::syntax::expand_tabs(chunk, vstart, DEFAULT_TAB_SIZE),
                                font_id.clone(),
                                text_color,
                            )
                        })
                    } else {
                        let job = crate::syntax::build_layout_job(
                            chunk,
                            sb,
                            tokens,
                            theme,
                            font_id.clone(),
                            vstart,
                        );
                        ui.fonts(|f| f.layout_job(job))
                    }
                }))
            };
            // Captura la x real del cursor primario en su fila visual. Si
            // la línea está fuera de pantalla queda `None` y el bloque del
            // caret usa la aproximación nominal (no se dibuja igual).
            let cursor_sub = (state.cursor_col / wrap_chars.max(1)).min(rows_here - 1);
            if i == state.cursor_line && sub == cursor_sub {
                caret_rel_x = Some(match galley.as_ref() {
                    Some(g) => x_for_col(
                        g,
                        visual_col_at(&line, state.cursor_col, DEFAULT_TAB_SIZE)
                            .saturating_sub(vstart),
                    ),
                    None => 0.0,
                });
            }
            if let Some(ref g) = galley {
                last_row_layout = Some((g.clone(), vstart, vend));
            }

            // S4 (Rust IDE): Resaltado de fondo para la línea activa en depuración
            if state.stopped_line == Some(i) {
                let stopped_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), y),
                    egui::pos2(rect.right(), y + row_height),
                );
                painter.rect_filled(
                    stopped_rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(255, 215, 0, 35),
                );
            }

            // S2 (Etapa 2): Resaltado de todas las coincidencias de búsqueda visibles
            if let (Some((global_start, matches_in_line)), Some(ref s)) = (line_matches, &search) {
                for (local_idx, &(_ml, mc)) in matches_in_line.iter().enumerate() {
                    let match_end = mc + s.query_len;
                    let ov_start = mc.max(start);
                    let ov_end = match_end.min(end);
                    if ov_start < ov_end {
                        let vs0 = visual_col_at(&line, ov_start, DEFAULT_TAB_SIZE)
                            .saturating_sub(vstart);
                        let vs1 = visual_col_at(&line, ov_end, DEFAULT_TAB_SIZE)
                            .saturating_sub(vstart);
                        let x0 = text_left
                            + galley.as_ref().map_or(0.0, |g| x_for_col(g, vs0));
                        let x1 = text_left
                            + galley.as_ref().map_or(0.0, |g| x_for_col(g, vs1));
                        let match_rect = egui::Rect::from_min_max(
                            egui::pos2(x0, y),
                            egui::pos2(x1, y + row_height),
                        );
                        let is_current = (global_start + local_idx) == s.current_idx;
                        let match_color = if is_current {
                            egui::Color32::from_rgba_unmultiplied(255, 140, 0, 180)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(235, 200, 50, 90)
                        };
                        painter.rect_filled(match_rect, 2.0, match_color);
                    }
                }
            }

            if let Some((s, e)) = sel {
                if i >= s.0 && i <= e.0 {
                    let line_start_col = if i == s.0 { s.1 } else { 0 };
                    let line_end_col = if i == e.0 { e.1 } else { char_len };
                    let row_start = line_start_col.max(start);
                    let row_end = line_end_col.min(end);
                    if row_start < row_end || (line_start_col == line_end_col && start == 0 && sub == 0) {
                        let vs_start = visual_col_at(&line, row_start, DEFAULT_TAB_SIZE)
                            .saturating_sub(vstart);
                        let vs_end = visual_col_at(&line, row_end, DEFAULT_TAB_SIZE)
                            .saturating_sub(vstart);
                        let (rx0, rx1) = match galley.as_ref() {
                            Some(g) => {
                                let x0 = x_for_col(g, vs_start);
                                (x0, x_for_col(g, vs_end).max(x0 + 1.0))
                            }
                            None => (0.0, 1.0),
                        };
                        let sel_rect = egui::Rect::from_min_max(
                            egui::pos2(text_left + rx0, y),
                            egui::pos2(text_left + rx1, y + row_height),
                        );
                        painter.rect_filled(sel_rect, 0.0, sel_color.gamma_multiply(0.5));
                    }
                }
            }

            // F3 (Roadmap v0.2.29): selección y cursor de los cursores extra
            // que caen en esta fila visual. Se pintan antes del texto para
            // no taparlo y su x sale del galley, igual que el primario.
            for caret in &state.extra_carets {
                if caret.line != i {
                    continue;
                }
                if let Some((start_pos, end_pos)) = caret.range() {
                    if start_pos.0 == i && end_pos.0 == i {
                        let row_start = start_pos.1.max(start);
                        let row_end = end_pos.1.min(end);
                        if row_start < row_end {
                            let vs = visual_col_at(&line, row_start, DEFAULT_TAB_SIZE)
                                .saturating_sub(vstart);
                            let ve = visual_col_at(&line, row_end, DEFAULT_TAB_SIZE)
                                .saturating_sub(vstart);
                            let sx = text_left
                                + galley.as_ref().map_or(0.0, |g| x_for_col(g, vs));
                            let ex = text_left
                                + galley.as_ref().map_or(0.0, |g| x_for_col(g, ve));
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(sx, y),
                                    egui::pos2(ex.max(sx + 1.0), y + row_height),
                                ),
                                0.0,
                                theme.selection,
                            );
                        }
                    }
                }
                if caret.col / wrap_chars.max(1) == sub {
                    let cv = visual_col_at(&line, caret.col, DEFAULT_TAB_SIZE)
                        .saturating_sub(vstart);
                    let cx = text_left
                        + galley.as_ref().map_or(0.0, |g| x_for_col(g, cv));
                    painter.line_segment(
                        [egui::pos2(cx, y), egui::pos2(cx, y + row_height)],
                        egui::Stroke::new(1.5_f32, theme.caret),
                    );
                }
            }

            if let Some(ref galley) = galley {
                painter.galley(egui::pos2(text_left, y), galley.clone(), text_color);
            }

            // S2 (Rust IDE): Render Inlay Hints (types, parameter names)
            for hint in &state.inlay_hints {
                if hint.position.line == i
                    && hint.position.character >= start
                    && hint.position.character <= end
                {
                    let hint_vcol = visual_col_at(&line, hint.position.character, DEFAULT_TAB_SIZE)
                        .saturating_sub(vstart);
                    let hx = text_left
                        + galley.as_ref().map_or(0.0, |g| x_for_col(g, hint_vcol));
                    let hint_font_size = state.font_size * 0.78;
                    let hint_color = egui::Color32::from_rgb(130, 155, 195);
                    let hint_bg = egui::Color32::from_rgba_unmultiplied(35, 45, 65, 140);

                    // OPT-2: mismo galley reutilizado hasta que cambie la
                    // etiqueta o el tamaño de fuente (dos lecturas, cero
                    // layout por frame para hints repetidos como ": i32").
                    let hint_key = (hint.label.clone(), (hint_font_size * 100.0) as u32);
                    let hint_galley = match state.hint_galleys.get(&hint_key).cloned() {
                        Some(galley) => galley,
                        None => {
                            let galley = ui.fonts(|f| {
                                f.layout_no_wrap(
                                    hint.label.clone(),
                                    egui::FontId::monospace(hint_font_size),
                                    hint_color,
                                )
                            });
                            // Cota defensiva: si el LSP inventara etiquetas
                            // nuevas toda la sesión, el caché no crece sin fin.
                            if state.hint_galleys.len() >= 512 {
                                state.hint_galleys.clear();
                            }
                            state.hint_galleys.insert(hint_key, galley.clone());
                            galley
                        }
                    };
                    let hint_size = hint_galley.size();
                    let hint_rect = egui::Rect::from_min_size(
                        egui::pos2(hx, y + 2.0),
                        egui::vec2(hint_size.x + 4.0, row_height - 4.0),
                    );

                    painter.rect_filled(hint_rect, 2.0, hint_bg);
                    painter.galley(egui::pos2(hx + 2.0, y + 2.0), hint_galley, hint_color);
                }
            }

            // S3 (Etapa 2): Resaltado de delimitadores coincidentes
            if let Some(bm) = bracket_match {
                let check_pos = |pos: (usize, usize)| -> bool {
                    pos.0 == i && pos.1 >= start && pos.1 < end
                };
                let is_origin = check_pos(bm.origin);
                let is_partner = bm.partner.is_some_and(check_pos);

                if is_origin || is_partner {
                    let col = if is_origin { bm.origin.1 } else { bm.partner.unwrap().1 };
                    let col_visual =
                        visual_col_at(&line, col, DEFAULT_TAB_SIZE).saturating_sub(vstart);
                    let (bx0, bw) = match galley.as_ref() {
                        Some(g) => {
                            let x0 = x_for_col(g, col_visual);
                            (x0, (x_for_col(g, col_visual + 1) - x0).max(1.0))
                        }
                        None => (0.0, char_width),
                    };
                    let b_rect = egui::Rect::from_min_size(
                        egui::pos2(text_left + bx0, y),
                        egui::vec2(bw, row_height),
                    );
                    if bm.partner.is_some() {
                        painter.rect_filled(
                            b_rect,
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(100, 180, 255, 60),
                        );
                        painter.rect_stroke(
                            b_rect,
                            2.0,
                            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(120, 200, 255)),
                        );
                    } else {
                        painter.rect_filled(
                            b_rect,
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(255, 80, 80, 50),
                        );
                        painter.rect_stroke(
                            b_rect,
                            2.0,
                            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 90, 90)),
                        );
                    }
                }
            }

            // S11: El cursor primario ahora se dibuja con animación fluida smear al final del pase de líneas.
        }

        // T4 (S3): fin visual de la línea según el último galley dibujado
        // (la píldora de fold y el blame se anclan ahí, no a `char_width`).
        let line_end_x = match last_row_layout.as_ref() {
            Some((g, vs, ve)) => x_for_col(g, ve.saturating_sub(*vs)),
            None => visual_col_at(&line, char_len, DEFAULT_TAB_SIZE) as f32 * char_width,
        };

        // F4 (Roadmap v0.2.30): si la línea está plegada, dibujamos una píldora "..." compacta
        if state.collapsed_folds.contains(&i) {
            let badge_x = text_left + line_end_x + 8.0;
            let badge_rect = egui::Rect::from_min_size(
                egui::pos2(badge_x, line_top_y + 2.0),
                egui::vec2(24.0, row_height - 4.0),
            );
            painter.rect_filled(
                badge_rect,
                3.0,
                egui::Color32::from_rgba_unmultiplied(80, 100, 140, 100),
            );
            painter.text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                "...",
                egui::FontId::monospace(state.font_size * 0.75),
                egui::Color32::from_rgb(180, 200, 230),
            );
        }

        // F8 (Roadmap v0.2.35): ghost text de blame al final de la línea activa.
        if i == state.cursor_line {
            if let Some(blame) = state.blame_text.as_deref() {
                let gx = text_left + line_end_x + 16.0;
                painter.text(
                    egui::pos2(gx.min(clip_rect.right() - 4.0), line_top_y),
                    egui::Align2::LEFT_TOP,
                    blame,
                    egui::FontId::monospace(state.font_size * 0.85),
                    egui::Color32::from_gray(100),
                );
            }
        }

        visual_row += rows_here;
    }

    // S11: Renderizado y animación fluida del cursor smear
    if has_focus {
        let wrap_c = wrap_chars.max(1);
        let cursor_line = state.cursor_line.min(lines.line_count().saturating_sub(1));
        let line_str = lines.get_line(cursor_line);
        let rows_here = state.row_counts.get(cursor_line).copied().unwrap_or(1).max(1);
        let sub = (state.cursor_col / wrap_c).min(rows_here - 1);
        let start = sub * wrap_c;
        let vcol = visual_col_at(&line_str, state.cursor_col, DEFAULT_TAB_SIZE);
        let vstart = visual_col_at(&line_str, start, DEFAULT_TAB_SIZE);
        // T4 (S3): la x real sale del galley de la fila del cursor. El
        // fallback nominal solo aplica si la línea quedó fuera del
        // viewport (en ese caso el caret ni se dibuja).
        let caret_rel = caret_rel_x
            .unwrap_or_else(|| vcol.saturating_sub(vstart) as f32 * char_width);
        let caret_doc_x = gutter_w + PAD_X + caret_rel;

        let visual_row = visual_row_of(&state.cum_rows, cursor_line, state.cursor_col, wrap_chars);
        let caret_doc_y = PAD_Y + visual_row as f32 * row_height;
        let target = egui::pos2(caret_doc_x, caret_doc_y);

        let has_selection = selection_range(state).is_some() || response.dragged();

        let (next_head, next_tail, is_animating_smear) = if state.smear_enabled && !has_selection {
            let cursor_pos_key = (state.cursor_line, state.cursor_col);
            let just_moved = state.smear_last_target != Some(cursor_pos_key);
            if just_moved {
                state.smear_last_target = Some(cursor_pos_key);
            }

            let (head, tail) = match (state.smear_head, state.smear_tail) {
                (Some(h), Some(t)) => {
                    let dist = (target - h).length();
                    if dist > SMEAR_MAX_DISTANCE {
                        // Salto muy grande (cambio de archivo, salto de página):
                        // Acotamos el tail para que no cruce toda la pantalla.
                        let dir = (h - target).normalized();
                        (target, target + dir * (SMEAR_MAX_DISTANCE * 0.5))
                    } else {
                        (h, t)
                    }
                }
                _ => (target, target),
            };

            let (nh, anim_h) = smear_step(head, target, SMEAR_HEAD_EASING);
            let (nt, anim_t) = smear_step(tail, nh, SMEAR_TAIL_EASING);
            state.smear_head = Some(nh);
            state.smear_tail = Some(nt);
            (nh, nt, anim_h || anim_t)
        } else {
            state.smear_head = Some(target);
            state.smear_tail = Some(target);
            state.smear_last_target = Some((state.cursor_line, state.cursor_col));
            (target, target, false)
        };

        if is_animating_smear {
            ui.ctx().request_repaint();
        }

        let screen_head = egui::pos2(rect.left() + next_head.x, rect.top() + next_head.y - state.scroll);
        let screen_tail = egui::pos2(rect.left() + next_tail.x, rect.top() + next_tail.y - state.scroll);
        let caret_w = 2.0_f32;
        let head_rect = egui::Rect::from_min_size(screen_head, egui::vec2(caret_w, row_height));
        let tail_rect = egui::Rect::from_min_size(screen_tail, egui::vec2(caret_w, row_height));

        if head_rect.intersects(clip_rect) || tail_rect.intersects(clip_rect) {
            let dir = screen_head.to_vec2() - screen_tail.to_vec2();
            let dist_sq = dir.length_sq();

            if state.smear_enabled && !has_selection && dist_sq > 1.0 {
                let pts = vec![
                    tail_rect.left_top(),
                    tail_rect.right_top(),
                    tail_rect.right_bottom(),
                    tail_rect.left_bottom(),
                    head_rect.left_top(),
                    head_rect.right_top(),
                    head_rect.right_bottom(),
                    head_rect.left_bottom(),
                ];
                let hull = convex_hull(pts);
                if hull.len() >= 3 {
                    let mut mesh = egui::Mesh::default();
                    let tail_center = tail_rect.center();
                    let head_center = head_rect.center();
                    let d = head_center.to_vec2() - tail_center.to_vec2();
                    let d_sq = d.length_sq().max(1.0);

                    for p in &hull {
                        let t = ((p.to_vec2() - tail_center.to_vec2()).dot(d) / d_sq).clamp(0.0, 1.0);
                        let alpha = (t.powf(1.4) * 0.55 + 0.12).clamp(0.0, 0.75);
                        let color = theme.caret.gamma_multiply(alpha);
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: *p,
                            uv: egui::epaint::WHITE_UV,
                            color,
                        });
                    }

                    for i in 1..(hull.len().saturating_sub(1)) {
                        mesh.indices.push(0);
                        mesh.indices.push(i as u32);
                        mesh.indices.push((i + 1) as u32);
                    }

                    painter.add(egui::Shape::Mesh(mesh));
                }
            }

            // Dibuja el cursor principal sólido: si está animando siempre visible, si está quieto respeta blink
            if is_animating_smear || caret_on {
                painter.rect_filled(head_rect, 1.0_f32, theme.caret);
            }
        }
    }

    // F2 (Roadmap v0.2.28): sticky scroll. El contenedor (impl, fn, struct…)
    // que abarca la primera línea visible queda pegado arriba, hasta 3
    // niveles, y un clic en su banda salta al inicio del bloque.
    if !state.sticky_scopes.is_empty() {
        let active: Vec<(usize, String)> = state
            .sticky_scopes
            .iter()
            .filter(|s| s.start_line < start_line && s.end_line >= start_line)
            .take(3)
            .map(|s| (s.start_line, s.header_text.trim().to_string()))
            .collect();

        let band_h = 20.0;
        for (level, (scope_line, header)) in active.iter().enumerate() {
            let top = rect.top() + level as f32 * band_h;
            let band = egui::Rect::from_min_max(
                egui::pos2(rect.left() + gutter_w, top),
                egui::pos2(clip_rect.right(), top + band_h),
            );
            painter.rect_filled(band, 0.0, theme.bg.gamma_multiply(0.95));
            let indent = "  ".repeat(level);
            painter.text(
                egui::pos2(band.left() + PAD_X, band.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{}{}", indent, header),
                egui::FontId::monospace(state.font_size * 0.85),
                theme.text_muted,
            );
            let response = ui.interact(
                band,
                ui.id().with(("sticky_scope", *scope_line)),
                egui::Sense::click(),
            );
            if response.clicked() {
                state.cursor_line = (*scope_line).min(lines.line_count().saturating_sub(1));
                state.cursor_col = 0;
                state.preferred_col = None;
                state.sel_anchor = None;
                let row = state.cum_rows.get(*scope_line).copied().unwrap_or(0);
                state.scroll = row as f32 * row_height;
                state.target_scroll = state.scroll;
                state.applied_scroll = state.scroll;
            }
        }
    }

    // F9 (Roadmap v0.2.36) / UI-7 (v0.2.38): minimap optativo. Una barra por
    // línea proporcional a su largo, borde divisorio, viewport y arrastre.
    if state.show_minimap {
        let mm_painter = ui.painter();
        mm_painter.rect_filled(minimap_rect, 0.0, theme.bg);
        let total = lines.line_count().max(1);
        let mm_height = minimap_rect.height();
        let mm_height_usize = (mm_height as usize).max(1);

        if total <= mm_height_usize {
            // El documento entra entero: una barra por línea en su posición
            // proporcional, sin forzar un paso mínimo.
            let step = mm_height / total as f32;
            for i in 0..lines.line_count() {
                let y = minimap_rect.top() + i as f32 * step;
                let len = state
                    .char_lens
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| lines.line_char_len(i));
                if len == 0 {
                    continue;
                }
                let width = ((len as f32 / 120.0).min(1.0)) * (MINIMAP_W - 8.0);
                mm_painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(minimap_rect.left() + 4.0, y),
                        egui::vec2(width.max(1.0), step),
                    ),
                    0.0,
                    theme.text_muted.gamma_multiply(0.35),
                );
            }
        } else {
            // Documento más alto que el minimap: muestrear una línea por píxel
            // vertical, mapeando el píxel a la línea proporcional. Así se
            // cubre el 0%..100% del archivo sin truncar las líneas inferiores.
            let mapping = minimap_line_mapping(lines.line_count(), mm_height_usize);
            for (py, &line_idx) in mapping.iter().enumerate() {
                let len = state
                    .char_lens
                    .get(line_idx)
                    .copied()
                    .unwrap_or_else(|| lines.line_char_len(line_idx));
                if len == 0 {
                    continue;
                }
                let width = ((len as f32 / 120.0).min(1.0)) * (MINIMAP_W - 8.0);
                mm_painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(minimap_rect.left() + 4.0, minimap_rect.top() + py as f32),
                        egui::vec2(width.max(1.0), 1.0),
                    ),
                    0.0,
                    theme.text_muted.gamma_multiply(0.35),
                );
            }
        }

        let visible_frac = rect.height() / total_height.max(1.0);
        let scroll_frac = if max_scroll > 0.0 {
            state.scroll / max_scroll
        } else {
            0.0
        };
        let (viewport_top, viewport_h) =
            minimap_viewport(scroll_frac, visible_frac, minimap_rect.height());
        mm_painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(minimap_rect.left(), minimap_rect.top() + viewport_top),
                egui::vec2(MINIMAP_W, viewport_h),
            ),
            2.0,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 20),
        );

        mm_painter.line_segment(
            [
                egui::pos2(minimap_rect.left(), minimap_rect.top()),
                egui::pos2(minimap_rect.left(), minimap_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme.border_subtle),
        );

        let mm_response = ui
            .interact(
                minimap_rect,
                id.with("minimap"),
                egui::Sense::click_and_drag(),
            )
            .on_hover_text("Minimap (drag to scroll)");
        if let Some(pos) = mm_response.interact_pointer_pos() {
            if mm_response.clicked() || mm_response.dragged() {
                let frac = ((pos.y - minimap_rect.top()) / minimap_rect.height().max(1.0))
                    .clamp(0.0, 1.0);
                state.scroll = frac * max_scroll;
                state.target_scroll = state.scroll;
                state.applied_scroll = state.scroll;
            }
        }
    }

    // S2 (Roadmap v0.2.9): wake del parpadeo — request_repaint_after
    // pide el PRÓXIMO cruce de fase (nunca un loop continuo). Sin foco
    // no hay caret ni blink: no se pinta ni se despierta.
    if has_focus {
        let wake_s = caret_blink_next_wake(now_time, state.last_activity_time).max(0.01);
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(wake_s));
    }

    // --- barra de scroll (encima de todo, painter SIN recortar) ---
    if show_scrollbar {
        let thumb_h = (rect.height() / total_height * rect.height()).max(24.0);
        let scroll_frac = state.scroll / max_scroll;
        let thumb_top = rect.top() + scroll_frac * (rect.height() - thumb_h);
        let thumb = egui::Rect::from_min_size(
            egui::pos2(scrollbar_track.left() + 2.0, thumb_top),
            egui::vec2(SCROLLBAR_W - 4.0, thumb_h),
        );
        let scrollbar_painter = ui.painter();
        scrollbar_painter.rect_filled(scrollbar_track, 0.0, egui::Color32::from_white_alpha(10));
        scrollbar_painter.rect_filled(thumb, 4.0, egui::Color32::from_white_alpha(90));
    }

    // S4 (Etapa 4): Popup flotante de autocompletado LSP
    if state.show_completions && !state.completions.is_empty() {
        let cursor_vrow = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
        let cur_y = rect.top() + PAD_Y + cursor_vrow as f32 * row_height - state.scroll + row_height;
        let cur_x = text_left
            + caret_rel_x.unwrap_or((state.cursor_col % wrap_chars.max(1)) as f32 * char_width);

        let popup_w = 260.0_f32;
        let item_h = 22.0_f32;
        let visible_items = state.completions.len().min(8);
        let popup_h = visible_items as f32 * item_h + 8.0;

        let popup_rect = egui::Rect::from_min_size(
            egui::pos2(
                cur_x.min(rect.right() - popup_w - 10.0).max(rect.left()),
                cur_y.min(rect.bottom() - popup_h - 10.0).max(rect.top()),
            ),
            egui::vec2(popup_w, popup_h),
        );

        let popup_painter = ui.painter();
        popup_painter.rect_filled(popup_rect, 4.0, egui::Color32::from_rgb(32, 34, 40));
        popup_painter.rect_stroke(popup_rect, 4.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(70, 75, 90)));

        for (idx, item) in state.completions.iter().take(visible_items).enumerate() {
            let item_y = popup_rect.top() + 4.0 + idx as f32 * item_h;
            let item_rect = egui::Rect::from_min_size(
                egui::pos2(popup_rect.left() + 4.0, item_y),
                egui::vec2(popup_w - 8.0, item_h),
            );

            let is_sel = idx == state.selected_completion;
            if is_sel {
                popup_painter.rect_filled(item_rect, 2.0, egui::Color32::from_rgb(50, 70, 110));
            }

            popup_painter.text(
                egui::pos2(item_rect.left() + 6.0, item_y + 3.0),
                egui::Align2::LEFT_TOP,
                &item.label,
                egui::FontId::monospace(state.font_size * 0.9),
                if is_sel { egui::Color32::WHITE } else { theme.text },
            );

            if let Some(ref detail) = item.detail {
                popup_painter.text(
                    egui::pos2(item_rect.right() - 6.0, item_y + 3.0),
                    egui::Align2::RIGHT_TOP,
                    detail,
                    egui::FontId::monospace(state.font_size * 0.75),
                    theme.gutter_text,
                );
            }
        }
    }

    // S2 (Rust IDE): Popup flotante de documentación / hover LSP
    if !state.show_completions {
        if let Some(ref text) = state.hover_text {
            let cursor_vrow = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
            let cur_y = rect.top() + PAD_Y + cursor_vrow as f32 * row_height - state.scroll + row_height;
            let cur_x = text_left
            + caret_rel_x.unwrap_or((state.cursor_col % wrap_chars.max(1)) as f32 * char_width);

            let sample_lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(6).collect();
            let display_text = sample_lines.join("\n");
            let line_count = sample_lines.len().max(1);

            let popup_w = 360.0_f32;
            let popup_h = line_count as f32 * 18.0 + 16.0;

            let popup_rect = egui::Rect::from_min_size(
                egui::pos2(
                    cur_x.min(rect.right() - popup_w - 10.0).max(rect.left()),
                    cur_y.min(rect.bottom() - popup_h - 10.0).max(rect.top()),
                ),
                egui::vec2(popup_w, popup_h),
            );

            let popup_painter = ui.painter();
            popup_painter.rect_filled(popup_rect, 4.0, egui::Color32::from_rgb(26, 28, 34));
            popup_painter.rect_stroke(popup_rect, 4.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(70, 85, 115)));

            popup_painter.text(
                egui::pos2(popup_rect.left() + 8.0, popup_rect.top() + 8.0),
                egui::Align2::LEFT_TOP,
                &display_text,
                egui::FontId::monospace(state.font_size * 0.85),
                egui::Color32::from_rgb(220, 230, 245),
            );
        }
    }

    EditorResponse {
        changed,
        lost_focus,
        pre_break_snapshot,
        force_snapshot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T4 (S3): el caret se posiciona con el galley real que se dibuja.
    // La fórmula vieja (`col * glyph_width(' ')`) acumulaba el redondeo
    // a píxeles que egui aplica glifo a glifo: con Geist Mono a 12 px el
    // desfase llegaba a ~20 px en una línea de 54 columnas. Este test
    // deja constancia de esa medición y exige que el mapeo del render
    // (`x_for_col`) coincida con el galley en todos los tamaños.
    #[test]
    fn caret_x_coincide_con_el_galley() {
        let ctx = egui::Context::default();
        crate::fonts::install_fonts(&ctx);
        let line = "let mut resultado = calcular(a, b) + 42; // comentario";
        let cols = line.chars().count();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for size in [12.0_f32, 14.0, 16.0, 20.0] {
                let font_id = egui::FontId::monospace(size);
                let galley = ctx.fonts(|f| {
                    f.layout_no_wrap(line.to_owned(), font_id.clone(), egui::Color32::WHITE)
                });
                let char_width = ctx.fonts(|f| f.glyph_width(&font_id, ' '));
                let mut max_diff_fija = 0.0_f32;
                let mut max_diff_real = 0.0_f32;
                for col in 0..=cols {
                    let x_fija = col as f32 * char_width;
                    let x_galley = galley
                        .pos_from_ccursor(egui::text::CCursor::new(col))
                        .min
                        .x;
                    max_diff_fija = max_diff_fija.max((x_fija - x_galley).abs());
                    max_diff_real = max_diff_real.max((x_for_col(&galley, col) - x_galley).abs());
                }
                println!(
                    "T4: size={size} char_width={char_width:.4} desfase_formula_vieja={max_diff_fija:.3}px desfase_real={max_diff_real:.3}px"
                );
                assert!(
                    max_diff_real < 0.5,
                    "size {size}: el caret se desvía {max_diff_real:.3}px del galley"
                );
            }
        });
    }

    // T4 (S3): el clic también se resuelve con el galley. Un clic dentro
    // del 40% izquierdo del glifo de la columna `n` deja el cursor en `n`
    // (el redondeo de frontera más cercana no se adelanta una columna).
    #[test]
    fn clic_en_columna_n_deja_el_cursor_en_n() {
        let ctx = egui::Context::default();
        crate::fonts::install_fonts(&ctx);
        let line = "let mut resultado = calcular(a, b) + 42; // comentario";
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let font_id = egui::FontId::monospace(14.0);
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(line.to_owned(), font_id, egui::Color32::WHITE)
            });
            for col in [0usize, 1, 10, 40] {
                let x0 = x_for_col(&galley, col);
                let x1 = x_for_col(&galley, col + 1);
                let x_izq40 = x0 + (x1 - x0) * 0.4;
                assert_eq!(
                    col_for_x(&galley, x_izq40),
                    col,
                    "col {col}: un clic al 40% del glifo cayó en otra columna"
                );
            }
        });
    }

    // T4 (S3): un tab inicial se expande a espacios antes de tipografiar,
    // así que el caret de la columna lógica 1 (columna visual 4) cae
    // exactamente sobre el glifo 'l', no sobre el tab de reemplazo.
    #[test]
    fn caret_alineado_con_tab_inicial() {
        let ctx = egui::Context::default();
        crate::fonts::install_fonts(&ctx);
        let line = "\tlet x = 1;";
        let expanded = crate::syntax::expand_tabs(line, 0, DEFAULT_TAB_SIZE);
        assert_eq!(expanded, "    let x = 1;");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let font_id = egui::FontId::monospace(14.0);
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(expanded.clone(), font_id, egui::Color32::WHITE)
            });
            let caret_x = x_for_col(&galley, visual_col_at(line, 1, DEFAULT_TAB_SIZE));
            let glyph_l_x = galley.rows[0].glyphs[4].pos.x;
            assert!(
                (caret_x - glyph_l_x).abs() < 0.01,
                "el caret del primer carácter tras el tab ({caret_x}) no coincide con el glifo 'l' ({glyph_l_x})"
            );
        });
    }

    // T4: la expansión de tabs respeta la parada visual de arranque: un
    // fragmento que empieza en la columna 2 expande el tab con 2 espacios.
    #[test]
    fn expand_tabs_respeta_la_fase_de_arranque() {
        assert_eq!(crate::syntax::expand_tabs("\tx", 0, 4), "    x");
        assert_eq!(crate::syntax::expand_tabs("\tx", 1, 4), "   x");
        assert_eq!(crate::syntax::expand_tabs("a\tb", 0, 4), "a   b");
    }

    // S2 (Roadmap v0.2.9): fase del parpadeo del caret — funciones puras.

    #[test]
    fn caret_blink_solida_mientras_se_tipea() {
        // recién tecleado (0.4s de inactividad) sigue sólido
        assert!(caret_blink_visible(0.0, 0.0));
        assert!(caret_blink_visible(0.35, 0.0));
        assert!(caret_blink_visible(0.49, 0.0));
    }

    #[test]
    fn caret_blink_se_apaga_y_reaparece() {
        // ventana [0.5, 1.0): apagado; [1.0, 1.5): encendido; alterna
        assert!(!caret_blink_visible(0.75, 0.0));
        assert!(caret_blink_visible(1.05, 0.0));
        assert!(!caret_blink_visible(1.75, 0.0));
        assert!(caret_blink_visible(2.05, 0.0));
    }

    #[test]
    fn caret_blink_proximo_wake_en_el_cruce_de_fase() {
        // a 0.2s de la última actividad faltan 0.3s para apagarse
        assert!((caret_blink_next_wake(0.2, 0.0) - 0.3).abs() < 1e-6);
        // a medio apagado faltan 0.25s para reencender
        assert!((caret_blink_next_wake(0.75, 0.0) - 0.25).abs() < 1e-6);
        // justo en el cruce: pide un período completo
        assert!((caret_blink_next_wake(1.0, 0.0) - 0.5).abs() < 1e-6);
    }

    // egui-winit consume Ctrl+C/X y las re-envia como Event::Copy/Cut
    // (sin payload): el widget decide que copiar con su estado propio.
    // copy_text centraliza esa decision: seleccion o, sin ella, la
    // linea logica entera bajo el cursor.
    #[test]
    fn copy_text_con_seleccion() {
        let lines = vec!["uno".to_string(), "dos".to_string(), "tres".to_string()];
        // Selección cruzando dos líneas: el salto de línea forma parte
        // del texto copiado (igual que en cualquier editor estándar).
        assert_eq!(copy_text(&lines, Some(((0, 1), (1, 2))), 0), "no\ndo");
        assert_eq!(copy_text(&lines, Some(((2, 0), (2, 4))), 0), "tres");
    }

    #[test]
    fn copy_text_sin_seleccion_copia_linea_del_cursor() {
        let lines = vec!["uno".to_string(), "dos tres".to_string()];
        assert_eq!(copy_text(&lines, None, 1), "dos tres");
        assert_eq!(copy_text(&lines, None, 0), "uno");
    }

    #[test]
    fn normalize_pasted_convierte_crlf() {
        // El portapapeles de Windows/WSL llega con \r\n; sin
        // normalizar quedan caracteres \r invisibles dentro de las
        // líneas (y el conteo de columnas deja de coincidir).
        assert_eq!(normalize_pasted("a\r\nb"), "a\nb");
        assert_eq!(normalize_pasted("x\r\ny\r\nz"), "x\ny\nz");
        assert_eq!(normalize_pasted("sin cambios"), "sin cambios");
        // Un \r suelto no se toca (no es un salto de línea en Windows).
        assert_eq!(normalize_pasted("a\rb"), "a\rb");
    }

    #[test]
    fn pegar_crlf_spawea_dos_lineas_limpias() {
        let mut lines = vec!["xx".to_string()];
        let (nl, nc) = insert_text(&mut lines, (0, 0), &normalize_pasted("a\r\nb"));
        assert_eq!(lines, vec!["a", "bxx"]);
        assert_eq!((nl, nc), (1, 1)); // cursor al final del texto pegado
    }

    #[test]
    fn vivir_insert_text_con_multilinea() {
        // Comportamiento previo que NO pidió el cambio: pegar texto
        // multi-línea en medio de una línea parte bien.
        let mut lines = vec!["uno dos".to_string()];
        let (nl, nc) = insert_text(&mut lines, (0, 3), "A\nB\n");
        assert_eq!(lines, vec!["unoA", "B", " dos"]);
        assert_eq!((nl, nc), (2, 0));
    }

    // S6: Enter hereda la indentación (incluye espacios y TABS
    // mezclados) de la línea anterior, convención de editores tipo
    // VS Code; una línea sin sangría arranca en columna 0.
    #[test]
    fn inherited_indent_copia_whitespace_inicial() {
        assert_eq!(inherited_indent("    hola"), "    ");
        assert_eq!(inherited_indent("\t\tfn main()"), "\t\t");
        assert_eq!(inherited_indent("sin sangria"), "");
        assert_eq!(inherited_indent("   \t  mix"), "   \t  ");
        assert_eq!(inherited_indent(""), "");
        // Solo el whitespace INICIAL cuenta: el de la cola es contenido.
        assert_eq!(inherited_indent("hola   "), "");
    }

    // S6: Tab debe_INSERTAR el caracter tab cuando el editor tiene
    // foco (y NO cambiar el foco de widget). La inserción va a
    // través de insert_text, así que este pin cubre el esfuerzo
    // ancho: 1 char por tab, cursor avanzado.
    #[test]
    fn tab_inserta_caracter_tab() {
        let mut lines = vec!["x".to_string()];
        let (nl, nc) = insert_text(&mut lines, (0, 1), "\t");
        assert_eq!((nl, nc), (0, 2));
        assert_eq!(lines[0], "x\t");
        let mut lines = vec!["".to_string()];
        let (nl, nc) = insert_text(&mut lines, (0, 0), "\t");
        assert_eq!((nl, nc), (0, 1));
        assert_eq!(lines[0], "\t");
    }

    // S4: el ancho de la banda de números se calcula con los dígitos
    // del total de líneas (VS Code style: crece cuando crece el doc,
    // con padding fijo para no pegarlo al texto).
    #[test]
    fn gutter_width_escala_con_digitos() {
        let cw = 10.0;
        let pad = 8.0;
        assert_eq!(gutter_width_for(9, cw), cw + pad); // 1 dígito
        assert_eq!(gutter_width_for(99, cw), 2.0 * cw + pad);
        assert_eq!(gutter_width_for(1234, cw), 4.0 * cw + pad);
        // Documento vacío (0) cuenta como 1 línea: siempre se muestra.
        assert_eq!(gutter_width_for(0, cw), cw + pad);
    }

    // S5: salto por palabra (Ctrl+←/→). Palabra = runa alfanumérica
    // unicode (á, ñ, dígitos, chacras todo); los separadores se
    // saltean, y al terminar la línea se sigue en el principio de
    // la siguiente (al final del doc: clavado).
    #[test]
    fn next_word_pos_saltea_separadores_y_come_la_palabra() {
        let lines = vec!["hola =x mundo".to_string(), "segunda línea".to_string()];
        assert_eq!(next_word_pos(&lines, 0, 1), (0, 4)); // come "ola"
        assert_eq!(next_word_pos(&lines, 0, 4), (0, 6)); // ' ' -> 'x'
        assert_eq!(next_word_pos(&lines, 0, 6), (0, 7)); // come "x"
        assert_eq!(next_word_pos(&lines, 0, 0), (0, 4)); // ya en palabra: come todo
        // Fin de línea -> principio de la siguiente.
        assert_eq!(next_word_pos(&lines, 0, 13), (1, 0));
        // Fin de documento: clavado (no hay a donde ir).
        let fin = line_char_count(&lines[1]);
        assert_eq!(next_word_pos(&lines, 1, fin), (1, fin));
    }

    #[test]
    fn prev_word_pos_es_el_espejo() {
        let lines = vec!["hola =x mundo".to_string(), "segunda".to_string()];
        assert_eq!(prev_word_pos(&lines, 0, 10), (0, 7)); // come "mundo"
        assert_eq!(prev_word_pos(&lines, 0, 6), (0, 5)); // ' ' -> fin "hola"
        assert_eq!(prev_word_pos(&lines, 0, 3), (0, 0)); // come "hol"
        // Principio de línea -> final de la anterior.
        assert_eq!(prev_word_pos(&lines, 1, 0), (0, 13));
        // Principio del documento: clavado.
        assert_eq!(prev_word_pos(&lines, 0, 0), (0, 0));
    }

    #[test]
    fn prev_word_pos_saltea_unicode_raro() {
        let lines = vec!["áé-í".to_string()];
        assert_eq!(prev_word_pos(&lines, 0, 4), (0, 2)); // come "áé"
        assert_eq!(prev_word_pos(&lines, 0, 1), (0, 0)); // come "á"
    }

    // S5: doble click = palabra bajo el cursor, rango [start, end)
    // exclusivo al final (la convención de select_range del widget).
    #[test]
    fn word_range_at_selecta_alfanumerico_unicode() {
        assert_eq!(word_range_at("hola mundo", 0), (0, 4)); // "hola"
        assert_eq!(word_range_at("hola mundo", 2), (0, 4)); // mitad de palabra
        assert_eq!(word_range_at("hola mundo", 3), (0, 4));
        assert_eq!(word_range_at("señor", 2), (0, 5)); // ñ es alfanumérica
        // Sobre un separador: el carácter en sí.
        assert_eq!(word_range_at("hola mundo", 4), (4, 5));
        // Fuera de rango (fin de línea): la ÚLTIMA runa.
        assert_eq!(word_range_at("hola", 4), (0, 4));
    }

    // S5: destino de fila visual al paginar, acotado a [0, total-1].
    #[test]
    fn page_target_row_acotea_en_los_dos_bordes() {
        assert_eq!(page_target_row(3, 10, 4, true), 7);
        assert_eq!(page_target_row(8, 10, 4, true), 9); // no pasa del final
        assert_eq!(page_target_row(3, 10, 4, false), 0); // arriba
        assert_eq!(page_target_row(0, 10, 4, false), 0); // ya arriba: clavado
        // Documento de una fila: no pasa nada.
        assert_eq!(page_target_row(0, 1, 50, true), 0);
    }

    #[test]
    fn build_cum_rows_suma_prefix() {
        let cum = build_cum_rows(&[2, 1, 3]);
        assert_eq!(cum, vec![0, 2, 3, 6]);
    }

    #[test]
    fn build_cum_rows_vacio() {
        assert_eq!(build_cum_rows(&[]), vec![0]);
    }

    // T3: copia exacta de la implementación previa de `rebuild_row_counts`,
    // que materializaba cada línea con `get_line`. Se usa como referencia
    // de equivalencia (y para medir el antes en el perf test).
    fn rebuild_row_counts_antes(
        lines: &dyn LineBuffer,
        wrap_chars: usize,
        state: &EditorState,
    ) -> Vec<usize> {
        (0..lines.line_count())
            .map(|i| {
                if state.is_line_folded(i) {
                    0
                } else {
                    row_count_for(&lines.get_line(i), wrap_chars)
                }
            })
            .collect()
    }

    fn comprobar_caches_igual_que_antes(lines: &dyn LineBuffer, state: &EditorState) {
        let char_lens_antes: Vec<usize> = (0..lines.line_count())
            .map(|i| lines.get_line(i).chars().count())
            .collect();
        for wrap in [1usize, 7, 40, 120, 500] {
            let rows_antes = rebuild_row_counts_antes(lines, wrap, state);
            assert_eq!(
                rebuild_row_counts(lines, wrap, state),
                rows_antes,
                "row_counts con wrap {wrap}"
            );
            let (rows, lens) = rebuild_caches(lines, wrap, state);
            assert_eq!(rows, rows_antes, "rebuild_caches con wrap {wrap}");
            assert_eq!(lens, char_lens_antes, "char_lens con wrap {wrap}");
        }
        assert_eq!(lines.rebuild_char_lens(), char_lens_antes);
    }

    #[test]
    fn line_char_len_coincide_con_get_line() {
        fn comprobar(lines: &dyn LineBuffer) {
            for i in 0..lines.line_count() {
                assert_eq!(
                    lines.line_char_len(i),
                    lines.get_line(i).chars().count(),
                    "línea {i}"
                );
            }
        }

        // Vec<String>: líneas vacías, tildes/emoji y última línea sin `\n`.
        let vec_lines = vec![
            String::new(),
            "hola niño".to_string(),
            "🦀🚀✨".to_string(),
            String::new(),
            "final sin salto".to_string(),
        ];
        comprobar(&vec_lines);

        // Buffer vacío: una única línea lógica vacía.
        comprobar(&TextBuffer::new());

        // TextBuffer con CRLF, líneas vacías y última línea sin salto.
        comprobar(&TextBuffer::from_text("primera\r\n\r\nniño 🦀\r\nfinal"));
    }

    #[test]
    fn rebuild_row_counts_igual_que_antes() {
        use karkinos_core::syntax_ast::{FoldingKind, FoldingRange};

        let vec_lines = vec![
            String::new(),
            "corta".to_string(),
            "\tcon tab al inicio".to_string(),
            "x".repeat(120),
            format!("\t{}", "y".repeat(95)),
            "última línea con tildes ñ 🦀".to_string(),
        ];
        let rope_lines = TextBuffer::from_text(&format!(
            "corta\n\tcon tab al inicio\n{}\n\t{}\nfinal\n",
            "x".repeat(120),
            "y".repeat(95)
        ));

        let mut state = EditorState::default();
        comprobar_caches_igual_que_antes(&vec_lines, &state);
        comprobar_caches_igual_que_antes(&rope_lines, &state);

        // Con un bloque plegado, las líneas ocultas pesan 0 igual que antes.
        state.folding_ranges = vec![FoldingRange {
            start_line: 1,
            end_line: 3,
            kind: FoldingKind::Block,
        }];
        state.collapsed_folds.push(1);
        comprobar_caches_igual_que_antes(&vec_lines, &state);
        comprobar_caches_igual_que_antes(&rope_lines, &state);
    }

    #[test]
    #[ignore = "perf: correr a mano con cargo test --release -- --ignored"]
    fn rebuild_caches_es_barato_con_documento_grande() {
        use std::time::Instant;

        // 200 000 líneas sin tabs: el camino rápido no debe asignar por línea.
        let text = "let mut suma = contador + 1;\n".repeat(200_000);
        let lines = TextBuffer::from_text(&text);
        let state = EditorState::default();

        let antes_inicio = Instant::now();
        let rows_antes = rebuild_row_counts_antes(&lines, 80, &state);
        let lens_antes: Vec<usize> = (0..lines.line_count())
            .map(|i| lines.get_line(i).chars().count())
            .collect();
        let antes = antes_inicio.elapsed();

        // Los call sites que reconstruyen las dos cachés (Enter, pegar,
        // borrar líneas) usan una sola pasada sobre el Rope.
        let despues_inicio = Instant::now();
        let (rows, lens) = rebuild_caches(&lines, 80, &state);
        let despues = despues_inicio.elapsed();

        assert_eq!(rows, rows_antes);
        assert_eq!(lens, lens_antes);
        eprintln!("rebuild de cachés, 200 000 líneas: antes={antes:?} después={despues:?}");
        assert!(
            despues.as_millis() < 30,
            "rebuild de cachés tardó {despues:?} (> 30 ms)"
        );
    }

    #[test]
    fn adjust_cum_rows_desplaza_solo_tail() {
        let mut cum = vec![0, 2, 3, 6];
        // La línea 1 pasa de 1 fila a 2 (+1)
        adjust_cum_rows(&mut cum, 1, 1);
        assert_eq!(cum, vec![0, 2, 4, 7]);
        // Y hacia abajo (unión de líneas, -2)
        adjust_cum_rows(&mut cum, 0, -2);
        assert_eq!(cum, vec![0, 0, 2, 5]);
    }

    #[test]
    fn adjust_cum_rows_sin_delta_no_hace_nada() {
        let mut cum = vec![0, 2, 3];
        adjust_cum_rows(&mut cum, 0, 0);
        assert_eq!(cum, vec![0, 2, 3]);
    }

    #[test]
    fn line_at_visual_row_busca_binaria() {
        let cum = build_cum_rows(&[1, 3, 2]);
        // filas: línea0 en fila 0; línea1 en filas 1..4; línea2 en 4..6
        assert_eq!(line_at_visual_row(&cum, 0), 0);
        assert_eq!(line_at_visual_row(&cum, 1), 1);
        assert_eq!(line_at_visual_row(&cum, 3), 1);
        assert_eq!(line_at_visual_row(&cum, 4), 2);
        assert_eq!(line_at_visual_row(&cum, 5), 2);
        // Más allá del final: cae en la última línea (el llamador
        // acota la columna).
        assert_eq!(line_at_visual_row(&cum, 999), 2);
    }

    #[test]
    fn line_and_subrow_at_doc_y_con_covacia() {
        let cum = build_cum_rows(&[1, 2, 1]);
        // Líneas de 2 filas cada una para simplificar
        let (line, sub) = line_and_subrow_at_doc_y(&cum, 2.5 * 10.0, 10.0);
        assert_eq!(line, 1); // fila 2 => cae en línea 1
        assert_eq!(sub, 1);
    }

    #[test]
    fn visual_row_of_usa_prefix() {
        let cum = build_cum_rows(&[1, 3, 2]);
        assert_eq!(visual_row_of(&cum, 0, 0, 80), 0);
        assert_eq!(visual_row_of(&cum, 1, 0, 80), 1);
        assert_eq!(visual_row_of(&cum, 2, 10, 80), 4);
        // col avanza en sub-filas enteras dentro de su línea
        assert_eq!(visual_row_of(&cum, 1, 160, 80), 3);
    }

    #[test]
    fn char_to_byte_y_line_char_count_ascii_y_unicode() {
        // ASCII puro: byte == char
        let ascii = "hello world 123";
        assert_eq!(line_char_count(ascii), 15);
        for c in 0..=15 {
            assert_eq!(char_to_byte(ascii, c), c);
        }
        // Clampeo a len en overflow
        assert_eq!(char_to_byte(ascii, 999), 15);

        // Unicode multi-byte (acentos, ñ, emoji)
        let unicode = "hola niños 🦀!";
        // "hola " (chars 0..5, bytes 0..5)
        // "ni" (chars 5..7, bytes 5..7)
        // 'ñ' (char 7, byte 7..9)
        // "os " (chars 8..11, bytes 9..12)
        // '🦀' (char 11, bytes 12..16)
        // '!' (char 12, byte 16..17)
        assert_eq!(line_char_count(unicode), 13);
        assert_eq!(char_to_byte(unicode, 0), 0);
        assert_eq!(char_to_byte(unicode, 5), 5); // 'n'
        assert_eq!(char_to_byte(unicode, 7), 7); // 'ñ'
        assert_eq!(char_to_byte(unicode, 8), 9); // 'o' después de 'ñ'
        assert_eq!(char_to_byte(unicode, 11), 12); // '🦀'
        assert_eq!(char_to_byte(unicode, 12), 16); // '!' después de emoji 4 bytes
        assert_eq!(char_to_byte(unicode, 13), 17); // fin
        assert_eq!(char_to_byte(unicode, 100), unicode.len());

        // Equivalencia estricta con el cálculo ingenuo sobre char_indices()
        let test_cases = ["", "abc", "áéíóú", "🦀🚀✨", "mixed niño 123 ñ"];
        for text in test_cases {
            let naive_count = text.chars().count();
            assert_eq!(line_char_count(text), naive_count);
            for c in 0..=(naive_count + 5) {
                let naive_byte = text.char_indices().nth(c).map(|(b, _)| b).unwrap_or(text.len());
                assert_eq!(char_to_byte(text, c), naive_byte);
            }
        }
    }

    #[test]
    fn find_matching_bracket_mismo_renglon_multilinea_y_desbalance() {
        let lines = vec![
            "fn main() {".to_string(),
            "    let a = [1, (2 + 3)];".to_string(),
            "}".to_string(),
        ];

        // 1. Paréntesis () en misma línea
        // Línea 1: "    let a = [1, (2 + 3)];"
        // '[' está en col 12, '(' en col 16, ')' en col 22, ']' en col 23
        let match_paren = find_matching_bracket(&lines, 1, 16).expect("debería encontrar '('");
        assert_eq!(match_paren.origin, (1, 16));
        assert_eq!(match_paren.partner, Some((1, 22)));

        // Cursor a la derecha de '(': col 17
        let match_paren_right = find_matching_bracket(&lines, 1, 17).expect("debería encontrar '('");
        assert_eq!(match_paren_right.origin, (1, 16));
        assert_eq!(match_paren_right.partner, Some((1, 22)));

        // Cursor sobre ')': col 22 -> backward search
        let match_paren_close = find_matching_bracket(&lines, 1, 22).expect("debería encontrar ')'");
        assert_eq!(match_paren_close.origin, (1, 22));
        assert_eq!(match_paren_close.partner, Some((1, 16)));

        // 2. Corchetes [] anidados en misma línea
        let match_bracket = find_matching_bracket(&lines, 1, 12).expect("debería encontrar '['");
        assert_eq!(match_bracket.origin, (1, 12));
        assert_eq!(match_bracket.partner, Some((1, 23)));

        // 3. Llaves {} multilínea
        // Línea 0, col 10: '{'
        let match_brace = find_matching_bracket(&lines, 0, 10).expect("debería encontrar '{'");
        assert_eq!(match_brace.origin, (0, 10));
        assert_eq!(match_brace.partner, Some((2, 0))); // '}' en línea 2, col 0

        // 4. Delimitador desbalanceado (sin pareja)
        let unclosed = vec!["let x = (a + b;".to_string()];
        let match_unclosed = find_matching_bracket(&unclosed, 0, 8).expect("debería detectar '('");
        assert_eq!(match_unclosed.origin, (0, 8));
        assert_eq!(match_unclosed.partner, None); // Sin pareja

        // 5. Cursor en caracter cualquiera
        assert_eq!(find_matching_bracket(&lines, 0, 1), None);
    }

    #[test]
    fn indent_y_dedent_line_tab_y_espacios() {
        let mut line_empty = String::new();
        assert_eq!(dedent_line(&mut line_empty), 0);
        assert_eq!(line_empty, "");

        let mut line_tab = "\thola".to_string();
        assert_eq!(dedent_line(&mut line_tab), 1);
        assert_eq!(line_tab, "hola");

        let mut line_4_spaces = "    mundo".to_string();
        assert_eq!(dedent_line(&mut line_4_spaces), 4);
        assert_eq!(line_4_spaces, "mundo");

        let mut line_2_spaces = "  dos".to_string();
        assert_eq!(dedent_line(&mut line_2_spaces), 2);
        assert_eq!(line_2_spaces, "dos");

        let mut line_6_spaces = "      seis".to_string();
        assert_eq!(dedent_line(&mut line_6_spaces), 4);
        assert_eq!(line_6_spaces, "  seis");

        let mut line_clean = "codigo".to_string();
        indent_line(&mut line_clean);
        assert_eq!(line_clean, "    codigo");
        assert_eq!(dedent_line(&mut line_clean), 4);
        assert_eq!(line_clean, "codigo");
    }

    #[test]
    fn test_toggle_comment_range_lineas_y_bloques() {
        let mut lines = vec![
            "fn main() {".to_string(),
            "    let x = 10;".to_string(),
            "    println!(\"{}\", x);".to_string(),
            "}".to_string(),
        ];

        // Comentar líneas 1..=2 con "//"
        toggle_comment_range(&mut lines, 1, 2, "//");
        assert_eq!(lines[1], "    // let x = 10;");
        assert_eq!(lines[2], "    // println!(\"{}\", x);");

        // Descomentar líneas 1..=2 con "//"
        toggle_comment_range(&mut lines, 1, 2, "//");
        assert_eq!(lines[1], "    let x = 10;");
        assert_eq!(lines[2], "    println!(\"{}\", x);");

        // Comentar línea SQL con "--"
        let mut sql_lines = vec!["SELECT * FROM users;".to_string()];
        toggle_comment_range(&mut sql_lines, 0, 0, "--");
        assert_eq!(sql_lines[0], "-- SELECT * FROM users;");
        toggle_comment_range(&mut sql_lines, 0, 0, "--");
        assert_eq!(sql_lines[0], "SELECT * FROM users;");
    }

    // OPT-3 (v0.2.26): interpolación exponencial del scroll.

    #[test]
    fn smooth_scroll_converges_and_settles_exactly() {
        let target = 320.0;
        let mut scroll = 0.0;
        let mut steps = 0;
        loop {
            let (next, animating) = smooth_scroll_step(scroll, target);
            scroll = next;
            steps += 1;
            if !animating {
                break;
            }
            assert!(steps < 200, "la animación tiene que converger");
        }
        assert_eq!(scroll, target, "asienta exacto en el destino");
        assert!(steps > 1, "no salta en un solo paso");
    }

    #[test]
    fn smooth_scroll_never_overshoots_the_target() {
        let target = 120.0;
        let mut scroll = 0.0;
        for _ in 0..200 {
            let (next, animating) = smooth_scroll_step(scroll, target);
            assert!(next <= target, "no pasa el destino: {next} > {target}");
            scroll = next;
            if !animating {
                break;
            }
        }
        assert_eq!(scroll, target);

        // Y también en sentido contrario (scroll hacia arriba).
        let mut scroll = target;
        for _ in 0..200 {
            let (next, animating) = smooth_scroll_step(scroll, 0.0);
            assert!(next >= 0.0, "no sube por encima del destino: {next}");
            scroll = next;
            if !animating {
                break;
            }
        }
        assert_eq!(scroll, 0.0);
    }

    #[test]
    fn smooth_scroll_small_difference_settles_immediately() {
        let (scroll, animating) = smooth_scroll_step(10.0, 10.4);
        assert_eq!(scroll, 10.4, "diferencias sub-pixel se asientan solas");
        assert!(!animating, "no se pide repaint por menos de medio pixel");
    }

    // F9 (Roadmap v0.2.36): minimap.

    #[test]
    fn minimap_esta_apagado_por_defecto() {
        let state = EditorState::default();
        assert!(
            !state.show_minimap,
            "sin minimap el editor ocupa todo el ancho y no queda ninguna franja"
        );
    }

    #[test]
    fn minimap_viewport_es_proporcional_y_no_desborda() {
        let (top0, h0) = minimap_viewport(0.0, 0.25, 200.0);
        assert_eq!(top0, 0.0);
        assert_eq!(h0, 50.0);

        let (top1, h1) = minimap_viewport(1.0, 0.25, 200.0);
        assert_eq!(top1, 150.0);
        assert!(top1 + h1 <= 200.0 + f32::EPSILON);

        // Documento que entra entero: viewport del alto total y sin desplazamiento.
        let (top2, h2) = minimap_viewport(0.0, 1.5, 120.0);
        assert_eq!(h2, 120.0);
        assert_eq!(top2, 0.0);
    }

    #[test]
    fn minimap_mapea_cada_linea_si_entra_entero() {
        // Documento corto: una entrada por línea, en orden.
        let mapping = minimap_line_mapping(10, 600);
        assert_eq!(mapping, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn minimap_documento_largo_no_trunca_las_lineas_inferiores() {
        // 2,000 líneas en un minimap de 600 px: antes el `step.max(1.0)` +
        // `break` cortaba en la línea 599 y ocultaba el resto.
        let total = 2000usize;
        let height = 600usize;
        let mapping = minimap_line_mapping(total, height);

        assert_eq!(mapping.len(), height, "una muestra por píxel vertical");
        assert_eq!(mapping[0], 0, "cubre la primera línea");

        // Monótono no decreciente: el mapeo respeta el orden del archivo.
        for w in mapping.windows(2) {
            assert!(w[0] <= w[1], "el mapeo no puede retroceder");
        }

        let last = mapping[height - 1];
        assert!(
            last > height,
            "el fondo debe mapear más allá del viejo techo de {height} líneas (fue {last})"
        );
        // El hueco hasta el final es como máximo un paso de muestreo: no trunca.
        assert!(
            total - last <= total / height + 1,
            "las líneas inferiores no deben quedar fuera (última muestreada: {last} de {total})"
        );
    }

    #[test]
    fn minimap_documento_vacio_no_produce_entradas() {
        assert!(minimap_line_mapping(0, 600).is_empty());
    }

    // F3 (Roadmap v0.2.29): multi-cursor.

    #[test]
    fn caret_range_normalizes_both_directions() {
        let forward = Caret { line: 1, col: 5, anchor: Some((1, 2)) };
        assert_eq!(forward.range(), Some(((1, 2), (1, 5))));
        let backward = Caret { line: 1, col: 2, anchor: Some((1, 5)) };
        assert_eq!(backward.range(), Some(((1, 2), (1, 5))));
        let no_selection = Caret { line: 1, col: 2, anchor: Some((1, 2)) };
        assert_eq!(no_selection.range(), None);
        let bare = Caret { line: 1, col: 2, anchor: None };
        assert_eq!(bare.range(), None);
    }

    #[test]
    fn find_next_occurrence_searches_forward_across_lines() {
        let lines = vec!["let x = foo;".to_string(), "foo();".to_string()];
        // Desde el final de la primera aparición salta a la de la línea 2.
        assert_eq!(find_next_occurrence(&lines, "foo", (0, 11)), Some(((1, 0), (1, 3))));
        // Sin más apariciones devuelve None.
        assert_eq!(find_next_occurrence(&lines, "foo", (1, 3)), None);
        // Un texto vacío no busca nada.
        assert_eq!(find_next_occurrence(&lines, "", (0, 0)), None);
    }

    #[test]
    fn insert_text_multi_does_not_shift_earlier_positions() {
        let mut lines = vec!["abc".to_string(), "abc".to_string()];
        let new_positions = insert_text_multi(&mut lines, &[(0, 1), (1, 1)], "X");
        assert_eq!(lines, vec!["aXbc".to_string(), "aXbc".to_string()]);
        assert_eq!(new_positions, vec![(0, 2), (1, 2)]);
    }

    #[test]
    fn delete_ranges_multi_removes_every_range() {
        let mut lines = vec!["abcdef".to_string(), "123456".to_string()];
        let starts = delete_ranges_multi(&mut lines, &[((0, 1), (0, 3)), ((1, 0), (1, 2))]);
        assert_eq!(lines, vec!["adef".to_string(), "3456".to_string()]);
        assert_eq!(starts, vec![(0, 1), (1, 0)]);
    }

    #[test]
    fn dedup_extra_carets_drops_duplicates_and_primary() {
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 0,
            extra_carets: vec![
                Caret { line: 0, col: 0, anchor: None },
                Caret { line: 0, col: 5, anchor: None },
                Caret { line: 0, col: 5, anchor: Some((0, 3)) },
            ],
            ..Default::default()
        };
        dedup_extra_carets(&mut state);
        assert_eq!(state.extra_carets.len(), 1, "solo queda el cursor distinto del primario");
        assert_eq!((state.extra_carets[0].line, state.extra_carets[0].col), (0, 5));
    }

    #[test]
    fn test_code_folding_toggle_and_row_counts() {
        let mut state = EditorState {
            folding_ranges: vec![karkinos_core::syntax_ast::FoldingRange {
                start_line: 1,
                end_line: 3,
                kind: karkinos_core::syntax_ast::FoldingKind::Function,
            }],
            ..Default::default()
        };

        let lines = vec![
            "fn before() {}".to_string(),
            "fn fold_me() {".to_string(),
            "    let a = 1;".to_string(),
            "}".to_string(),
            "fn after() {}".to_string(),
        ];

        // Inicialmente nada está plegado
        assert!(!state.is_line_folded(0));
        assert!(!state.is_line_folded(1));
        assert!(!state.is_line_folded(2));
        assert!(!state.is_line_folded(3));
        assert!(!state.is_line_folded(4));
        let rows_unfolded = rebuild_row_counts(&lines, 80, &state);
        assert_eq!(rows_unfolded, vec![1, 1, 1, 1, 1]);

        // Plegamos el bloque que inicia en la línea 1
        state.toggle_fold(1);
        assert!(state.collapsed_folds.contains(&1));
        assert!(!state.is_line_folded(1), "la línea cabecera se mantiene visible");
        assert!(state.is_line_folded(2), "el interior está oculto");
        assert!(state.is_line_folded(3), "el cierre está oculto");
        assert!(!state.is_line_folded(4), "el exterior sigue visible");

        // row_counts asigna 0 a las líneas plegadas
        let rows_folded = rebuild_row_counts(&lines, 80, &state);
        assert_eq!(rows_folded, vec![1, 1, 0, 0, 1]);
        let cum = build_cum_rows(&rows_folded);
        assert_eq!(*cum.last().unwrap(), 3, "el documento se achica a 3 filas visuales");

        // Desplegamos nuevamente
        state.toggle_fold(1);
        assert!(!state.collapsed_folds.contains(&1));
        let rows_restored = rebuild_row_counts(&lines, 80, &state);
        assert_eq!(rows_restored, vec![1, 1, 1, 1, 1]);
    }

    // Roadmap v0.2.50: smart delimiters & auto-closing pairs.

    #[test]
    fn smart_auto_close_paren_inserts_pair_and_centers_cursor() {
        let mut lines = vec![String::new()];
        let mut state = EditorState::default(); // cursor (0, 0)
        let outcome = handle_delimiter_char(&mut lines, &mut state, '(');
        assert_eq!(outcome, DelimiterOutcome::TextMutated);
        assert_eq!(lines, vec!["()".to_string()]);
        assert_eq!((state.cursor_line, state.cursor_col), (0, 1));
        assert_eq!(state.sel_anchor, Some((0, 1)));
    }

    #[test]
    fn smart_auto_close_skips_when_letter_follows() {
        let mut lines = vec!["abc".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 1, ..Default::default() };
        // cursor entre 'a' y 'b'; la letra 'b' a la derecha impide el auto-cierre.
        let outcome = handle_delimiter_char(&mut lines, &mut state, '(');
        assert_eq!(outcome, DelimiterOutcome::Unhandled);
        assert_eq!(lines, vec!["abc".to_string()]);
    }

    #[test]
    fn smart_overtype_closing_paren_advances_without_duplicating() {
        let mut lines = vec![")".to_string()];
        let mut state = EditorState::default(); // cursor (0, 0)
        let outcome = handle_delimiter_char(&mut lines, &mut state, ')');
        assert_eq!(outcome, DelimiterOutcome::CursorMoved);
        assert_eq!(lines, vec![")".to_string()]); // sin duplicar
        assert_eq!((state.cursor_line, state.cursor_col), (0, 1));
    }

    #[test]
    fn smart_selection_wrap_wraps_selected_text() {
        let mut lines = vec!["foo".to_string()];
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 3,
            sel_anchor: Some((0, 0)),
            ..Default::default()
        };
        let outcome = handle_delimiter_char(&mut lines, &mut state, '(');
        assert_eq!(outcome, DelimiterOutcome::TextMutated);
        assert_eq!(lines, vec!["(foo)".to_string()]);
        assert_eq!((state.cursor_line, state.cursor_col), (0, 5));
        assert_eq!(state.sel_anchor, Some((0, 5)));
    }

    #[test]
    fn smart_backspace_removes_empty_pair() {
        let mut lines = vec!["()".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 1, ..Default::default() };
        assert!(backspace_empty_pair(&mut lines, &mut state));
        assert_eq!(lines, vec![String::new()]);
        assert_eq!((state.cursor_line, state.cursor_col), (0, 0));
    }

    #[test]
    fn smart_backspace_leaves_non_empty_pair_alone() {
        let mut lines = vec!["(x)".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 2, ..Default::default() };
        assert!(!backspace_empty_pair(&mut lines, &mut state));
        assert_eq!(lines, vec!["(x)".to_string()]);
    }

    #[test]
    fn smart_enter_between_braces_expands_block() {
        let mut lines = vec!["{}".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 1, ..Default::default() };
        assert!(enter_between_braces(&mut lines, &mut state));
        assert_eq!(
            lines,
            vec!["{".to_string(), "    ".to_string(), "}".to_string()]
        );
        assert_eq!((state.cursor_line, state.cursor_col), (1, 4));
        assert_eq!(state.sel_anchor, Some((1, 4)));
    }

    #[test]
    fn smart_enter_between_braces_preserves_base_indent() {
        let mut lines = vec!["    {}".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 5, ..Default::default() };
        assert!(enter_between_braces(&mut lines, &mut state));
        assert_eq!(
            lines,
            vec!["    {".to_string(), "        ".to_string(), "    }".to_string()]
        );
        assert_eq!((state.cursor_line, state.cursor_col), (1, 8));
    }

    #[test]
    fn smart_enter_outside_braces_is_unhandled() {
        let mut lines = vec!["abc".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 1, ..Default::default() };
        assert!(!enter_between_braces(&mut lines, &mut state));
        assert_eq!(lines, vec!["abc".to_string()]);
    }

    #[test]
    fn smart_quote_overtype_inside_empty_pair_moves_cursor() {
        // Cursor at col 1 inside `""`
        let mut lines = vec!["\"\"".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 1, ..Default::default() };
        let outcome = handle_delimiter_char(&mut lines, &mut state, '"');
        assert_eq!(outcome, DelimiterOutcome::CursorMoved);
        assert_eq!((state.cursor_line, state.cursor_col), (0, 2));
    }

    #[test]
    fn smart_quote_before_existing_quote_does_not_overtype() {
        // Cursor at col 0 before `"hello"`
        let mut lines = vec!["\"hello\"".to_string()];
        let mut state = EditorState { cursor_line: 0, cursor_col: 0, ..Default::default() };
        let outcome = handle_delimiter_char(&mut lines, &mut state, '"');
        // Debe auto-cerrar o tratarse como apertura, NO saltar sobre la comilla existente
        assert_ne!(outcome, DelimiterOutcome::CursorMoved);
    }

    #[test]
    fn test_smear_step_converges_and_settles() {
        let start = egui::pos2(10.0, 20.0);
        let target = egui::pos2(50.0, 80.0);
        let mut current = start;
        let mut steps = 0;
        loop {
            let (next, animating) = smear_step(current, target, SMEAR_HEAD_EASING);
            current = next;
            steps += 1;
            if !animating {
                break;
            }
            assert!(steps < 50, "smear_step debe converger");
        }
        assert_eq!(current, target);
        assert!(steps > 1, "no debe saltar en un solo paso");
    }

    #[test]
    fn test_convex_hull_corners_and_collinear() {
        let pts = vec![
            egui::pos2(0.0, 0.0),
            egui::pos2(10.0, 0.0),
            egui::pos2(10.0, 20.0),
            egui::pos2(0.0, 20.0),
            egui::pos2(5.0, 5.0),
        ];
        let hull = convex_hull(pts);
        assert_eq!(hull.len(), 4, "los 4 vértices exteriores forman el hull");
        assert!(hull.contains(&egui::pos2(0.0, 0.0)));
        assert!(hull.contains(&egui::pos2(10.0, 0.0)));
        assert!(hull.contains(&egui::pos2(10.0, 20.0)));
        assert!(hull.contains(&egui::pos2(0.0, 20.0)));
    }

    #[test]
    fn test_preferred_col_preserves_position_across_empty_line() {
        let lines = vec![
            "0123456789012345678901234".to_string(), // len 25
            "".to_string(),                          // empty, len 0
            "012345678901234567890123456789".to_string(), // len 30
        ];
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 25,
            ..Default::default()
        };

        // Move down into the empty line: cursor lands on col 0, preferred_col is Some(25)
        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 1);
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.preferred_col, Some(25));

        // Move down into the third line: cursor restores to col 25
        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 2);
        assert_eq!(state.cursor_col, 25);
        assert_eq!(state.preferred_col, Some(25));

        // Move up back into empty line: cursor clamps to col 0
        move_cursor_up(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 1);
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.preferred_col, Some(25));

        // Move up back to first line: cursor restores to col 25
        move_cursor_up(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 0);
        assert_eq!(state.cursor_col, 25);
        assert_eq!(state.preferred_col, Some(25));
    }

    #[test]
    fn test_preferred_col_preserves_across_shorter_line() {
        let lines = vec![
            "0123456789".to_string(),          // len 10
            "ab".to_string(),                  // len 2
            "0123456789abcdef".to_string(),    // len 16
        ];
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 8,
            ..Default::default()
        };

        // Move down to short line: cursor clamped to 2, preferred_col is Some(8)
        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 1);
        assert_eq!(state.cursor_col, 2);
        assert_eq!(state.preferred_col, Some(8));

        // Move down to longer line: cursor restored to 8
        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 2);
        assert_eq!(state.cursor_col, 8);
        assert_eq!(state.preferred_col, Some(8));
    }

    #[test]
    fn test_preferred_col_across_multiple_empty_lines() {
        let lines = vec![
            "abcdefghij".to_string(), // len 10
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "012345678901234".to_string(), // len 15
        ];
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 7,
            ..Default::default()
        };

        for expected_line in 1..=3 {
            move_cursor_down(&lines, &mut state, false);
            assert_eq!(state.cursor_line, expected_line);
            assert_eq!(state.cursor_col, 0);
            assert_eq!(state.preferred_col, Some(7));
        }

        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 4);
        assert_eq!(state.cursor_col, 7);
        assert_eq!(state.preferred_col, Some(7));

        for expected_line in (1..=3).rev() {
            move_cursor_up(&lines, &mut state, false);
            assert_eq!(state.cursor_line, expected_line);
            assert_eq!(state.cursor_col, 0);
            assert_eq!(state.preferred_col, Some(7));
        }

        move_cursor_up(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 0);
        assert_eq!(state.cursor_col, 7);
        assert_eq!(state.preferred_col, Some(7));
    }

    #[test]
    fn test_preferred_col_resets_on_horizontal_movement() {
        let lines = vec![
            "0123456789".to_string(),
            "".to_string(),
            "0123456789".to_string(),
        ];
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 5,
            ..Default::default()
        };

        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 1);
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.preferred_col, Some(5));

        // Horizontal movement clears preferred_col
        state.preferred_col = None;
        state.cursor_col = 0;

        // Next vertical down should now capture col 0 as preferred
        move_cursor_down(&lines, &mut state, false);
        assert_eq!(state.cursor_line, 2);
        assert_eq!(state.cursor_col, 0);
        assert_eq!(state.preferred_col, Some(0));
    }

    #[test]
    fn test_selection_range_detects_active_selection_for_smear_suppression() {
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 5,
            sel_anchor: None,
            ..Default::default()
        };
        assert!(selection_range(&state).is_none(), "sin anchor no hay seleccion");

        state.sel_anchor = Some((0, 5));
        assert!(selection_range(&state).is_none(), "cursor == anchor no cuenta como seleccion");

        state.sel_anchor = Some((0, 0));
        assert!(selection_range(&state).is_some(), "rango activo detectado para suprimir smear");
    }

    // Espejo de la rama de `show()` que coloca el cursor con un clic simple.
    // `show()` depende de un `egui::Ui`/`egui::Response` reales que un test
    // unitario no puede construir, así que este helper reproduce esa decisión
    // para poder fijar el contrato. Mantener en sincronía con `show()`.
    fn place_cursor_like_click(
        state: &mut EditorState,
        clicked: (usize, usize),
        shift: bool,
        dragged: bool,
        drag_stopped: bool,
    ) {
        if !shift && !dragged && !drag_stopped {
            state.sel_anchor = Some(clicked);
        } else if shift && state.sel_anchor.is_none() {
            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
        }
        state.cursor_line = clicked.0;
        state.cursor_col = clicked.1;
        state.preferred_col = None;
    }

    #[test]
    fn test_mouse_click_without_drag_updates_anchor_to_cursor() {
        // Había una selección activa; un clic simple en otra posición debe
        // recolocar el ancla en el cursor en el MISMO frame, para que no quede
        // ninguna selección fantasma (selection_range == None).
        let mut state = EditorState {
            cursor_line: 1,
            cursor_col: 10,
            sel_anchor: Some((1, 4)),
            ..Default::default()
        };

        place_cursor_like_click(&mut state, (3, 2), false, false, false);

        assert_eq!(state.cursor_line, 3);
        assert_eq!(state.cursor_col, 2);
        assert_eq!(state.sel_anchor, Some((3, 2)), "el ancla debe coincidir con el cursor");
        assert_eq!(state.preferred_col, None);
        assert!(
            selection_range(&state).is_none(),
            "clic sin arrastre no debe dejar selección transitoria"
        );
    }

    #[test]
    fn test_mouse_click_without_drag_clears_prior_selection() {
        // Aunque el ancla apunte a otra línea, un clic simple la reemplaza por
        // la posición clickeada, colapsando la selección anterior al instante.
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 0,
            sel_anchor: Some((7, 9)),
            ..Default::default()
        };

        place_cursor_like_click(&mut state, (2, 5), false, false, false);

        assert_eq!(state.sel_anchor, Some((2, 5)));
        assert_eq!(state.cursor_line, 2);
        assert_eq!(state.cursor_col, 5);
        assert!(selection_range(&state).is_none());
    }

    #[test]
    fn test_mouse_drag_preserves_selection_anchor() {
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 2,
            sel_anchor: Some((0, 2)),
            ..Default::default()
        };

        place_cursor_like_click(&mut state, (0, 8), false, true, false);

        assert_eq!(state.sel_anchor, Some((0, 2)), "arrastrar no colapsa la selección");
        assert_eq!(state.cursor_col, 8);
        assert_eq!(selection_range(&state), Some(((0, 2), (0, 8))));
    }

    #[test]
    fn test_mouse_drag_stopped_preserves_selection_anchor() {
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 2,
            sel_anchor: Some((0, 2)),
            ..Default::default()
        };

        place_cursor_like_click(&mut state, (0, 8), false, false, true);

        assert_eq!(state.sel_anchor, Some((0, 2)), "soltar el arrastre conserva el ancla");
        assert_eq!(selection_range(&state), Some(((0, 2), (0, 8))));
    }

    #[test]
    fn test_shift_click_seeds_anchor_from_old_cursor_when_none() {
        let mut state = EditorState {
            cursor_line: 1,
            cursor_col: 3,
            sel_anchor: None,
            ..Default::default()
        };

        place_cursor_like_click(&mut state, (4, 6), true, false, false);

        assert_eq!(state.sel_anchor, Some((1, 3)), "Shift+clic ancla en el cursor previo");
        assert_eq!(selection_range(&state), Some(((1, 3), (4, 6))));
    }

    #[test]
    fn test_shift_click_keeps_existing_anchor() {
        let mut state = EditorState {
            cursor_line: 0,
            cursor_col: 2,
            sel_anchor: Some((0, 2)),
            ..Default::default()
        };

        place_cursor_like_click(&mut state, (2, 5), true, false, false);

        assert_eq!(state.sel_anchor, Some((0, 2)), "Shift+clic extiende la selección existente");
        assert_eq!(selection_range(&state), Some(((0, 2), (2, 5))));
    }
}
