use std::sync::Arc;
use std::time::{Duration, Instant};

// Undo del GUI: el widget edita un Vec<String> en vivo, así que cada
// snapshot del historial guarda el documento completo como String
// (antes / después) por sesión de tipeo. Los snapshots de
// documentos completos son pesados; con un tope acotamos el peor
// caso en archivos gigantes en vez de crecer sin límite.

/// S3: timeout de inactividad para agrupar caracteres tipeados en un
/// mismo chunk de deshacer.
pub const CHUNK_TIMEOUT: Duration = Duration::from_millis(700);

/// S3: tipo de acción de edición para decidir la política de chunking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum EditAction {
    Typing,
    Enter,
    Paste,
    LostFocus,
    Save,
}

/// S3: corte inmediato vs espera de inactividad (~700ms).
pub fn should_snapshot_immediately(action: EditAction) -> bool {
    match action {
        EditAction::Enter | EditAction::Paste | EditAction::LostFocus | EditAction::Save => true,
        EditAction::Typing => false,
    }
}

/// S3: rastreador de chunks de tipeo por tiempo de inactividad o acción estructural.
pub struct ChunkTracker {
    pub last_edit_time: Option<Instant>,
    pub timeout: Duration,
}

impl Default for ChunkTracker {
    fn default() -> Self {
        Self::new(CHUNK_TIMEOUT)
    }
}

impl ChunkTracker {
    pub fn new(timeout: Duration) -> Self {
        Self {
            last_edit_time: None,
            timeout,
        }
    }

    pub fn default_timeout() -> Self {
        Self::default()
    }

    pub fn on_edit(&mut self, action: EditAction, now: Instant) -> bool {
        self.last_edit_time = Some(now);
        should_snapshot_immediately(action)
    }

    pub fn check_idle_timeout(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_edit_time {
            if now.duration_since(last) >= self.timeout {
                self.last_edit_time = None;
                return true;
            }
        }
        false
    }

    pub fn remaining_time(&self, now: Instant) -> Option<Duration> {
        let last = self.last_edit_time?;
        let elapsed = now.duration_since(last);
        Some(self.timeout.saturating_sub(elapsed))
    }

    pub fn reset(&mut self) {
        self.last_edit_time = None;
    }
}

/// Tope del historial: cada Edit guarda el documento completo dos
/// veces (antes/después), así que sin tope un archivo grande
/// multiplicaría la memoria por cada sesión de tipeo sin límite.
const MAX_UNDO_EDITS: usize = 200;

/// Un paso de undo/redo: contenido inmutable compartido vía Arc<str>.
/// Almacenar Arc<str> en lugar de String evita duplicar el documento entero
/// cuando el estado N pasa de ser 'after' del paso anterior a 'before' del
/// siguiente paso, reduciendo a la mitad el consumo de memoria del historial.
#[derive(Clone)]
pub struct Edit {
    pub before: Arc<str>,
    pub after: Arc<str>,
}

pub struct UndoStack {
    pub stack: Vec<Edit>,
    pub redo_stack: Vec<Edit>,
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
        let edit = self.stack.pop()?;
        self.redo_stack.push(Edit {
            before: Arc::clone(&edit.before),
            after: Arc::clone(&edit.after),
        });
        Some(edit)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        let edit = self.redo_stack.pop()?;
        self.stack.push(Edit {
            before: Arc::clone(&edit.before),
            after: Arc::clone(&edit.after),
        });
        Some(edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(n: usize) -> Edit {
        Edit {
            before: Arc::from(format!("antes-{}", n)),
            after: Arc::from(format!("despues-{}", n)),
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
        assert_eq!(&*undone[0], &format!("antes-{}", MAX_UNDO_EDITS + 9));
        assert_eq!(undone.last().map(|s| &**s), Some(format!("antes-10").as_str()));
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
        assert_eq!(&*undone.before, "antes-7");
        let redone = stack.redo().expect("debería haber algo para rehacer");
        assert_eq!(&*redone.after, "despues-7");
        // La pila de undo vuelve a quedar con el paso para deshacerlo
        // de nuevo (round-trip).
        assert!(stack.undo().is_some());
    }

    #[test]
    fn snapshots_consecutivos_comparten_puntero_arc() {
        let mut stack = UndoStack::new();
        let s1: Arc<str> = Arc::from("estado 1");
        let s2: Arc<str> = Arc::from("estado 2");
        stack.push(Edit {
            before: Arc::clone(&s1),
            after: Arc::clone(&s2),
        });
        let s3: Arc<str> = Arc::from("estado 3");
        stack.push(Edit {
            before: Arc::clone(&s2),
            after: Arc::clone(&s3),
        });
        // s2 es 'after' del paso 0 y 'before' del paso 1: deben apuntar exactamente a la misma dirección
        assert!(Arc::ptr_eq(&stack.stack[0].after, &stack.stack[1].before));
    }

    #[test]
    fn chunk_policy_timeout_corta_tras_inactividad() {
        let mut tracker = ChunkTracker::default_timeout();
        let t0 = Instant::now();

        // Tipear un carácter no corta inmediatamente.
        assert!(!tracker.on_edit(EditAction::Typing, t0));

        // Antes de 700ms no corta por inactividad.
        let t_early = t0 + Duration::from_millis(300);
        assert!(!tracker.check_idle_timeout(t_early));
        assert!(tracker.remaining_time(t_early).unwrap() <= Duration::from_millis(400));

        // A los 700ms o más corta el chunk.
        let t_late = t0 + Duration::from_millis(700);
        assert!(tracker.check_idle_timeout(t_late));

        // Un chequeo subsiguiente no repite el corte.
        assert!(!tracker.check_idle_timeout(t_late + Duration::from_millis(10)));
        assert_eq!(tracker.remaining_time(t_late), None);
    }

    #[test]
    fn chunk_policy_enter_y_paste_fuerzan_corte_inmediato() {
        let mut tracker = ChunkTracker::default_timeout();
        let now = Instant::now();

        assert!(tracker.on_edit(EditAction::Enter, now));
        assert!(tracker.on_edit(EditAction::Paste, now));
        assert!(tracker.on_edit(EditAction::LostFocus, now));
        assert!(tracker.on_edit(EditAction::Save, now));
    }

    #[test]
    fn chunk_policy_typing_no_fuerza_corte_inmediato() {
        let mut tracker = ChunkTracker::default_timeout();
        let now = Instant::now();

        assert!(!tracker.on_edit(EditAction::Typing, now));
    }
}
