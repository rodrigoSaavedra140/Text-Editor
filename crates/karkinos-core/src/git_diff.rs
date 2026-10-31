//! F1 (Roadmap v0.2.27): diff por líneas contra `HEAD` para las marcas del gutter.
//!
//! Compara el texto actual con el que tiene Git en `HEAD` y devuelve, por cada
//! línea lógica del buffer actual, si es nueva, modificada o si arriba de ella
//! se borraron líneas. Usa el algoritmo de Myers (O(ND)) sobre hashes de línea:
//! no guarda la matriz completa, así que un archivo grande no explota en memoria.

use std::collections::HashMap;

/// Estado de una línea del buffer frente a `HEAD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    /// Línea que no existe en `HEAD`.
    Added,
    /// Línea que existe en `HEAD` pero con otro contenido.
    Modified,
    /// Se borraron líneas justo debajo de esta (se marca la anterior).
    DeletedBelow,
}

/// Marcas de diff de un archivo, indexadas por línea lógica (0-based) del
/// buffer actual.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct FileDiffHunks {
    pub line_status: HashMap<usize, DiffStatus>,
}

impl FileDiffHunks {
    /// Computa los estados de línea comparando el buffer actual con el
    /// contenido de `HEAD`. Nunca falla: si el diff excede el presupuesto de
    /// trabajo, marca el tramo cambiado como modificado en bloque.
    pub fn compute(head_text: &str, current_text: &str) -> Self {
        let head: Vec<&str> = head_text.lines().collect();
        let current: Vec<&str> = current_text.lines().collect();
        if head == current {
            return Self::default();
        }

        let head_hashes: Vec<u64> = head.iter().map(|l| fnv1a(l.as_bytes())).collect();
        let current_hashes: Vec<u64> = current.iter().map(|l| fnv1a(l.as_bytes())).collect();

        let ops = match myers_ops(&head_hashes, &current_hashes, MAX_EDIT_DISTANCE) {
            Some(ops) => ops,
            None => return Self::fallback(&head, &current),
        };

        let mut line_status = HashMap::new();
        let mut cur = 0usize;
        let mut idx = 0usize;
        while idx < ops.len() {
            match ops[idx] {
                Op::Equal => {
                    cur += 1;
                    idx += 1;
                }
                _ => {
                    // Bloque contiguo de cambios: borrados y agregados seguidos.
                    let mut deletes = 0usize;
                    let mut inserts = 0usize;
                    while idx < ops.len() && ops[idx] != Op::Equal {
                        match ops[idx] {
                            Op::Delete => deletes += 1,
                            Op::Insert => inserts += 1,
                            Op::Equal => unreachable!(),
                        }
                        idx += 1;
                    }
                    // Un borrado + un agregado en el mismo punto es una
                    // modificación, no una línea nueva.
                    let paired = deletes.min(inserts);
                    for _ in 0..paired {
                        line_status.insert(cur, DiffStatus::Modified);
                        cur += 1;
                    }
                    for _ in paired..inserts {
                        line_status.insert(cur, DiffStatus::Added);
                        cur += 1;
                    }
                    if deletes > paired {
                        // No queda línea propia donde marcar: se marca la de
                        // arriba del punto de borrado (o la primera del archivo
                        // si el borrado fue al principio). Con el buffer vacío no
                        // hay ninguna línea que pintar.
                        let mark = cur.saturating_sub(1);
                        if mark < current.len() {
                            line_status.entry(mark).or_insert(DiffStatus::DeletedBelow);
                        }
                    }
                }
            }
        }

        Self { line_status }
    }

    /// Sin marcas (archivo igual a `HEAD` o sin información).
    pub fn is_empty(&self) -> bool {
        self.line_status.is_empty()
    }

    /// Estado de una línea, si tiene alguna marca.
    pub fn status_of(&self, line: usize) -> Option<DiffStatus> {
        self.line_status.get(&line).copied()
    }

    /// Fallback cuando el diff es demasiado caro: todo el tramo que difiere
    /// entre el prefijo y el sufijo comunes se marca como modificado.
    fn fallback(head: &[&str], current: &[&str]) -> Self {
        let mut prefix = 0usize;
        while prefix < head.len() && prefix < current.len() && head[prefix] == current[prefix] {
            prefix += 1;
        }
        let mut suffix = 0usize;
        while suffix < head.len() - prefix
            && suffix < current.len() - prefix
            && head[head.len() - 1 - suffix] == current[current.len() - 1 - suffix]
        {
            suffix += 1;
        }

        let current_end = current.len() - suffix;
        let head_middle = head.len() - prefix - suffix;
        let mut line_status = HashMap::new();
        for line in prefix..current_end {
            line_status.insert(line, DiffStatus::Modified);
        }
        if head_middle > current_end - prefix {
            let mark = current_end.saturating_sub(1);
            if mark < current.len() {
                line_status
                    .entry(mark)
                    .or_insert(DiffStatus::DeletedBelow);
            }
        }
        Self { line_status }
    }
}

