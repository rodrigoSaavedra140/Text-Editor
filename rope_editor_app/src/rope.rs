use std::rc::Rc;

/// Nodo del árbol: hoja con texto, o nodo interno con dos hijos
/// compartidos vía Rc (structural sharing). `weight` en Internal
/// es la longitud del subárbol izquierdo, en bytes.
#[derive(Clone)]
enum Node {
    Leaf(String),
    Internal {
        weight: usize,
        left: Rc<Node>,
        right: Rc<Node>,
    },
}

impl Node {
    fn new_leaf(text: impl Into<String>) -> Rc<Node> {
        Rc::new(Node::Leaf(text.into()))
    }

    fn new_internal(left: Rc<Node>, right: Rc<Node>) -> Rc<Node> {
        let weight = left.len();
        Rc::new(Node::Internal { weight, left, right })
    }

    fn len(&self) -> usize {
        match self {
            Node::Leaf(s) => s.len(),
            Node::Internal { left, right, .. } => left.len() + right.len(),
        }
    }

    fn collect_string(&self, out: &mut String) {
        match self {
            Node::Leaf(s) => out.push_str(s),
            Node::Internal { left, right, .. } => {
                left.collect_string(out);
                right.collect_string(out);
            }
        }
    }

    fn collect_leaves(node: &Rc<Node>, out: &mut Vec<Rc<Node>>) {
        match node.as_ref() {
            Node::Leaf(_) => out.push(Rc::clone(node)),
            Node::Internal { left, right, .. } => {
                Node::collect_leaves(left, out);
                Node::collect_leaves(right, out);
            }
        }
    }

    /// Reconstruye un árbol balanceado a partir de una lista de hojas.
    fn build_balanced(leaves: &[Rc<Node>]) -> Rc<Node> {
        match leaves.len() {
            0 => Node::new_leaf(""),
            1 => Rc::clone(&leaves[0]),
            n => {
                let mid = n / 2;
                let left = Node::build_balanced(&leaves[..mid]);
                let right = Node::build_balanced(&leaves[mid..]);
                Node::new_internal(left, right)
            }
        }
    }

    /// Divide en (izquierda, derecha) en el offset de byte `index`,
    /// compartiendo (Rc) todo subárbol que no necesita partirse.
    fn split_at(node: &Rc<Node>, index: usize) -> (Rc<Node>, Rc<Node>) {
        match node.as_ref() {
            Node::Leaf(s) => (Node::new_leaf(&s[..index]), Node::new_leaf(&s[index..])),
            Node::Internal { weight, left, right } => {
                if index < *weight {
                    let (ll, lr) = Node::split_at(left, index);
                    (ll, Node::new_internal(lr, Rc::clone(right)))
                } else if index > *weight {
                    let (rl, rr) = Node::split_at(right, index - weight);
                    (Node::new_internal(Rc::clone(left), rl), rr)
                } else {
                    (Rc::clone(left), Rc::clone(right))
                }
            }
        }
    }

    fn char_at(node: &Rc<Node>, index: usize) -> Option<char> {
        match node.as_ref() {
            Node::Leaf(s) => s.get(index..)?.chars().next(),
            Node::Internal { weight, left, right } => {
                if index < *weight {
                    Node::char_at(left, index)
                } else {
                    Node::char_at(right, index - weight)
                }
            }
        }
    }

    fn count_newlines(&self) -> usize {
        match self {
            Node::Leaf(s) => s.bytes().filter(|&b| b == b'\n').count(),
            Node::Internal { left, right, .. } => left.count_newlines() + right.count_newlines(),
        }
    }

    /// Último byte del subárbol (None si todo está vacío). Sirve para
    /// saber si el texto termina en '\n' sin alocar el texto entero.
    fn last_byte(&self) -> Option<u8> {
        match self {
            Node::Leaf(s) => s.bytes().last(),
            Node::Internal { left, right, .. } => right.last_byte().or_else(|| left.last_byte()),
        }
    }

    fn collect_window(node: &Rc<Node>, collector: &mut WindowCollector) {
        if collector.done {
            return;
        }
        match node.as_ref() {
            Node::Leaf(s) => collector.feed(s),
            Node::Internal { left, right, .. } => {
                Node::collect_window(left, collector);
                if !collector.done {
                    Node::collect_window(right, collector);
                }
            }
        }
    }
}

/// Recolecta UNA ventana de líneas en un solo recorrido en orden de
/// las hojas, sin alocar el texto entero. `cur` acumula la línea que
/// a la fecha siga abierta: puede continuar a lo largo de varias
/// hojas (por eso los recortes van en '\n', que es seguro en UTF-8).
struct WindowCollector {
    first: usize, // primera línea del rango (0-based)
    end: usize,   // first + count: primera línea fuera del rango
    nl_seen: usize,
    lines: Vec<String>,
    cur: String,
    done: bool,
}

impl WindowCollector {
    fn new(first: usize, count: usize) -> Self {
        WindowCollector {
            first,
            end: first.saturating_add(count),
            nl_seen: 0,
            lines: Vec::new(),
            cur: String::new(),
            done: false,
        }
    }

