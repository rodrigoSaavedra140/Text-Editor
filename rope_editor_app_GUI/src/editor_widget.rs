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
    last_wrap_chars: usize,
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

/// Dado un Y en "espacio de documento" (ya sumado el scroll), devuelve
/// (línea lógica, sub-fila dentro de esa línea).
fn line_and_subrow_at_doc_y(row_counts: &[usize], doc_y: f32, row_height: f32) -> (usize, usize) {
    let target_row = (doc_y / row_height).floor().max(0.0) as usize;
    let mut acc = 0usize;
    for (i, &rc) in row_counts.iter().enumerate() {
        if target_row < acc + rc {
            return (i, target_row - acc);
        }
        acc += rc;
    }
    let last = row_counts.len().saturating_sub(1);
    (last, row_counts.last().copied().unwrap_or(1).saturating_sub(1))
}

/// Fila visual absoluta (contando desde el principio del documento)
/// en la que cae (line, col).
fn visual_row_of(row_counts: &[usize], line: usize, col: usize, wrap_chars: usize) -> usize {
    let base: usize = row_counts[..line].iter().sum();
    base + col / wrap_chars
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

    // Reservamos SIEMPRE el ancho de la scrollbar para calcular el
    // wrap (no solo cuando está visible) — así el punto de corte de
    // las líneas no "salta" apenas aparece o desaparece la barra.
    let usable_width = (rect.width() - PAD_X - SCROLLBAR_W).max(10.0);
    let wrap_chars = wrap_chars_for(usable_width, char_width);

    // Reconstruir la caché de filas por línea SOLO si cambió la
    // cantidad de líneas o el ancho disponible (abrir archivo,
    // deshacer/rehacer, redimensionar ventana) — en el caso común
    // (tipear un carácter) esto se saltea, y más abajo parcheamos
    // solo la línea que cambió.
    if state.row_counts.len() != lines.len() || state.last_wrap_chars != wrap_chars {
        state.row_counts = rebuild_row_counts(lines, wrap_chars);
        state.last_wrap_chars = wrap_chars;
    }

    let total_rows: usize = state.row_counts.iter().sum();
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
                    line_and_subrow_at_doc_y(&state.row_counts, doc_y.max(0.0), row_height);
                let rel_x = (pos.x - rect.left() - PAD_X).max(0.0);
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
            } else {
                state.row_counts[state.cursor_line] =
                    row_count_for(&lines[state.cursor_line], wrap_chars);
            }
        };
    }

    // --- teclado ---
    if has_focus {
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
                    let (nl, nc) = insert_text(lines, (state.cursor_line, state.cursor_col), &t);
                    state.cursor_line = nl;
                    state.cursor_col = nc;
                    state.sel_anchor = Some((nl, nc));
                    changed = true;
                    resync_rows!();
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Enter => {
                        if let Some((s, e)) = selection_range(state) {
                            delete_range(lines, s, e);
                            state.cursor_line = s.0;
                            state.cursor_col = s.1;
                        }
                        let (nl, nc) =
                            insert_text(lines, (state.cursor_line, state.cursor_col), "\n");
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
                    egui::Key::A if modifiers.ctrl => {
                        state.sel_anchor = Some((0, 0));
                        state.cursor_line = lines.len() - 1;
                        state.cursor_col = line_char_count(&lines[state.cursor_line]);
                    }
                    egui::Key::C if modifiers.ctrl => {
                        if let Some((s, e)) = selection_range(state) {
                            let text = selected_text(lines, s, e);
                            ui.output_mut(|o| o.copied_text = text);
                        }
                    }
                    egui::Key::X if modifiers.ctrl => {
                        if let Some((s, e)) = selection_range(state) {
                            let text = selected_text(lines, s, e);
                            ui.output_mut(|o| o.copied_text = text);
                            delete_range(lines, s, e);
                            state.cursor_line = s.0;
                            state.cursor_col = s.1;
                            state.sel_anchor = Some((s.0, s.1));
                            changed = true;
                            resync_rows!();
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    // --- autoscroll vertical: que el cursor nunca quede fuera de
    // vista, SOLO cuando se acaba de mover ---
    if cursor_moved {
        let cursor_row = visual_row_of(&state.row_counts, state.cursor_line, state.cursor_col, wrap_chars);
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

    let mut visual_row = 0usize; // fila visual acumulada, línea por línea
    for (i, line) in lines.iter().enumerate() {
        let rows_here = state.row_counts.get(i).copied().unwrap_or(1);
        let line_top_y = rect.top() + PAD_Y + visual_row as f32 * row_height - state.scroll;
        let line_bottom_y = line_top_y + rows_here as f32 * row_height;

        if line_bottom_y < rect.top() || line_top_y > rect.bottom() {
            visual_row += rows_here;
            continue;
        }

        let char_len = line_char_count(line);
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
                        let x0 = rect.left() + PAD_X + (row_start.saturating_sub(start)) as f32 * char_width;
                        let x1 =
                            rect.left() + PAD_X + (row_end.saturating_sub(start)).max(row_start.saturating_sub(start) + 1) as f32 * char_width;
                        let sel_rect =
                            egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + row_height));
                        painter.rect_filled(sel_rect, 0.0, sel_color.gamma_multiply(0.5));
                    }
                }
            }

            if !chunk.is_empty() {
                let galley = ui.fonts(|f| f.layout_no_wrap(chunk.to_string(), font_id.clone(), text_color));
                painter.galley(egui::pos2(rect.left() + PAD_X, y), galley, text_color);
            }

            if has_focus && i == state.cursor_line && state.cursor_col >= start
                && (state.cursor_col < end || (state.cursor_col == end && sub == rows_here - 1))
            {
                let caret_x = rect.left() + PAD_X + (state.cursor_col - start) as f32 * char_width;
                painter.line_segment(
                    [egui::pos2(caret_x, y), egui::pos2(caret_x, y + row_height)],
                    egui::Stroke::new(1.5, egui::Color32::WHITE),
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