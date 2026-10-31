use eframe::egui;

/// Estado del cursor/selección/scroll del editor — vive en EditorApp,
/// no acá, porque tiene que sobrevivir entre frames.
/// S10: límites y tamaño por defecto de la fuente del editor.
pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// S10: acota el tamaño de fuente al rango permitido [8, 32] px.
pub fn clamp_font_size(size: f32) -> f32 {
    size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

/// Estado del cursor/selección/scroll del editor — vive en EditorApp,
/// no acá, porque tiene que sobrevivir entre frames.
#[derive(Debug, Clone)]
pub struct EditorState {
    pub cursor_line: usize,
    pub cursor_col: usize, // índice de CARÁCTER (no byte) dentro de la línea LÓGICA
    pub sel_anchor: Option<(usize, usize)>,
    pub scroll: f32, // vertical, en píxeles (ya no hay scroll horizontal: el texto se ajusta de línea)
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
    // S3 & S4 (Etapa 4): Diagnósticos en vivo y autocompletado LSP
    pub diagnostics: Vec<karkinos_core::lsp::LspDiagnostic>,
    pub hover_text: Option<String>,
    pub completions: Vec<karkinos_core::lsp::LspCompletionItem>,
    pub show_completions: bool,
    pub selected_completion: usize,
    pub inlay_hints: Vec<karkinos_core::lsp::LspInlayHint>,
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
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            cursor_line: 0,
            cursor_col: 0,
            sel_anchor: None,
            scroll: 0.0,
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
            diagnostics: Vec::new(),
            hover_text: None,
            completions: Vec::new(),
            show_completions: false,
            selected_completion: 0,
            inlay_hints: Vec::new(),
            breakpoints: std::collections::HashSet::new(),
            stopped_line: None,
            layout_cache: crate::syntax::LineLayoutCache::new(),
            ast_rev: 0,
            last_activity_time: 0.0,
        }
    }
}

impl EditorState {
    /// Pide desplazar la vista hasta el cursor en el próximo frame.
    pub fn request_autoscroll(&mut self) {
        self.needs_autoscroll = true;
    }

    /// S10: zoom in (+1px).
    pub fn zoom_in(&mut self) {
        self.font_size = clamp_font_size(self.font_size + 1.0);
        self.request_autoscroll();
    }

    /// S10: zoom out (-1px).
    pub fn zoom_out(&mut self) {
        self.font_size = clamp_font_size(self.font_size - 1.0);
        self.request_autoscroll();
    }

    /// S10: fija el tamaño de fuente acotado al rango válido.
    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = clamp_font_size(size);
        self.request_autoscroll();
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
    ((idle / CARET_BLINK_PERIOD_S).floor() as u64) % 2 == 0
}

/// S2 (Roadmap v0.2.9): segundos hasta el próximo cruce de fase (para
/// `request_repaint_after`): el parpadeo vive con eventos, nunca en
/// loop continuo — reposo efectivo ~0% de CPU salvo el blink a 2 Hz.
fn caret_blink_next_wake(now: f64, last_activity: f64) -> f32 {
    let idle = (now - last_activity).max(0.0);
    (CARET_BLINK_PERIOD_S - (idle % CARET_BLINK_PERIOD_S)) as f32
}

fn clamp_col(lines: &[String], line: usize, col: usize) -> usize {
    col.min(line_char_count(&lines[line]))
}

fn selection_range(state: &EditorState) -> Option<((usize, usize), (usize, usize))> {
    let anchor = state.sel_anchor?;
    let cursor = (state.cursor_line, state.cursor_col);
    if anchor == cursor {
        return None;
    }
    Some(if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) })
}

fn selected_text(lines: &[String], start: (usize, usize), end: (usize, usize)) -> String {
    let (sl, sc) = start;
    let (el, ec) = end;
    if sl == el {
        let sb = char_to_byte(&lines[sl], sc);
        let eb = char_to_byte(&lines[sl], ec);
        lines[sl][sb..eb].to_string()
    } else {
        let mut out = String::new();
        let sb = char_to_byte(&lines[sl], sc);
        out.push_str(&lines[sl][sb..]);
        for line in &lines[sl + 1..el] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        let eb = char_to_byte(&lines[el], ec);
        out.push_str(&lines[el][..eb]);
        out
    }
}

