use eframe::egui;

/// Estado del cursor/selección/scroll del editor — vive en EditorApp,
/// no acá, porque tiene que sobrevivir entre frames.
#[derive(Default)]
pub struct EditorState {
    pub cursor_line: usize,
    pub cursor_col: usize, // índice de CARÁCTER (no byte) dentro de la línea
    pub sel_anchor: Option<(usize, usize)>,
    pub scroll: f32,   // vertical, en píxeles
    pub scroll_x: f32, // horizontal, en píxeles
    was_focused: bool,
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

/// Dibuja y maneja el editor de texto, mostrando SOLO las líneas que
/// entran en pantalla (a diferencia de egui::TextEdit, que relayoutea
/// el texto entero en cada tecla — por eso se ponía lento con
/// archivos grandes).
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
    ui.painter().rect_filled(rect, 0.0, bg);

    let id = response.id;
    if response.clicked() || response.drag_started() {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let has_focus = ui.memory(|m| m.has_focus(id));
    let lost_focus = state.was_focused && !has_focus;
    state.was_focused = has_focus;

    // Solo forzamos que el cursor quede visible cuando realmente se
    // movió (teclado/click) — si no, el autoscroll le pelea a
    // cualquier scroll manual con la rueda del mouse: como la rueda
    // no mueve el cursor, en el frame siguiente el autoscroll "corrige"
    // devolviéndote a donde estaba antes de scrollear.
    let mut cursor_moved = false;

    // --- click / arrastre: mueve el cursor y arma la selección ---
    if let Some(pos) = response.interact_pointer_pos() {
        let rel_y = pos.y - rect.top() + state.scroll;
        let clicked_line =
            ((rel_y / row_height).floor().max(0.0) as usize).min(lines.len().saturating_sub(1));
        let rel_x = (pos.x - rect.left() + state.scroll_x).max(0.0);
        let clicked_col = ((rel_x / char_width).round() as usize).min(line_char_count(&lines[clicked_line]));

        if response.clicked() && !response.dragged() {
            state.sel_anchor = Some((clicked_line, clicked_col));
        } else if response.drag_started() {
            state.sel_anchor = Some((clicked_line, clicked_col));
        }
        state.cursor_line = clicked_line;
        state.cursor_col = clicked_col;
        cursor_moved = true;
    }

    // --- teclado ---
    if has_focus {
        let events = ui.input(|i| i.events.clone());
        if !events.is_empty() {
            // Aproximación simple: casi cualquier evento de teclado
            // puede mover el cursor (escribir, flechas, Enter,
            // Backspace...). Para las pocas teclas que no lo mueven
            // (ej. Ctrl+C) esto solo hace que el autoscroll reafirme
            // la posición actual — no se nota ningún efecto raro.
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
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    // --- autoscroll: que el cursor nunca quede fuera de vista (ni
    // vertical ni horizontalmente), SOLO cuando se acaba de mover ---
    if cursor_moved {
        let cursor_y = state.cursor_line as f32 * row_height;
        if cursor_y < state.scroll {
            state.scroll = cursor_y;
        } else if cursor_y + row_height > state.scroll + rect.height() {
            state.scroll = cursor_y + row_height - rect.height();
        }

        let cursor_x = state.cursor_col as f32 * char_width;
        if cursor_x < state.scroll_x {
            state.scroll_x = cursor_x;
        } else if cursor_x + char_width > state.scroll_x + rect.width() {
            state.scroll_x = cursor_x + char_width - rect.width();
        }
    }

    // --- scroll con la rueda del mouse / trackpad ---
    // smooth_scroll_delta (en vez de raw_scroll_delta) es lo que
    // recomienda la documentación de egui para este uso: raw_scroll_delta
    // da un pico grande por un solo frame y cae a cero enseguida, lo
    // cual hacía que el scroll se sintiera trabado.
    if response.hovered() {
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        state.scroll -= scroll_delta.y;
        state.scroll_x -= scroll_delta.x;
    }
    let max_scroll = (lines.len() as f32 * row_height - rect.height()).max(0.0);
    state.scroll = state.scroll.clamp(0.0, max_scroll);
    state.scroll_x = state.scroll_x.max(0.0);

    // --- dibujar SOLO las líneas visibles (la clave del rendimiento:
    // esto es O(líneas visibles), no O(líneas totales del archivo)) ---
    // Recortamos al rect del editor para que el texto que se sale por
    // los costados (líneas largas) no se dibuje encima de otros
    // paneles — se ve, pero solo dentro del área de edición.
    let painter = ui.painter_at(rect);
    let sel = selection_range(state);
    for (i, line) in lines.iter().enumerate() {
        let y = rect.top() + i as f32 * row_height - state.scroll;
        if y + row_height < rect.top() || y > rect.bottom() {
            continue;
        }
        let x_off = rect.left() - state.scroll_x;

        if let Some((s, e)) = sel {
            if i >= s.0 && i <= e.0 {
                let start_col = if i == s.0 { s.1 } else { 0 };
                let end_col = if i == e.0 { e.1 } else { line_char_count(line) };
                let x0 = x_off + start_col as f32 * char_width;
                let x1 = x_off + (end_col as f32 * char_width).max(start_col as f32 * char_width + 4.0);
                let sel_rect = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + row_height));
                painter.rect_filled(sel_rect, 0.0, sel_color.gamma_multiply(0.5));
            }
        }

        let galley = ui.fonts(|f| f.layout_no_wrap(line.clone(), font_id.clone(), text_color));
        painter.galley(egui::pos2(x_off, y), galley, text_color);

        if has_focus && i == state.cursor_line {
            let caret_x = x_off + state.cursor_col as f32 * char_width;
            painter.line_segment(
                [egui::pos2(caret_x, y), egui::pos2(caret_x, y + row_height)],
                egui::Stroke::new(1.5, egui::Color32::WHITE),
            );
        }
    }

    EditorResponse { changed, lost_focus }
}