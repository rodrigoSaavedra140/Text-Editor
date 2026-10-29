use rope_editor::rope::Rope;

#[test]
fn concat_basico() {
    let a = Rope::new("Hola, ");
    let b = Rope::new("mundo!");
    let c = a.concat(&b);
    assert_eq!(c.to_string(), "Hola, mundo!");
    assert_eq!(c.len(), 12);
}

#[test]
fn split_preserva_contenido() {
    let r = Rope::new("Hola, mundo!");
    let (left, right) = r.split_at(6);
    assert_eq!(left.to_string(), "Hola, ");
    assert_eq!(right.to_string(), "mundo!");
}

#[test]
fn insert_no_muta_original() {
    let original = Rope::new("Hola mundo");
    let editado = original.insert(4, " lindo");
    assert_eq!(original.to_string(), "Hola mundo");
    assert_eq!(editado.to_string(), "Hola lindo mundo");
}

#[test]
fn delete_funciona() {
    let r = Rope::new("Hola lindo mundo");
    let sin_lindo = r.delete(4, 10);
    assert_eq!(sin_lindo.to_string(), "Hola mundo");
}

#[test]
fn rebalance_preserva_contenido() {
    let mut r = Rope::new("");
    for palabra in ["uno ", "dos ", "tres ", "cuatro"] {
        r = r.insert(r.len(), palabra);
    }
    let balanceado = r.rebalance();
    assert_eq!(r.to_string(), balanceado.to_string());
}

#[test]
fn char_at_funciona() {
    let r = Rope::new("Hola");
    assert_eq!(r.char_at(0), Some('H'));
    assert_eq!(r.char_at(3), Some('a'));
    assert_eq!(r.char_at(4), None);
}

// =====================================================================
// line / line_count / lines_window — lectura de líneas con UN solo
// recorrido del árbol (antes: cada Buffer::line() alocaba el texto
// entero con to_string; el renderer pedía ~30 líneas por redraw, o sea
// 30 textos completos por tecla).
// =====================================================================

#[test]
fn line_count_y_line_sobre_un_hoja() {
    let r = Rope::new("uno\ndos\ntres\n");
    // Semántica idéntica al viejo text.lines().count(): el "\n" final
    // no agrega una línea vacía extra.
    assert_eq!(r.line_count(), 3);
    assert_eq!(r.line(0), "uno");
    assert_eq!(r.line(1), "dos");
    assert_eq!(r.line(2), "tres");
    assert_eq!(r.line(3), ""); // fuera de rango: vacía, no pánico
}

#[test]
fn line_count_sin_trailing_newline() {
    assert_eq!(Rope::new("").line_count(), 1);
    assert_eq!(Rope::new("a").line_count(), 1);
    assert_eq!(Rope::new("a\nb").line_count(), 2);
    assert_eq!(Rope::new("a\n\n").line_count(), 2);
}

#[test]
fn line_sobre_lineas_vacias() {
    let r = Rope::new("\n\nx\n");
    assert_eq!(r.line_count(), 3);
    assert_eq!(r.line(0), "");
    assert_eq!(r.line(1), "");
    assert_eq!(r.line(2), "x");
    assert_eq!(r.line(3), "");
}

#[test]
fn line_y_line_count_con_arbol_multi_hoja() {
    // Fuerza varios hojas concatendolas a mano: la línea 0 termina en
    // el borde entre dos hojas, y las continuaciones tienen que
    // mantenerse intactas a lo largo.
    let r = Rope::new("ab").concat(&Rope::new("\ncd\nef"));
    assert_eq!(r.line_count(), 3);
    assert_eq!(r.line(0), "ab");
    assert_eq!(r.line(1), "cd");
    assert_eq!(r.line(2), "ef");
}

#[test]
fn lines_window_devuelve_las_lineas_pedidas() {
    let r = Rope::new("uno\ndos\ntres\ncuatro");
    assert_eq!(r.lines_window(0, 2).join("\n"), "uno\ndos");
    assert_eq!(r.lines_window(1, 2).join("\n"), "dos\ntres");
    assert_eq!(r.lines_window(2, 10).join("\n"), "tres\ncuatro");
    assert!(r.lines_window(5, 2).is_empty()); // más allá del final
    assert!(r.lines_window(2, 0).is_empty()); // ventana vacía
}

#[test]
fn lines_window_con_lineas_que_cruzan_hojas() {
    // "ab\ncd" partido justo después de "ab": la línea 0 vive entre
    // dos hojas (termina donde arranca la hoja 2). El recorrido tiene
    // que concatenar bien los trozos.
    let r = Rope::new("ab").concat(&Rope::new("\ncd\n"));
    assert_eq!(r.lines_window(0, 10).len(), 2);
    assert_eq!(r.lines_window(0, 10).first().map(|s| s.as_str()), Some("ab"));
    assert_eq!(r.lines_window(0, 10).get(1).map(|s| s.as_str()), Some("cd"));
    // El mismo texto reconstruido tiene que coincidir.
    assert_eq!(r.to_string(), "ab\ncd\n");
}
