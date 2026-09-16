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
