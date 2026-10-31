//! Tree-sitter semantic AST engine for incremental parsing, code folding, and semantic tokens.

use crate::buffer::TextBuffer;

/// Languages supported by the Tree-sitter AST engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstLanguage {
    Rust,
}

impl AstLanguage {
    pub fn tree_sitter_language(&self) -> tree_sitter::Language {
        match self {
            AstLanguage::Rust => tree_sitter_rust::language(),
        }
    }
}

/// Semantic classifications for syntax tokens derived from the AST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticTokenType {
    Keyword,
    FunctionDef,
    FunctionCall,
    TypeDef,
    TypeRef,
    StringLiteral,
    NumberLiteral,
    Comment,
    Macro,
    Operator,
    Punctuation,
    Variable,
}

/// A semantic highlight span with 0-based `(line, col)` coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstHighlightSpan {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub token_type: SemanticTokenType,
}

/// Metadata about an AST node at a given buffer position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstNodeInfo {
    pub kind: String,
    pub start_position: (usize, usize),
    pub end_position: (usize, usize),
    pub is_named: bool,
}

/// A foldable code block range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldingRange {
    pub start_line: usize,
    pub end_line: usize,
    pub kind: FoldingKind,
}

/// Structural categories for code folding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldingKind {
    Function,
    Type,
    Impl,
    Block,
    Comment,
}

/// F2 (Roadmap v0.2.28): encabezado de un bloque contenedor, para el sticky
/// scroll que queda fijo arriba del editor mientras se scrollea adentro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeHeader {
    pub start_line: usize,
    pub end_line: usize,
    pub header_text: String,
    pub node_kind: &'static str,
    pub depth: usize,
}

/// F7 (Roadmap v0.2.34): categoría de un símbolo del documento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Method,
    Constant,
}

impl SymbolKind {
    /// Insignia corta para el selector de símbolos.
    pub fn label(&self) -> &'static str {
        match self {
            SymbolKind::Function => "ƒ",
            SymbolKind::Struct => "S",
            SymbolKind::Enum => "E",
            SymbolKind::Trait => "T",
            SymbolKind::Method => "M",
            SymbolKind::Constant => "C",
        }
    }
}

/// F7: símbolo navegable del documento activo (sin LSP: sale del AST).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 0-based.
    pub line: usize,
    pub character: usize,
}

/// Headless AST parsing engine backed by `tree_sitter::Parser`.
pub struct AstEngine {
    parser: tree_sitter::Parser,
    tree: Option<tree_sitter::Tree>,
    language: AstLanguage,
}

impl std::fmt::Debug for AstEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AstEngine")
            .field("language", &self.language)
            .field("has_tree", &self.tree.is_some())
            .finish()
    }
}

impl Clone for AstEngine {
    fn clone(&self) -> Self {
        let mut parser = tree_sitter::Parser::new();
        let _ = parser.set_language(&self.language.tree_sitter_language());
        Self {
            parser,
            tree: self.tree.clone(),
            language: self.language,
        }
    }
}