    fn feed(&mut self, s: &str) {
        if self.done {
            return;
        }
        let mut chunk_start = 0usize;
        for (i, &b) in s.as_bytes().iter().enumerate() {
            if b != b'\n' {
                continue;
            }
            // La línea de índice nl_seen termina en este recorte.
            if self.nl_seen >= self.first && self.nl_seen < self.end {
                self.cur.push_str(&s[chunk_start..i]);
                self.lines.push(std::mem::take(&mut self.cur));
            }
            self.nl_seen += 1;
            chunk_start = i + 1;
            if self.nl_seen >= self.end {
                self.done = true;
                return;
            }
        }
        // Trozo final de la hoja: alimenta la línea todavía abierta.
        if self.nl_seen >= self.first {
            self.cur.push_str(&s[chunk_start..]);
        }
    }

    /// Última línea del texto, si puede ser empujada en el rango
    /// (si termina sin '\n' nunca se procesa en el bucle). Solo se
    /// empuja si tiene contenido: una última línea vacía sin '\n'
    /// no existe (el texto habría terminado en '\n', y lines() no
    /// la cuenta).
    fn finish(&mut self) {
        if !self.done && !self.cur.is_empty() {
            self.lines.push(std::mem::take(&mut self.cur));
        }
    }
}

/// Facade persistente: insert/delete/concat/split_at devuelven un
/// Rope NUEVO y comparten (Rc) los subárboles no modificados. El
/// Rope original queda intacto — clave para que undo/redo sea barato.
#[derive(Clone)]
pub struct Rope {
    root: Rc<Node>,
}

impl Rope {
    pub fn new(text: &str) -> Rope {
        Rope { root: Node::new_leaf(text) }
    }

    fn from_node(root: Rc<Node>) -> Rope {
        Rope { root }
    }

    pub fn len(&self) -> usize {
        self.root.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn concat(&self, other: &Rope) -> Rope {
        Rope::from_node(Node::new_internal(Rc::clone(&self.root), Rc::clone(&other.root)))
    }

    pub fn split_at(&self, index: usize) -> (Rope, Rope) {
        assert!(index <= self.len(), "index fuera de rango");
        let (l, r) = Node::split_at(&self.root, index);
        (Rope::from_node(l), Rope::from_node(r))
    }

    pub fn insert(&self, pos: usize, text: &str) -> Rope {
        let (left, right) = self.split_at(pos);
        left.concat(&Rope::new(text)).concat(&right)
    }

    pub fn delete(&self, start: usize, end: usize) -> Rope {
        assert!(start <= end && end <= self.len(), "rango inválido");
        let (left, rest) = self.split_at(start);
        let (_, right) = rest.split_at(end - start);
        left.concat(&right)
    }

    pub fn char_at(&self, index: usize) -> Option<char> {
        if index >= self.len() {
            return None;
        }
        Node::char_at(&self.root, index)
    }

    /// Línea `target` (0-based) sin alocar el texto entero: es
    /// lines_window(target, 1) — un solo recorrido del árbol.
    /// Fuera de rango devuelve String::new(), igual que el viejo
    /// to_string().lines().nth(target).unwrap_or("").
    pub fn line(&self, target: usize) -> String {
        self.lines_window(target, 1)
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    /// Cantidad de líneas con la MISMA semántica que el viejo
    /// to_string().lines().count() (el '\n' final no agrega una línea
    /// vacía extra), pero sin alocar ni recorrer dos veces.
    pub fn line_count(&self) -> usize {
        if self.len() == 0 {
            return 1;
        }
        let newlines = self.root.count_newlines();
        if self.root.last_byte() == Some(b'\n') {
            newlines
        } else {
            newlines + 1
        }
    }

    /// Devuelve las líneas [first .. first+count) en UN solo recorrido
    /// del árbol, cortando el recorrido apenas completa la ventana.
    /// Es lo que el renderer pide en cada redibujado: N líneas visibles
    /// sin materializar el documento entero.
    pub fn lines_window(&self, first: usize, count: usize) -> Vec<String> {
        let mut collector = WindowCollector::new(first, count);
        Node::collect_window(&self.root, &mut collector);
        collector.finish();
        collector.lines
    }

    /// Reconstruye el árbol balanceado (útil tras muchas ediciones
    /// pequeñas, que pueden degenerar la forma del árbol).
    pub fn rebalance(&self) -> Rope {
        let mut leaves = Vec::new();
        Node::collect_leaves(&self.root, &mut leaves);
        Rope::from_node(Node::build_balanced(&leaves))
    }

    /// Número de línea (0-based) en la que cae el offset de byte `index`.
    /// Implementación simple: cuenta '\n' antes de index.
    pub fn line_at(&self, index: usize) -> usize {
        let text = self.to_string();
        let cut = index.min(text.len());
        text[..cut].matches('\n').count()
    }

    pub fn cursor_at(&self, offset: usize) -> Cursor {
        Cursor::new(offset, self.len())
    }
}

impl ToString for Rope {
    fn to_string(&self) -> String {
        let mut s = String::with_capacity(self.len());
        self.root.collect_string(&mut s);
        s
    }
}

pub struct Cursor {
    offset: usize,
    max: usize,
}

impl Cursor {
    fn new(offset: usize, max: usize) -> Self {
        Cursor { offset: offset.min(max), max }
    }

    pub fn move_left(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
    }

    pub fn move_right(&mut self, n: usize) {
        self.offset = (self.offset + n).min(self.max);
    }

    pub fn position(&self) -> usize {
        self.offset
    }
}
