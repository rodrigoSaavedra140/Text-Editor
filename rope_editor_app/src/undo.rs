use crate::rope::Rope;

#[derive(Clone, Copy, Debug)]
pub enum EditKind {
    Insert,
    Delete,
    Replace,
}

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
