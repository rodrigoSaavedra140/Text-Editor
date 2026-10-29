use rope_editor::rope::Rope;
use rope_editor::undo::{Edit, EditKind, UndoStack};

fn edit(i: usize) -> Edit {
    Edit {
        kind: EditKind::Insert,
        position: i,
        before: Rope::new(&format!("antes-{}", i)),
        after: Rope::new(&format!("despues-{}", i)),
    }
}

#[test]
fn el_tope_descarta_el_mas_viejo() {
    let mut stack = UndoStack::new();
    for i in 0..300 {
        stack.push(edit(i));
    }
    let mut positions = Vec::new();
    while let Some(e) = stack.undo() {
        positions.push(e.position);
    }
    // Solo quedan los últimos 200 (los primeros 100 se pudrieron
    // fuera por el tope del historial).
    assert_eq!(positions.len(), 200);
    assert_eq!(positions[0], 299);
    assert_eq!(positions.last(), Some(&100));
}

/// El push después de un undo invalida el redo: se verifica por el
/// comportamiento público (redo() ya no devuelve nada), porque el
/// stack interno es privado en el contrato de la API.
#[test]
fn push_limpia_redo() {
    let mut stack = UndoStack::new();
    stack.push(edit(0));
    assert!(stack.undo().is_some());
    stack.push(edit(1));
    assert!(stack.redo().is_none());
}
