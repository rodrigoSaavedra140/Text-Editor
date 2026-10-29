use crate::rope::Rope;

#[derive(Clone, Copy, Debug)]
pub enum EditKind {
    Insert,
    Delete,
    Replace,
}

/// Tope del historial: cada Edit retiene árboles completos vía Rc
/// (structural sharing — cada edición nueva cuesta ~pocos nodos),
/// pero sin tope el grafo crecería para siempre con sesiones largas.
const MAX_UNDO_EDITS: usize = 200;

pub struct Edit {
    pub kind: EditKind,
    pub position: usize,
    pub before: Rope,
    pub after: Rope,
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
        // Suelta los pasos más viejos al pasar del tope; los árboles
        // que solo ellos retenían se liberan (Rc llega a 0).
        let overflow = self.stack.len().saturating_sub(MAX_UNDO_EDITS);
        if overflow > 0 {
            self.stack.drain(0..overflow);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> Option<Edit> {
        let edit = self.stack.pop()?;
        self.redo_stack.push(Edit {
            kind: edit.kind,
            position: edit.position,
            before: edit.before.clone(),
            after: edit.after.clone(),
        });
        Some(edit)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        let edit = self.redo_stack.pop()?;
        self.stack.push(Edit {
            kind: edit.kind,
            position: edit.position,
            before: edit.before.clone(),
            after: edit.after.clone(),
        });
        Some(edit)
    }
}
