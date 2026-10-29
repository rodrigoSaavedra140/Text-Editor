// Undo del GUI: el widget edita un Vec<String> en vivo, así que cada
// snapshot del historial guarda el documento completo como String
// (antes / después) por sesión de tipeo. Los snapshots de
// documentos completos son pesados; con un tope acotamos el peor
// caso en archivos gigantes en vez de crecer sin límite.

/// Tope del historial: cada Edit guarda el documento completo dos
/// veces (antes/después), así que sin tope un archivo grande
/// multiplicaría la memoria por cada sesión de tipeo sin límite.
const MAX_UNDO_EDITS: usize = 200;

/// Un paso de undo/redo: contenido completo del documento antes y
/// después del snapshot.
pub struct Edit {
    pub before: String,
    pub after: String,
}

pub struct UndoStack {
    stack: Vec<Edit>,
    redo_stack: Vec<Edit>,
}

impl UndoStack {
    pub fn new() -> Self {
        UndoStack { stack: Vec::new(), redo_stack: Vec::new() }
    }

    pub fn push(&mut self, edit: Edit) {
        self.stack.push(edit);
        // Descarta los más viejos del frente al pasar del tope.
        let overflow = self.stack.len().saturating_sub(MAX_UNDO_EDITS);
        if overflow > 0 {
            self.stack.drain(0..overflow);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> Option<Edit> {
        // Clonamos igual que antes (misma orientación before/after):
        // el clon va al redo para poder rehacer el mismo paso.
        let edit = self.stack.pop()?;
        self.redo_stack.push(Edit {
            before: edit.before.clone(),
            after: edit.after.clone(),
        });
        Some(edit)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        let edit = self.redo_stack.pop()?;
        self.stack.push(Edit {
            before: edit.before.clone(),
            after: edit.after.clone(),
        });
        Some(edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(n: usize) -> Edit {
        Edit {
            before: format!("antes-{}", n),
            after: format!("despues-{}", n),
        }
    }

    #[test]
    fn el_tope_descarta_el_mas_viejo() {
        let mut stack = UndoStack::new();
        for i in 0..(MAX_UNDO_EDITS + 10) {
            stack.push(edit(i));
        }
        let mut undone = Vec::new();
        while let Some(e) = stack.undo() {
            undone.push(e.before);
        }
        // Solo quedan los últimos MAX_UNDO_EDITS, en orden de más
        // reciente a más viejo, y se descartaron los 10 primeros.
        assert_eq!(undone.len(), MAX_UNDO_EDITS);
        assert_eq!(undone[0], format!("antes-{}", MAX_UNDO_EDITS + 9));
        assert_eq!(undone.last(), Some(&format!("antes-10")));
    }

    #[test]
    fn push_limpia_redo() {
        let mut stack = UndoStack::new();
        stack.push(edit(0));
        stack.undo();
        assert!(!stack.redo_stack.is_empty());
        stack.push(edit(1));
        assert!(stack.redo_stack.is_empty());
    }

    #[test]
    fn undo_y_redo_recuperan_el_mismo_paso() {
        let mut stack = UndoStack::new();
        stack.push(edit(7));
        let undone = stack.undo().expect("debería haber algo para deshacer");
        assert_eq!(undone.before, "antes-7");
        let redone = stack.redo().expect("debería haber algo para rehacer");
        assert_eq!(redone.after, "despues-7");
        // La pila de undo vuelve a quedar con el paso para deshacerlo
        // de nuevo (round-trip).
        assert!(stack.undo().is_some());
    }
}
