use eframe::egui;

/// Estado del cursor/selección/scroll del editor — vive en EditorApp,
/// no acá, porque tiene que sobrevivir entre frames.
#[derive(Default)]
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
}

impl EditorState {
    /// Pide desplazar la vista hasta el cursor en el próximo frame.
    pub fn request_autoscroll(&mut self) {
        self.needs_autoscroll = true;
    }
}

pub struct EditorResponse {
    pub changed: bool,
    pub lost_focus: bool,
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

fn line_char_count(s: &str) -> usize {
    s.chars().count()
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

/// Dibuja y maneja el editor de texto, mostrando SOLO las filas que
/// entran en pantalla (a diferencia de egui::TextEdit, que relayoutea
/// el texto entero en cada tecla). Ajusta de línea automáticamente:
/// una línea lógica larga ocupa varias filas visuales, así nunca se
/// sale de la pantalla hacia los costados.
pub fn show(
    ui: &mut egui::Ui,
    lines: &mut Vec<String>,
    state: &mut EditorState,
    bg: egui::Color32,
) -> EditorResponse {
    let mut changed = false;

    let font_id = egui::FontId::monospace(14.0);
    let row_height = ui.fonts(|f| f.row_height(&font_id));
    let char_width = ui.fonts(|f| f.glyph_width(&font_id, ' '));
    let text_color = egui::Color32::from_gray(225);
    let sel_color = egui::Color32::from_rgb(60, 90, 150);

    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    let _ = bg; // el fondo ya lo pinta el CentralPanel de ui.rs

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
                let rel_x = (pos.x - text_left).max(0.0);
                let col_in_row = (rel_x / char_width).round() as usize;
                let clicked_col =
                    (sub_row * wrap_chars + col_in_row).min(line_char_count(&lines[clicked_line]));

                if response.clicked() && !response.dragged() {
                    state.sel_anchor = Some((clicked_line, clicked_col));
                } else if response.drag_started() {
                    state.sel_anchor = Some((clicked_line, clicked_col));
                }
                state.cursor_line = clicked_line;
                state.cursor_col = clicked_col;
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
        // S6: mientras el editor tiene foco, egui NO usa Tab para
        // cambiar de widget enfocado — así Event::Key{Tab} nos llega
        // y el widget lo inserta como caracter. (Mismos términos que
        // usa egui::TextEdit para las flechas: EventFilter.)
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    ..Default::default()
                },
            )
        });
        let events = ui.input(|i| i.events.clone());
        if !events.is_empty() {
            cursor_moved = true;
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
                    resync_rows!();
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Tab
                        if !(modifiers.shift
                            || modifiers.ctrl
                            || modifiers.command
                            || modifiers.alt) =>
                    {
                        // S6: Tab CON foco del editor inserta el
                        // caracter (no se roba el foco: el filtro de
                        // foco de egui quedó desactivado para Tab,
                        // ver set_focus_lock_filter más abajo).
                        let (nl, nc) =
                            insert_text(lines, (state.cursor_line, state.cursor_col), "\t");
                        state.cursor_line = nl;
                        state.cursor_col = nc;
                        state.sel_anchor = Some((nl, nc));
                        changed = true;
                        resync_rows!();
                    }
                    egui::Key::Enter => {
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
                        if state.cursor_col > 0 {
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
                        if state.cursor_col < line_char_count(&lines[state.cursor_line]) {
                            state.cursor_col += 1;
                        } else if state.cursor_line + 1 < lines.len() {
                            state.cursor_line += 1;
                            state.cursor_col = 0;
                        }
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
                    egui::Key::A if modifiers.command => {
                        state.sel_anchor = Some((0, 0));
                        state.cursor_line = lines.len() - 1;
                        state.cursor_col = line_char_count(&lines[state.cursor_line]);
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
    if response.hovered() {
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        state.scroll -= scroll_delta.y;
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

    // S4: banda de números de línea, pintada ANTES que el texto
    // (mismo layer: orden de llamadas = orden de pintura).
    let gutter_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + gutter_w, rect.max.y));
    let gutter_painter = ui.painter_at(gutter_rect);
    gutter_painter.rect_filled(gutter_rect, 0.0, egui::Color32::from_white_alpha(5));

    // La primera línea visible sale O(log n) de la búsqueda binaria
    // sobre cum_rows: el loop de abajo solo recorre las filas que
    // entran en pantalla (más la primera, que puede verse partida),
    // no el documento entero como antes.
    let first_row = ((state.scroll / row_height).floor().max(0.0)) as usize;
    let start_line = line_at_visual_row(&state.cum_rows, first_row).min(lines.len().saturating_sub(1));
    let mut visual_row = state.cum_rows[start_line];
    for i in start_line..lines.len() {
        let line = &lines[i];
        let rows_here = state.row_counts.get(i).copied().unwrap_or(1);
        let line_top_y = rect.top() + PAD_Y + visual_row as f32 * row_height - state.scroll;

        if line_top_y > rect.bottom() {
            break;
        }

        // S4: el número se alinea a la FILA VISUAL primera de la
        // línea lógica (una línea envuelta muestra su número una
        // sola vez) y right-align , para que los dígitos no bailen.
        gutter_painter.text(
            egui::pos2(gutter_rect.right() - GUTTER_PAD, line_top_y),
            egui::Align2::RIGHT_TOP,
            (i + 1).to_string(),
            font_id.clone(),
            egui::Color32::from_gray(110),
        );

        let char_len = state
            .char_lens
            .get(i)
            .copied()
            .unwrap_or_else(|| line_char_count(line));
        for sub in 0..rows_here {
            let y = line_top_y + sub as f32 * row_height;
            if y + row_height < rect.top() || y > rect.bottom() {
                continue;
            }
            let start = sub * wrap_chars;
            let end = (start + wrap_chars).min(char_len);
            let sb = char_to_byte(line, start);
            let eb = char_to_byte(line, end);
            let chunk = &line[sb..eb];

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
                let galley = ui.fonts(|f| f.layout_no_wrap(chunk.to_string(), font_id.clone(), text_color));
                painter.galley(egui::pos2(text_left, y), galley, text_color);
            }

            if has_focus && i == state.cursor_line && state.cursor_col >= start
                && (state.cursor_col < end || (state.cursor_col == end && sub == rows_here - 1))
            {
                let caret_x = text_left + (state.cursor_col - start) as f32 * char_width;
                painter.line_segment(
                    [egui::pos2(caret_x, y), egui::pos2(caret_x, y + row_height)],
                    egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                );
            }
        }

        visual_row += rows_here;
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

    EditorResponse { changed, lost_focus }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}