/// Distancia de edición máxima antes de caer al fallback. Myers es O(ND):
/// con archivos muy distintos (o reescritos enteros) el costo se dispara.
const MAX_EDIT_DISTANCE: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Insert,
}

/// Diff de Myers sobre hashes de línea. Devuelve `None` si la distancia de
/// edición supera `max_d`.
fn myers_ops(a: &[u64], b: &[u64], max_d: usize) -> Option<Vec<Op>> {
    let n = a.len() as isize;
    let m = b.len() as isize;
    if n == 0 && m == 0 {
        return Some(Vec::new());
    }

    let max = (n + m).min(max_d as isize);
    let offset = max;
    let width = (2 * max + 1) as usize;
    let mut v = vec![0isize; width];
    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=max {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let idx = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                return Some(backtrack(&trace, n, m, offset));
            }
            k += 2;
        }
    }

    None
}

/// Reconstruye el camino óptimo desde la traza de `V` guardada por `d`.
fn backtrack(trace: &[Vec<isize>], n: isize, m: isize, offset: isize) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut x = n;
    let mut y = m;

    for d in (0..trace.len()).rev() {
        let v = &trace[d];
        let d_i = d as isize;
        let k = x - y;
        let prev_k = if k == -d_i || (k != d_i && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            ops.push(Op::Equal);
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            if x == prev_x {
                ops.push(Op::Insert);
            } else {
                ops.push(Op::Delete);
            }
        }
        x = prev_x;
        y = prev_y;
    }

    ops.reverse();
    ops
}

/// FNV-1a de 64 bits: barato y suficiente para comparar líneas.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(current: &str, head: &str) -> FileDiffHunks {
        FileDiffHunks::compute(head, current)
    }

    #[test]
    fn identical_content_has_no_marks() {
        let diff = marks("fn main() {}\nlet a = 1;\n", "fn main() {}\nlet a = 1;\n");
        assert!(diff.is_empty());
    }

    #[test]
    fn inserted_line_is_added() {
        let diff = marks("a\nb\nc\n", "a\nc\n");
        assert_eq!(diff.status_of(1), Some(DiffStatus::Added));
        assert_eq!(diff.line_status.len(), 1);
    }

    #[test]
    fn changed_line_is_modified() {
        let diff = marks("a\nB\nc\n", "a\nb\nc\n");
        assert_eq!(diff.status_of(1), Some(DiffStatus::Modified));
        assert_eq!(diff.line_status.len(), 1);
    }

    #[test]
    fn deleted_line_marks_the_line_above() {
        let diff = marks("a\nc\n", "a\nb\nc\n");
        assert_eq!(diff.status_of(0), Some(DiffStatus::DeletedBelow));
        assert_eq!(diff.line_status.len(), 1);
    }

    #[test]
    fn multiple_hunks_are_independent() {
        let current = "a\nNEW\nc\nD\n";
        let head = "a\nb\nc\nd\n";
        let diff = marks(current, head);
        assert_eq!(diff.status_of(1), Some(DiffStatus::Modified));
        assert_eq!(diff.status_of(3), Some(DiffStatus::Modified));
        assert_eq!(diff.line_status.len(), 2);
    }

    #[test]
    fn empty_head_marks_everything_added() {
        let diff = marks("a\nb\n", "");
        assert_eq!(diff.status_of(0), Some(DiffStatus::Added));
        assert_eq!(diff.status_of(1), Some(DiffStatus::Added));
    }

    #[test]
    fn empty_current_has_nothing_to_mark() {
        let diff = marks("", "a\nb\n");
        assert!(diff.is_empty(), "sin líneas actuales no hay gutter que marcar");
    }

    #[test]
    fn distant_change_keeps_common_lines_clean() {
        let head = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let current = "1\n2\n3\nX\n5\n6\n7\n8\n";
        let diff = marks(current, head);
        assert_eq!(diff.status_of(3), Some(DiffStatus::Modified));
        assert!(diff.status_of(0).is_none());
        assert!(diff.status_of(7).is_none());
    }
}