fn delete_range(lines: &mut Vec<String>, start: (usize, usize), end: (usize, usize)) {
    let (sl, sc) = start;
    let (el, ec) = end;
    if sl == el {
        let sb = char_to_byte(&lines[sl], sc);
        let eb = char_to_byte(&lines[sl], ec);
        lines[sl].replace_range(sb..eb, "");
    } else {
        let sb = char_to_byte(&lines[sl], sc);
        let eb = char_to_byte(&lines[el], ec);
        let tail = lines[el][eb..].to_string();
        lines[sl].truncate(sb);
        lines[sl].push_str(&tail);
        lines.drain(sl + 1..=el);
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

/// S4 (Etapa 2): indenta una línea insertando '\t' al inicio.
fn indent_line(line: &mut String) {
    line.insert(0, '\t');
}

/// S6: Comenta o descomenta el rango de líneas [start_line..=end_line] usando el prefijo indicado.
pub fn toggle_comment_range(
    lines: &mut Vec<String>,
    start_line: usize,
    end_line: usize,
    prefix: &str,
) {
    if lines.is_empty() {
        return;
    }
    let end_l = end_line.min(lines.len() - 1);
    let start_l = start_line.min(end_l);

    let all_commented = (start_l..=end_l).all(|l| {
        let trimmed = lines[l].trim_start();
        trimmed.is_empty() || trimmed.starts_with(prefix)
    });

    for l in start_l..=end_l {
        let line = &lines[l];
        if line.trim().is_empty() {
            continue;
        }
        if all_commented {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(line, indent_len);
            let rest = &line[indent_bytes..];
            if rest.starts_with(prefix) {
                let strip_len = if rest[prefix.len()..].starts_with(' ') {
                    prefix.len() + 1
                } else {
                    prefix.len()
                };
                lines[l].replace_range(indent_bytes..indent_bytes + strip_len, "");
            }
        } else {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(line, indent_len);
            let insert = format!("{} ", prefix);
            lines[l].insert_str(indent_bytes, &insert);
        }
    }
}

/// Texto que Ctrl+C/Ctrl+X copiarian: la selección si hay, y si no
/// la línea LÓGICA completa bajo el cursor (convención de editores
/// tipo VS Code). egui-winit consume Ctrl+C/X y solo emite
/// Event::Copy / Event::Cut sin payload, así que la decisión vive
/// en este helper puro.
fn copy_text(
    lines: &[String],
    sel: Option<((usize, usize), (usize, usize))>,
    cursor_line: usize,
) -> String {
    match sel {
        Some((s, e)) => selected_text(lines, s, e),
        None => lines[cursor_line.min(lines.len().saturating_sub(1))].clone(),
    }
}

/// Inserta `text` (puede tener saltos de línea, ej. al pegar) en
/// `at`, y devuelve la posición del cursor después de insertar.
fn insert_text(lines: &mut Vec<String>, at: (usize, usize), text: &str) -> (usize, usize) {
    let (line, col) = at;
    let byte = char_to_byte(&lines[line], col);

    if !text.contains('\n') {
        lines[line].insert_str(byte, text);
        return (line, col + text.chars().count());
    }

    let tail = lines[line][byte..].to_string();
    lines[line].truncate(byte);

    let mut parts: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
    let first = parts.remove(0);
    lines[line].push_str(&first);

    let last_idx = parts.len().saturating_sub(1);
    let mut insert_at = line + 1;
    let mut result = (line, line_char_count(&lines[line]));
    for (i, part) in parts.into_iter().enumerate() {
        if i == last_idx {
            let col_after = line_char_count(&part);
            let mut final_line = part;
            final_line.push_str(&tail);
            lines.insert(insert_at, final_line);
            result = (insert_at, col_after);
        } else {
            lines.insert(insert_at, part);
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
fn next_word_pos(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    let line = line.min(lines.len() - 1);
    let chars: Vec<char> = lines[line].chars().collect();
    let len = chars.len();
    if col >= len {
        if line + 1 < lines.len() {
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
fn prev_word_pos(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    if col == 0 {
        if line > 0 {
            (line - 1, line_char_count(&lines[line - 1]))
        } else {
            (0, 0)
        }
    } else {
        let line = line.min(lines.len() - 1);
        let chars: Vec<char> = lines[line].chars().collect();
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
    lines: &[String],
    cursor_line: usize,
    cursor_col: usize,
) -> Option<BracketMatch> {
    if lines.is_empty() || cursor_line >= lines.len() {
        return None;
    }
    let cur_line = &lines[cursor_line];
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
        let end_line = (cursor_line + max_lines).min(lines.len());
        for l in cursor_line..end_line {
            for (c_idx, c) in lines[l].chars().enumerate().skip(start_c) {
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
            let line_chars: Vec<char> = lines[l].chars().collect();
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
// Ajuste de línea (word wrap) — todo lo de acá para abajo asume
// fuente monoespaciada, así que "wrap_chars" caracteres entran
// exactos en el ancho disponible.
// ============================================================

fn wrap_chars_for(usable_width: f32, char_width: f32) -> usize {
    ((usable_width / char_width).floor() as usize).max(1)
}

fn row_count_for(line: &str, wrap_chars: usize) -> usize {
    let len = line_char_count(line);
    if len == 0 {
        1
    } else {
        (len + wrap_chars - 1) / wrap_chars
    }
}

fn rebuild_row_counts(lines: &[String], wrap_chars: usize) -> Vec<usize> {
    lines.iter().map(|l| row_count_for(l, wrap_chars)).collect()
}

fn rebuild_char_lens(lines: &[String]) -> Vec<usize> {
    lines.iter().map(|l| l.chars().count()).collect()
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
    lines: &mut Vec<String>,
    state: &mut EditorState,
    theme: &crate::theme::Theme,
    language: crate::syntax::Language,
    search: Option<SearchHighlight<'_>>,
) -> EditorResponse {
    let mut changed = false;
    let mut force_snapshot = false;
    let mut pre_break_snapshot: Option<String> = None;

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
    let gutter_w = gutter_width_for(lines.len(), char_width);
    // Origen del texto: después de la banda (y del PAD_X de siempre).
    let text_left = rect.left() + gutter_w + PAD_X;

    // Reservamos SIEMPRE el ancho de la scrollbar y del gutter para
    // calcular el wrap — así el punto de corte de las líneas no
    // "salta" cuando la banda gana un dígito o aparece la barra.
    let usable_width = (rect.width() - gutter_w - PAD_X - SCROLLBAR_W).max(10.0);
    let wrap_chars = wrap_chars_for(usable_width, char_width);

    // Reconstruir la caché de filas por línea SOLO si cambió la
    // cantidad de líneas o el ancho disponible (abrir archivo,
    // deshacer/rehacer, redimensionar ventana, gutter que crece) —
    // en el caso común (tipear un carácter) esto se saltea, y más
    // abajo parcheamos solo la línea que cambió.
    if state.row_counts.len() != lines.len()
        || state.last_wrap_chars != wrap_chars
        || state.last_gutter != gutter_w
    {
        state.row_counts = rebuild_row_counts(lines, wrap_chars);
        state.char_lens = rebuild_char_lens(lines);
        state.cum_rows = build_cum_rows(&state.row_counts);
        state.last_wrap_chars = wrap_chars;
        state.last_gutter = gutter_w;
    }

    let total_rows: usize = *state.cum_rows.last().unwrap_or(&0);
    let total_height = total_rows as f32 * row_height;
    let max_scroll = (total_height - rect.height()).max(0.0);
    let show_scrollbar = max_scroll > 0.0;
    let scrollbar_track = egui::Rect::from_min_max(
        egui::pos2(rect.right() - SCROLLBAR_W, rect.top()),
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
                state.scroll = rel * max_scroll;
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
                    // S8: Clic en el gutter (banda de números) selecciona la línea completa
                    let line_end = line_char_count(&lines[clicked_line]);
                    state.sel_anchor = Some((clicked_line, 0));
                    state.cursor_line = clicked_line;
                    state.cursor_col = line_end;
                } else {
                    let rel_x = (pos.x - text_left).max(0.0);
                    let col_in_row = (rel_x / char_width).round() as usize;
                    let clicked_col =
                        (sub_row * wrap_chars + col_in_row).min(line_char_count(&lines[clicked_line]));

                    // S5: doble click selecciona palabra, triple click selecciona línea lógica
                    if response.triple_clicked() {
                        state.sel_anchor = Some((clicked_line, 0));
                        state.cursor_line = clicked_line;
                        state.cursor_col = line_char_count(&lines[clicked_line]);
                    } else if response.double_clicked() {
                        let (ws, we) = word_range_at(&lines[clicked_line], clicked_col);
                        state.sel_anchor = Some((clicked_line, ws));
                        state.cursor_line = clicked_line;
                        state.cursor_col = we;
                    } else {
                        if response.clicked() && !response.dragged() {
                            state.sel_anchor = Some((clicked_line, clicked_col));
                        } else if response.drag_started() {
                            state.sel_anchor = Some((clicked_line, clicked_col));
                        }
                        state.cursor_line = clicked_line;
                        state.cursor_col = clicked_col;
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
            if state.row_counts.len() != lines.len() {
                state.row_counts = rebuild_row_counts(lines, wrap_chars);
                state.char_lens = rebuild_char_lens(lines);
                state.cum_rows = build_cum_rows(&state.row_counts);
            } else {
                // La línea editada es cursor_line (todas las mutaciones
                // dejan el cursor sobre la línea que cambió): parcheamos
                // SOLO su entrada de las tres cachés, en O(n) solo si
                // cambió su cantidad de filas (ajuste del sumatorio).
                let old_rows = state.row_counts[state.cursor_line];
                let new_rows = row_count_for(&lines[state.cursor_line], wrap_chars);
                state.row_counts[state.cursor_line] = new_rows;
                state.char_lens[state.cursor_line] = line_char_count(&lines[state.cursor_line]);
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
        let has_events = ui.input(|i| !i.events.is_empty());
        if has_events {
            cursor_moved = true;
            let events = ui.input(|i| i.events.clone());
            if events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::PointerButton { .. })) {
                state.hover_text = None;
            }
            for event in events {
            match event {
                egui::Event::Text(t) => {
                    if let Some((s, e)) = selection_range(state) {
                        delete_range(lines, s, e);
                        state.cursor_line = s.0;
                        state.cursor_col = s.1;
                    }
                    let (nl, nc) = insert_text(lines, (state.cursor_line, state.cursor_col), &t);
                    state.cursor_line = nl;
                    state.cursor_col = nc;
                    state.sel_anchor = Some((nl, nc));
                    changed = true;
                    resync_rows!();
                }
                egui::Event::Paste(t) => {
                    pre_break_snapshot = Some(lines.join("\n"));
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
                                for l in from_l..=to_l {
                                    let rem = dedent_line(&mut lines[l]);
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
                                // S4 (Etapa 2): Tab indenta bloque multilínea
                                for l in from_l..=to_l {
                                    indent_line(&mut lines[l]);
                                }
                                let new_s_col = s.1 + 1;
                                let new_e_col = if e.1 > 0 || e.0 == to_l { e.1 + 1 } else { e.1 };
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
                            state.row_counts = rebuild_row_counts(lines, wrap_chars);
                            state.char_lens = rebuild_char_lens(lines);
                            state.cum_rows = build_cum_rows(&state.row_counts);
                        } else if modifiers.shift {
                            // S4 (Etapa 2): Shift+Tab en línea simple desindenta la línea actual
                            let rem = dedent_line(&mut lines[state.cursor_line]);
                            if rem > 0 {
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
                            // S6: Tab en línea simple inserta \t
                            if let Some((s, e)) = sel {
                                delete_range(lines, s, e);
                                state.cursor_line = s.0;
                                state.cursor_col = s.1;
                            }
                            let (nl, nc) =
                                insert_text(lines, (state.cursor_line, state.cursor_col), "\t");
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                            state.sel_anchor = Some((nl, nc));
                            changed = true;
                            resync_rows!();
                        }
                    }
                    egui::Key::Enter => {
                        pre_break_snapshot = Some(lines.join("\n"));
                        if let Some((s, e)) = selection_range(state) {
                            delete_range(lines, s, e);
                            state.cursor_line = s.0;
                            state.cursor_col = s.1;
                        }
                        // S6: la línea nueva hereda la sangría de la
                        // línea en la que estaba el cursor. (se lee
                        // DESPUÉS de descartar la selección y ANTES de
                        // partir la línea en dos).
                        let indenta = inherited_indent(&lines[state.cursor_line]);
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
                    egui::Key::Backspace => {
                        if let Some((s, e)) = selection_range(state) {
                            delete_range(lines, s, e);
                            state.cursor_line = s.0;
                            state.cursor_col = s.1;
                        } else if state.cursor_col > 0 {
                            let bs = char_to_byte(&lines[state.cursor_line], state.cursor_col - 1);
                            let be = char_to_byte(&lines[state.cursor_line], state.cursor_col);
                            lines[state.cursor_line].replace_range(bs..be, "");
                            state.cursor_col -= 1;
                        } else if state.cursor_line > 0 {
                            let prev_len = line_char_count(&lines[state.cursor_line - 1]);
                            let cur = lines.remove(state.cursor_line);
                            lines[state.cursor_line - 1].push_str(&cur);
                            state.cursor_line -= 1;
                            state.cursor_col = prev_len;
                        }
                        state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        changed = true;
                        resync_rows!();
                    }
                    egui::Key::Delete => {
                        if let Some((s, e)) = selection_range(state) {
                            delete_range(lines, s, e);
                            state.cursor_line = s.0;
                            state.cursor_col = s.1;
                        } else if state.cursor_col < line_char_count(&lines[state.cursor_line]) {
                            let bs = char_to_byte(&lines[state.cursor_line], state.cursor_col);
                            let be = char_to_byte(&lines[state.cursor_line], state.cursor_col + 1);
                            lines[state.cursor_line].replace_range(bs..be, "");
                        } else if state.cursor_line + 1 < lines.len() {
                            let next = lines.remove(state.cursor_line + 1);
                            lines[state.cursor_line].push_str(&next);
                        }
                        state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        changed = true;
                        resync_rows!();
                    }
                    egui::Key::ArrowLeft => {
                        if modifiers.ctrl || modifiers.command {
                            let (nl, nc) = prev_word_pos(lines, state.cursor_line, state.cursor_col);
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                        } else if state.cursor_col > 0 {
                            state.cursor_col -= 1;
                        } else if state.cursor_line > 0 {
                            state.cursor_line -= 1;
                            state.cursor_col = line_char_count(&lines[state.cursor_line]);
                        }
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::ArrowRight => {
                        if modifiers.ctrl || modifiers.command {
                            let (nl, nc) = next_word_pos(lines, state.cursor_line, state.cursor_col);
                            state.cursor_line = nl;
                            state.cursor_col = nc;
                        } else if state.cursor_col < line_char_count(&lines[state.cursor_line]) {
                            state.cursor_col += 1;
                        } else if state.cursor_line + 1 < lines.len() {
                            state.cursor_line += 1;
                            state.cursor_col = 0;
                        }
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::PageUp => {
                        let page_rows = ((rect.height() / row_height).floor() as usize).max(1);
                        let total_rows = *state.cum_rows.last().unwrap_or(&1);
                        let cur_row = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
                        let target_row = page_target_row(cur_row, total_rows, page_rows, false);
                        let target_line = line_at_visual_row(&state.cum_rows, target_row);
                        let sub_row = target_row.saturating_sub(state.cum_rows[target_line]);
                        state.cursor_line = target_line;
                        state.cursor_col = clamp_col(lines, target_line, sub_row * wrap_chars + (state.cursor_col % wrap_chars));
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::PageDown => {
                        let page_rows = ((rect.height() / row_height).floor() as usize).max(1);
                        let total_rows = *state.cum_rows.last().unwrap_or(&1);
                        let cur_row = visual_row_of(&state.cum_rows, state.cursor_line, state.cursor_col, wrap_chars);
                        let target_row = page_target_row(cur_row, total_rows, page_rows, true);
                        let target_line = line_at_visual_row(&state.cum_rows, target_row);
                        let sub_row = target_row.saturating_sub(state.cum_rows[target_line]);
                        state.cursor_line = target_line;
                        state.cursor_col = clamp_col(lines, target_line, sub_row * wrap_chars + (state.cursor_col % wrap_chars));
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::ArrowUp => {
                        if state.cursor_line > 0 {
                            state.cursor_line -= 1;
                            state.cursor_col = clamp_col(lines, state.cursor_line, state.cursor_col);
                        }
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::ArrowDown => {
                        if state.cursor_line + 1 < lines.len() {
                            state.cursor_line += 1;
                            state.cursor_col = clamp_col(lines, state.cursor_line, state.cursor_col);
                        }
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::Home => {
                        state.cursor_col = 0;
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::End => {
                        state.cursor_col = line_char_count(&lines[state.cursor_line]);
                        if !modifiers.shift {
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                    }
                    egui::Key::A if modifiers.command || modifiers.ctrl => {
                        state.sel_anchor = Some((0, 0));
                        state.cursor_line = lines.len() - 1;
                        state.cursor_col = line_char_count(&lines[state.cursor_line]);
                    }
                    egui::Key::D if (modifiers.command || modifiers.ctrl) && !modifiers.shift => {
                        // S7: Duplicar línea o bloque seleccionado
                        let sel = selection_range(state);
                        if let Some((s, e)) = sel {
                            let from_l = s.0;
                            let to_l = if e.1 == 0 && e.0 > s.0 { e.0 - 1 } else { e.0 };
                            let cloned_lines: Vec<String> = lines[from_l..=to_l].to_vec();
                            let insert_idx = to_l + 1;
                            let num_cloned = cloned_lines.len();
                            for (offset, line) in cloned_lines.into_iter().enumerate() {
                                lines.insert(insert_idx + offset, line);
                            }
                            state.cursor_line += num_cloned;
                            state.sel_anchor = Some((s.0 + num_cloned, s.1));
                        } else {
                            let cur_line = lines[state.cursor_line].clone();
                            lines.insert(state.cursor_line + 1, cur_line);
                            state.cursor_line += 1;
                            state.sel_anchor = Some((state.cursor_line, state.cursor_col));
                        }
                        changed = true;
                        force_snapshot = true;
                        state.row_counts = rebuild_row_counts(lines, wrap_chars);
                        state.char_lens = rebuild_char_lens(lines);
                        state.cum_rows = build_cum_rows(&state.row_counts);
                    }
                    egui::Key::Slash if modifiers.command || modifiers.ctrl => {
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
                            state.row_counts = rebuild_row_counts(lines, wrap_chars);
                            state.char_lens = rebuild_char_lens(lines);
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
        }
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
    }

    // --- scroll con la rueda del mouse / trackpad ---
    // S10: con Ctrl tocando, la rueda zoomea en vez de scrollear
    // (UN paso por evento de rueda, no por píxel de trackpad).
    if response.hovered() {
        let (scroll_delta, ctrl) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers.ctrl));
        if ctrl {
            // Un notch por evento de rueda, no por píxel de trackpad.
            if scroll_delta.y > 2.0 {
                state.zoom_in();
            } else if scroll_delta.y < -2.0 {
                state.zoom_out();
            }
        } else {
            state.scroll -= scroll_delta.y;
        }
        // S10: el tamaño puede haber cambiado por zoom o desde afuera
        // (botones +/-): pedir re-layout/autoscroll cuando difiere del
        // frame anterior.
        if state.last_font_size != state.font_size {
            state.last_font_size = state.font_size;
            state.request_autoscroll();
        }
    }
    state.scroll = state.scroll.clamp(0.0, max_scroll);

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
    let start_line = line_at_visual_row(&state.cum_rows, first_row).min(lines.len().saturating_sub(1));
    // S1 (Roadmap v0.2.9): se alinea la estampa del cache con la revisión
    // del AST antes de recorrer las líneas visibles. Si `update_ast`
    // regeneró el árbol, acá se vacía la cache de una sola vez.
    state.layout_cache.sync_ast(state.ast_rev);
    let mut visual_row = state.cum_rows[start_line];
    for i in start_line..lines.len() {
        let line = &lines[i];
        let rows_here = state.row_counts.get(i).copied().unwrap_or(1);
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

        // S3 (Etapa 3): Indicador de plegado AST en el gutter
        if state.folding_ranges.iter().any(|r| r.start_line == i) {
            gutter_painter.text(
                egui::pos2(gutter_rect.left() + 2.0, line_top_y),
                egui::Align2::LEFT_TOP,
                "▾",
                egui::FontId::monospace(state.font_size * 0.8),
                theme.syntax_keyword,
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
            .unwrap_or_else(|| line_char_count(line));
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
            let sb = if is_pure_ascii { start } else { char_to_byte(line, start) };
            let eb = if is_pure_ascii { end } else { char_to_byte(line, end) };
            let chunk = &line[sb..eb];

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
                        let x0 = text_left + (ov_start - start) as f32 * char_width;
                        let x1 = text_left + (ov_end - start) as f32 * char_width;
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
                        let x0 = text_left + (row_start.saturating_sub(start)) as f32 * char_width;
                        let x1 =
                            text_left + (row_end.saturating_sub(start)).max(row_start.saturating_sub(start) + 1) as f32 * char_width;
                        let sel_rect =
                            egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + row_height));
                        painter.rect_filled(sel_rect, 0.0, sel_color.gamma_multiply(0.5));
                    }
                }
            }

            if !chunk.is_empty() {
                // S1 (Roadmap v0.2.9): el galley sale del cache si este
                // fragmento ya se tipografió (mismo texto/lenguaje/tema/
                // fuente). Solo en cache miss se arma el LayoutJob, se
                // tokeniza la línea y egui hace el layout.
                let key = crate::syntax::line_layout_key(chunk, language, state.font_size);
                let galley = state.layout_cache.get_or_insert(key, theme.mode, || {
                    let tokens = line_tokens.get_or_insert_with(|| {
                        if language == crate::syntax::Language::PlainText {
                            Vec::new()
                        } else {
                        crate::syntax::tokenize_line_semantic(
                            line,
                            i,
                            language,
                            Some(state.ast_spans.as_slice()),
                        )
                        }
                    });
                    if language == crate::syntax::Language::PlainText {
                        ui.fonts(|f| {
                            f.layout_no_wrap(chunk.to_string(), font_id.clone(), text_color)
                        })
                    } else {
                        let job =
                            crate::syntax::build_layout_job(chunk, sb, tokens, theme, font_id.clone());
                        ui.fonts(|f| f.layout_job(job))
                    }
                });
                painter.galley(egui::pos2(text_left, y), galley, text_color);
            }

            // S2 (Rust IDE): Render Inlay Hints (types, parameter names)
            for hint in &state.inlay_hints {
                if hint.position.line == i
                    && hint.position.character >= start
                    && hint.position.character <= end
                {
                    let hx = text_left + (hint.position.character - start) as f32 * char_width;
                    let hint_font = egui::FontId::monospace(state.font_size * 0.78);
                    let hint_color = egui::Color32::from_rgb(130, 155, 195);
                    let hint_bg = egui::Color32::from_rgba_unmultiplied(35, 45, 65, 140);

                    let hint_galley =
                        ui.fonts(|f| f.layout_no_wrap(hint.label.clone(), hint_font, hint_color));
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
                let is_partner = bm.partner.map_or(false, check_pos);

                if is_origin || is_partner {
                    let col = if is_origin { bm.origin.1 } else { bm.partner.unwrap().1 };
                    let bx0 = text_left + (col - start) as f32 * char_width;
                    let b_rect = egui::Rect::from_min_size(
                        egui::pos2(bx0, y),
                        egui::vec2(char_width, row_height),
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

            if has_focus
                && caret_on
                && i == state.cursor_line
                && state.cursor_col >= start
                && (state.cursor_col < end || (state.cursor_col == end && sub == rows_here - 1))
            {
                let caret_x = text_left + (state.cursor_col - start) as f32 * char_width;
                painter.line_segment(
                    [egui::pos2(caret_x, y), egui::pos2(caret_x, y + row_height)],
                    egui::Stroke::new(1.5_f32, theme.caret),
                );
            }
        }

        visual_row += rows_here;
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
        let cur_x = text_left + (state.cursor_col % wrap_chars.max(1)) as f32 * char_width;

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
            let cur_x = text_left + (state.cursor_col % wrap_chars.max(1)) as f32 * char_width;

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
        assert_eq!(line_clean, "\tcodigo");
        assert_eq!(dedent_line(&mut line_clean), 1);
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
}