impl AstEngine {
    /// Creates a new `AstEngine` for the given language.
    pub fn new(lang: AstLanguage) -> Result<Self, String> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&lang.tree_sitter_language())
            .map_err(|e| format!("Failed to set parser language: {:?}", e))?;
        Ok(Self {
            parser,
            tree: None,
            language: lang,
        })
    }

    /// Returns the active language.
    pub fn language(&self) -> AstLanguage {
        self.language
    }

    /// Re-parses full text and updates the internal syntax tree.
    pub fn update_from_text(&mut self, text: &str) {
        self.tree = self.parser.parse(text, None);
    }

    /// Updates the syntax tree from a `TextBuffer`.
    pub fn update_from_buffer(&mut self, buffer: &TextBuffer) {
        let text = buffer.to_string();
        self.update_from_text(&text);
    }

    /// Returns a reference to the active Tree-sitter tree if parsed.
    pub fn tree(&self) -> Option<&tree_sitter::Tree> {
        self.tree.as_ref()
    }

    /// Extracts all multi-line code folding ranges from the AST.
    pub fn folding_ranges(&self) -> Vec<FoldingRange> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let mut ranges = Vec::new();
        let root = tree.root_node();
        self.collect_folding_ranges(root, &mut ranges);
        ranges
    }

    /// F2 (Roadmap v0.2.28): bloques contenedores con su encabezado, en orden
    /// de aparición y con su profundidad de anidamiento. Solo se incluyen los
    /// que ocupan más de una línea: un `struct` de una línea no aporta contexto.
    pub fn sticky_scopes(&self, source: &str) -> Vec<ScopeHeader> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let lines: Vec<&str> = source.lines().collect();
        let mut scopes = Vec::new();
        self.collect_sticky_scopes(tree.root_node(), &lines, 0, &mut scopes);
        scopes
    }

    fn collect_sticky_scopes(
        &self,
        node: tree_sitter::Node<'_>,
        lines: &[&str],
        depth: usize,
        out: &mut Vec<ScopeHeader>,
    ) {
        let start_line = node.start_position().row;
        let end_line = node.end_position().row;
        let label = sticky_kind_label(node.kind());
        let next_depth = match (label, end_line > start_line) {
            (Some(kind), true) => {
                let header_text = lines
                    .get(start_line)
                    .map(|line| line.trim_end().to_string())
                    .unwrap_or_default();
                out.push(ScopeHeader {
                    start_line,
                    end_line,
                    header_text,
                    node_kind: kind,
                    depth,
                });
                depth + 1
            }
            _ => depth,
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_sticky_scopes(child, lines, next_depth, out);
        }
    }

    /// F7 (Roadmap v0.2.34): símbolos navegables del documento, en orden de
    /// aparición. Las funciones dentro de un `impl`/`trait` se marcan como
    /// método. Sale 100% del AST, sin LSP.
    pub fn extract_document_symbols(&self, source: &str) -> Vec<DocumentSymbol> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let mut out = Vec::new();
        self.collect_symbols(tree.root_node(), source, false, &mut out);
        out
    }

    fn collect_symbols(
        &self,
        node: tree_sitter::Node<'_>,
        source: &str,
        in_type_body: bool,
        out: &mut Vec<DocumentSymbol>,
    ) {
        let kind = match node.kind() {
            "function_item" | "function_definition" => Some(if in_type_body {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            }),
            "struct_item" | "class_definition" => Some(SymbolKind::Struct),
            "enum_item" => Some(SymbolKind::Enum),
            "trait_item" => Some(SymbolKind::Trait),
            "const_item" | "static_item" => Some(SymbolKind::Constant),
            _ => None,
        };

        if let Some(kind) = kind {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Some(name) = source.get(name_node.byte_range()) {
                    let pos = name_node.start_position();
                    out.push(DocumentSymbol {
                        name: name.to_string(),
                        kind,
                        line: pos.row,
                        character: pos.column,
                    });
                }
            }
        }

        let children_in_type = in_type_body || matches!(node.kind(), "impl_item" | "trait_item");
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_symbols(child, source, children_in_type, out);
        }
    }

    fn collect_folding_ranges(&self, node: tree_sitter::Node<'_>, out: &mut Vec<FoldingRange>) {
        let start_line = node.start_position().row;
        let end_line = node.end_position().row;

        if end_line > start_line {
            let kind = match node.kind() {
                "function_item" | "function_signature_item" => Some(FoldingKind::Function),
                "struct_item" | "enum_item" | "trait_item" => Some(FoldingKind::Type),
                "impl_item" => Some(FoldingKind::Impl),
                "block" => Some(FoldingKind::Block),
                "line_comment" | "block_comment" => Some(FoldingKind::Comment),
                _ => None,
            };

            if let Some(k) = kind {
                out.push(FoldingRange {
                    start_line,
                    end_line,
                    kind: k,
                });
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_folding_ranges(child, out);
        }
    }

    /// Computes semantic highlight tokens across the syntax tree.
    pub fn highlight_spans(&self) -> Vec<AstHighlightSpan> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        let root = tree.root_node();
        self.collect_highlight_spans(root, &mut spans);
        spans
    }

    fn collect_highlight_spans(
        &self,
        node: tree_sitter::Node<'_>,
        out: &mut Vec<AstHighlightSpan>,
    ) {
        if let Some(token_type) = classify_node(node) {
            out.push(AstHighlightSpan {
                start_line: node.start_position().row,
                start_col: node.start_position().column,
                end_line: node.end_position().row,
                end_col: node.end_position().column,
                token_type,
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_highlight_spans(child, out);
        }
    }

    /// Queries the innermost AST node at a given `(line, col)` position.
    pub fn node_at_point(&self, line: usize, col: usize) -> Option<AstNodeInfo> {
        let tree = self.tree.as_ref()?;
        let point = tree_sitter::Point {
            row: line,
            column: col,
        };
        let node = tree
            .root_node()
            .descendant_for_point_range(point, point)?;
        Some(AstNodeInfo {
            kind: node.kind().to_string(),
            start_position: (node.start_position().row, node.start_position().column),
            end_position: (node.end_position().row, node.end_position().column),
            is_named: node.is_named(),
        })
    }
}

/// Classifies a Tree-sitter AST node into a high-level semantic token.
fn classify_node(node: tree_sitter::Node<'_>) -> Option<SemanticTokenType> {
    let kind = node.kind();
    match kind {
        "fn" | "let" | "mut" | "pub" | "use" | "mod" | "struct" | "enum" | "impl" | "trait"
        | "for" | "while" | "loop" | "if" | "else" | "match" | "return" | "break"
        | "continue" | "as" | "const" | "static" | "type" | "unsafe" | "where" | "async"
        | "await" | "crate" | "super" | "self" | "Self" => Some(SemanticTokenType::Keyword),

        "identifier" => {
            if let Some(parent) = node.parent() {
                match parent.kind() {
                    "function_item" => {
                        if parent.child_by_field_name("name") == Some(node) {
                            return Some(SemanticTokenType::FunctionDef);
                        }
                    }
                    "call_expression" => {
                        if parent.child_by_field_name("function") == Some(node) {
                            return Some(SemanticTokenType::FunctionCall);
                        }
                    }
                    "struct_item" | "enum_item" | "trait_item" => {
                        if parent.child_by_field_name("name") == Some(node) {
                            return Some(SemanticTokenType::TypeDef);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        "type_identifier" | "primitive_type" => Some(SemanticTokenType::TypeRef),
        "string_literal" | "raw_string_literal" | "char_literal" => {
            Some(SemanticTokenType::StringLiteral)
        }
        "integer_literal" | "float_literal" | "boolean_literal" => {
            Some(SemanticTokenType::NumberLiteral)
        }
        "line_comment" | "block_comment" => Some(SemanticTokenType::Comment),
        "macro_invocation" => Some(SemanticTokenType::Macro),
        _ => None,
    }
}

/// F2 (Roadmap v0.2.28): categoría del nodo contenedor para el sticky scroll.
fn sticky_kind_label(kind: &str) -> Option<&'static str> {
    match kind {
        "function_item" | "function_definition" => Some("function"),
        "impl_item" => Some("impl"),
        "struct_item" => Some("struct"),
        "enum_item" => Some("enum"),
        "mod_item" => Some("module"),
        "trait_item" => Some("trait"),
        "class_definition" => Some("class"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {    use super::*;

    #[test]
    fn test_ast_engine_parse_and_query_node() {
        let mut engine = AstEngine::new(AstLanguage::Rust).expect("Rust engine should initialize");
        let code = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}";
        engine.update_from_text(code);

        assert!(engine.tree().is_some());

        // Point at (0, 3) is over "add" (function_item name identifier)
        let node = engine.node_at_point(0, 4).expect("should find node");
        assert_eq!(node.kind, "identifier");
    }

    #[test]
    fn test_ast_engine_folding_ranges() {
        let mut engine = AstEngine::new(AstLanguage::Rust).expect("Rust engine should initialize");
        let code = r#"
fn compute() {
    let x = 10;
    let y = 20;
}

struct Point {
    x: i32,
    y: i32,
}
"#;
        engine.update_from_text(code);
        let foldings = engine.folding_ranges();
        assert!(!foldings.is_empty());

        // Function spans lines 1 to 4
        let has_func = foldings.iter().any(|f| f.kind == FoldingKind::Function && f.start_line == 1 && f.end_line == 4);
        assert!(has_func, "should find folding for function");

        // Struct spans lines 6 to 9
        let has_struct = foldings.iter().any(|f| f.kind == FoldingKind::Type && f.start_line == 6 && f.end_line == 9);
        assert!(has_struct, "should find folding for struct");
    }

    #[test]
    fn test_ast_engine_semantic_highlights() {
        let mut engine = AstEngine::new(AstLanguage::Rust).expect("Rust engine should initialize");
        let code = "fn calculate() {\n    // A comment\n    let msg = \"hello\";\n}";
        engine.update_from_text(code);

        let spans = engine.highlight_spans();
        assert!(!spans.is_empty());

        let has_keyword_fn = spans.iter().any(|s| s.token_type == SemanticTokenType::Keyword && s.start_line == 0);
        assert!(has_keyword_fn);

        let has_func_def = spans.iter().any(|s| s.token_type == SemanticTokenType::FunctionDef);
        assert!(has_func_def);

        let has_comment = spans.iter().any(|s| s.token_type == SemanticTokenType::Comment && s.start_line == 1);
        assert!(has_comment);

        let has_str = spans.iter().any(|s| s.token_type == SemanticTokenType::StringLiteral && s.start_line == 2);
        assert!(has_str);
    }

    // F2 (Roadmap v0.2.28): encabezados del sticky scroll.

    #[test]
    fn sticky_scopes_list_nested_containers_with_depth() {
        let source = "use std::fmt;\n\nimpl Foo {\n    fn bar(&self) {\n        let x = 1;\n    }\n}\n";
        let mut engine = AstEngine::new(AstLanguage::Rust).expect("engine rust");
        engine.update_from_text(source);

        let scopes = engine.sticky_scopes(source);
        let kinds: Vec<(&str, usize)> = scopes.iter().map(|s| (s.node_kind, s.depth)).collect();
        assert!(kinds.contains(&("impl", 0)), "impl en el nivel raíz: {kinds:?}");
        assert!(kinds.contains(&("function", 1)), "fn anidada un nivel: {kinds:?}");

        let imp = scopes.iter().find(|s| s.node_kind == "impl").unwrap();
        assert_eq!(imp.start_line, 2);
        assert_eq!(imp.end_line, 6);
        assert_eq!(imp.header_text, "impl Foo {");

        let fun = scopes.iter().find(|s| s.node_kind == "function").unwrap();
        assert_eq!(fun.start_line, 3);
        assert_eq!(fun.header_text.trim(), "fn bar(&self) {");
    }

    #[test]
    fn sticky_scopes_skip_single_line_items() {
        let source = "struct Point { x: i32 }\n";
        let mut engine = AstEngine::new(AstLanguage::Rust).expect("engine rust");
        engine.update_from_text(source);
        assert!(
            engine.sticky_scopes(source).is_empty(),
            "un struct en una sola línea no aporta contexto pegajoso"
        );
    }

    #[test]
    fn sticky_scopes_are_empty_without_a_tree() {
        let engine = AstEngine::new(AstLanguage::Rust).expect("engine rust");
        assert!(engine.sticky_scopes("fn main() {}\n").is_empty());
    }

    // F7 (Roadmap v0.2.34): símbolos del documento.

    #[test]
    fn document_symbols_cover_functions_methods_and_types() {
        let source = concat!(
            "const MAX: usize = 3;\n",
            "struct Point { x: i32 }\n",
            "enum Color { Red }\n",
            "trait Draw {}\n",
            "impl Point {\n",
            "    fn draw(&self) {}\n",
            "}\n",
            "fn main() {}\n",
        );
        let mut engine = AstEngine::new(AstLanguage::Rust).expect("engine rust");
        engine.update_from_text(source);

        let symbols = engine.extract_document_symbols(source);
        let find = |name: &str| symbols.iter().find(|s| s.name == name).cloned();
        assert_eq!(find("MAX").map(|s| s.kind), Some(SymbolKind::Constant));
        assert_eq!(find("Point").map(|s| s.kind), Some(SymbolKind::Struct));
        assert_eq!(find("Color").map(|s| s.kind), Some(SymbolKind::Enum));
        assert_eq!(find("Draw").map(|s| s.kind), Some(SymbolKind::Trait));
        assert_eq!(find("draw").map(|s| s.kind), Some(SymbolKind::Method));
        assert_eq!(find("main").map(|s| s.kind), Some(SymbolKind::Function));
        assert_eq!(find("main").map(|s| s.line), Some(7));
    }

    #[test]
    fn document_symbols_are_empty_without_a_tree() {
        let engine = AstEngine::new(AstLanguage::Rust).expect("engine rust");
        assert!(engine.extract_document_symbols("fn main() {}\n").is_empty());
    }
}
