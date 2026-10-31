//! Resaltado de sintaxis ligero para los lenguajes más usados,
//! renderizado mediante `egui::text::LayoutJob`.
//!
//! Soporta:
//! - Rust (.rs)
//! - C (.c, .h)
//! - C++ (.cpp, .cc, .cxx, .hpp, .hxx)
//! - Java (.java)
//! - JavaScript / TypeScript (.js, .jsx, .ts, .tsx, .mjs, .cjs)
//! - SQL (.sql)
//! - HTML (.html, .htm)
//! - CSS (.css, .scss, .sass, .less)
//! - Markdown (.md, .markdown)
//! - JSON (.json, .jsonc)
//! - Texto plano (fallback)

use eframe::egui;
use std::hash::Hasher;
use std::ops::Range;
use std::path::Path;

/// Lenguajes con reglas de resaltado disponibles en el editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    PlainText,
    Rust,
    Markdown,
    Json,
    C,
    Cpp,
    Java,
    JavaScript,
    Sql,
    Html,
    Css,
}

impl Language {
    /// Detecta el lenguaje a partir de la extensión de la ruta.
    #[allow(dead_code)]
    pub fn from_path(path: &Path) -> Self {
        Self::detect(Some(path), "")
    }

    /// Autodetects language from file path (extensions & filenames) and content heuristics (shebang & syntax signatures).
    pub fn detect(path: Option<&Path>, sample_content: &str) -> Self {
        // 1. Try file extension
        if let Some(p) = path {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            let from_ext = Self::from_extension(ext);
            if from_ext != Language::PlainText {
                return from_ext;
            }

            // 2. Try well-known filenames without extension
            if let Some(name) = p.file_name().and_then(|f| f.to_str()) {
                match name.to_lowercase().as_str() {
                    "readme" | "changelog" | "contributing" | "license" | "todo" => {
                        return Language::Markdown;
                    }
                    _ => {}
                }
            }
        }

        // 3. Fallback: Autodetect from content without extension
        if !sample_content.is_empty() {
            Self::detect_from_content(sample_content)
        } else {
            Language::PlainText
        }
    }

    /// Autodetects language strictly from content (shebang, syntax patterns, keywords).
    pub fn detect_from_content(content: &str) -> Self {
        let lines: Vec<&str> = content.lines().take(100).collect();
        if lines.is_empty() {
            return Language::PlainText;
        }

        // 1. Check Shebang on first line
        if let Some(first) = lines.first() {
            let t = first.trim();
            if t.starts_with("#!") {
                if t.contains("node") || t.contains("deno") || t.contains("bun") {
                    return Language::JavaScript;
                }
                if t.contains("rust-script") || t.contains("cargo") {
                    return Language::Rust;
                }
            }
        }

        let trimmed_content = content.trim();

        // 2. Check JSON (object with "key": or array of items)
        if (trimmed_content.starts_with('{') && trimmed_content.contains("\":"))
            || (trimmed_content.starts_with('[') && trimmed_content.ends_with(']'))
        {
            return Language::Json;
        }

        // 3. Check HTML / XML
        let has_html_doctype = trimmed_content.to_ascii_lowercase().starts_with("<!doctype html");
        let has_html_tags = lines.iter().any(|l| {
            let lower = l.to_ascii_lowercase();
            lower.contains("<html") || lower.contains("</html>") || lower.contains("<body")
        });
        if has_html_doctype || has_html_tags {
            return Language::Html;
        }

        // 4. Check Rust signatures
        let is_rust = lines.iter().any(|line| {
            let l = line.trim();
            l.starts_with("fn ")
                || l.starts_with("pub fn ")
                || l.starts_with("use std::")
                || l.starts_with("pub use ")
                || l.starts_with("impl ")
                || l.starts_with("pub struct ")
                || l.starts_with("pub enum ")
                || l.starts_with("#[derive(")
                || l.starts_with("#[inline]")
                || l.starts_with("#[test]")
                || l.starts_with("#[no_std]")
                || l.starts_with("mod ")
                || l.starts_with("pub mod ")
                || l.starts_with("extern crate ")
                || l.contains("let mut ")
                || l.contains("println!(")
                || l.contains("eprintln!(")
        });
        if is_rust {
            return Language::Rust;
        }

        // 5. Check C / C++ signatures
        let has_include = lines.iter().any(|l| l.trim().starts_with("#include"));
        let has_cpp_syntax = lines.iter().any(|l| {
            let t = l.trim();
            t.contains("std::")
                || t.starts_with("template<")
                || t.starts_with("class ")
                || t.contains("cout <<")
                || t.contains("cin >>")
        });
        if has_include {
            if has_cpp_syntax {
                return Language::Cpp;
            }
            return Language::C;
        }

        // 6. Check Java signatures
        let is_java = lines.iter().any(|l| {
            let t = l.trim();
            t.starts_with("package ")
                || t.starts_with("public class ")
                || t.contains("public static void main")
                || t.contains("System.out.println")
        });
        if is_java {
            return Language::Java;
        }

        // 7. Check JavaScript / TypeScript signatures
        let is_js = lines.iter().any(|l| {
            let t = l.trim();
            (t.starts_with("import ") && (t.contains(" from '") || t.contains(" from \"")))
                || t.starts_with("export default ")
                || t.starts_with("export const ")
                || t.starts_with("export function ")
                || (t.starts_with("const ") && t.contains(" = require("))
                || t.contains("console.log(")
                || t.contains("function* ")
        });
        if is_js {
            return Language::JavaScript;
        }

        // 8. Check SQL signatures
        let is_sql = lines.iter().any(|l| {
            let t = l.trim().to_uppercase();
            (t.starts_with("SELECT ") && t.contains(" FROM "))
                || t.starts_with("CREATE TABLE ")
                || t.starts_with("INSERT INTO ")
                || t.starts_with("DROP TABLE ")
                || t.starts_with("ALTER TABLE ")
        });
        if is_sql {
            return Language::Sql;
        }

        // 9. Check CSS signatures
        let is_css = lines.iter().any(|l| {
            let t = l.trim();
            t.starts_with("@import ")
                || t.starts_with("@media ")
                || (t.contains('{')
                    && (t.contains("margin:")
                        || t.contains("padding:")
                        || t.contains("color:")
                        || t.contains("background:")))
        });
        if is_css {
            return Language::Css;
        }

        // 10. Check Markdown signatures
        let is_md = lines.iter().any(|l| {
            let t = l.trim();
            t.starts_with("# ")
                || t.starts_with("## ")
                || t.starts_with("### ")
                || t.starts_with("- [ ] ")
                || t.starts_with("- [x] ")
        });
        if is_md {
            return Language::Markdown;
        }

        Language::PlainText
    }

    /// Detecta el lenguaje a partir de una extensión textual (ej: "rs", "cpp", "js").
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "md" | "markdown" => Language::Markdown,
            "json" | "jsonc" => Language::Json,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "c++" | "h++" => Language::Cpp,
            "java" => Language::Java,
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Language::JavaScript,
            "sql" => Language::Sql,
            "html" | "htm" => Language::Html,
            "css" | "scss" | "sass" | "less" => Language::Css,
            _ => Language::PlainText,
        }
    }

    /// Nombre legible del lenguaje para la interfaz de usuario.
    pub fn name(&self) -> &'static str {
        match self {
            Language::PlainText => "Plain Text",
            Language::Rust => "Rust",
            Language::Markdown => "Markdown",
            Language::Json => "JSON",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::Java => "Java",
            Language::JavaScript => "JavaScript / TypeScript",
            Language::Sql => "SQL",
            Language::Html => "HTML",
            Language::Css => "CSS",
        }
    }

    /// Prefijo de comentario de línea (por ejemplo `//` o `--`).
    pub fn line_comment_prefix(&self) -> Option<&'static str> {
        match self {
            Language::Rust
            | Language::C
            | Language::Cpp
            | Language::Java
            | Language::JavaScript
            | Language::Css
            | Language::Markdown
            | Language::PlainText => Some("//"),
            Language::Sql => Some("--"),
            Language::Json => None,
            Language::Html => None,
        }
    }
}

/// Categoría semántica de un token para la asignación de color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Normal,
    Keyword,
    Type,
    StringLiteral,
    Comment,
    Number,
}

impl TokenType {
    /// Retorna el color correspondiente al token según el tema visual activo.
    pub fn color(&self, theme: &crate::theme::Theme) -> egui::Color32 {
        match self {
            TokenType::Normal => theme.text,
            TokenType::Keyword => theme.syntax_keyword,
            TokenType::Type => theme.syntax_type,
            TokenType::StringLiteral => theme.syntax_string,
            TokenType::Comment => theme.syntax_comment,
            TokenType::Number => theme.syntax_number,
        }
    }
}

/// Tokeniza una línea de texto según el lenguaje especificado.
/// Retorna una secuencia ordenada de rangos de bytes y su tipo de token.
pub fn tokenize_line(line: &str, lang: Language) -> Vec<(Range<usize>, TokenType)> {
    if line.is_empty() {
        return Vec::new();
    }

    match lang {
        Language::PlainText => vec![(0..line.len(), TokenType::Normal)],
        Language::Rust => tokenize_rust(line),
        Language::Markdown => tokenize_markdown(line),
        Language::Json => tokenize_json(line),
        Language::C => tokenize_c(line),
        Language::Cpp => tokenize_cpp(line),
        Language::Java => tokenize_java(line),
        Language::JavaScript => tokenize_javascript(line),
        Language::Sql => tokenize_sql(line),
        Language::Html => tokenize_html(line),
        Language::Css => tokenize_css(line),
    }
}

/// Tokeniza una línea incorporando los spans semánticos del AST (S3,
/// Roadmap v0.2.9: llegan como datos planos desde el worker en fondo),
/// o usando el tokenizador léxico como fallback automático.
pub fn tokenize_line_semantic(
    line: &str,
    line_idx: usize,
    lang: Language,
    ast_spans: Option<&[karkinos_core::syntax_ast::AstHighlightSpan]>,
) -> Vec<(Range<usize>, TokenType)> {
    if let Some(spans) = ast_spans {
        if lang == Language::Rust {
            let mut line_tokens = Vec::new();
            for span in spans {
                if span.start_line == line_idx && span.end_line == line_idx {
                    let sb = span.start_col;
                    let eb = span.end_col;
                    if sb < eb && eb <= line.len() {
                        let token_type = match span.token_type {
                            karkinos_core::syntax_ast::SemanticTokenType::Keyword => TokenType::Keyword,
                            karkinos_core::syntax_ast::SemanticTokenType::FunctionDef
                            | karkinos_core::syntax_ast::SemanticTokenType::FunctionCall => TokenType::Type,
                            karkinos_core::syntax_ast::SemanticTokenType::TypeDef
                            | karkinos_core::syntax_ast::SemanticTokenType::TypeRef => TokenType::Type,
                            karkinos_core::syntax_ast::SemanticTokenType::StringLiteral => TokenType::StringLiteral,
                            karkinos_core::syntax_ast::SemanticTokenType::NumberLiteral => TokenType::Number,
                            karkinos_core::syntax_ast::SemanticTokenType::Comment => TokenType::Comment,
                            _ => TokenType::Normal,
                        };
                        line_tokens.push((sb..eb, token_type));
                    }
                }
            }
            if !line_tokens.is_empty() {
                line_tokens.sort_by_key(|(r, _)| r.start);
                return line_tokens;
            }
        }
    }
    tokenize_line(line, lang)
}

// -----------------------------------------------------------------------------
// Tokenizador Rust
// -----------------------------------------------------------------------------

fn tokenize_rust(line: &str) -> Vec<(Range<usize>, TokenType)> {
    tokenize_c_family(
        line,
        is_rust_keyword,
        is_rust_type,
        false, // sin backticks
        false, // sin preprocesador '#'
        false, // sin anotaciones '@'
    )
}

fn is_rust_keyword(w: &str) -> bool {
    matches!(
        w,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "yield"
    )
}

fn is_rust_type(w: &str) -> bool {
    if matches!(
        w,
        "bool"
            | "char"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "str"
            | "String"
            | "Option"
            | "Result"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
            | "Vec"
            | "Box"
            | "Arc"
            | "Rc"
            | "Cell"
            | "RefCell"
            | "HashMap"
            | "HashSet"
    ) {
        return true;
    }
    is_pascal_case(w)
}

// -----------------------------------------------------------------------------
// Tokenizador C
// -----------------------------------------------------------------------------

fn tokenize_c(line: &str) -> Vec<(Range<usize>, TokenType)> {
    tokenize_c_family(
        line,
        is_c_keyword,
        is_c_type,
        false,
        true, // preprocesador '#' (ej: #include, #define)
        false,
    )
}

fn is_c_keyword(w: &str) -> bool {
    matches!(
        w,
        "auto"
            | "break"
            | "case"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "else"
            | "enum"
            | "extern"
            | "for"
            | "goto"
            | "if"
            | "inline"
            | "register"
            | "restrict"
            | "return"
            | "sizeof"
            | "static"
            | "struct"
            | "switch"
            | "typedef"
            | "union"
            | "volatile"
            | "while"
    )
}

fn is_c_type(w: &str) -> bool {
    if matches!(
        w,
        "void"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
            | "signed"
            | "unsigned"
            | "size_t"
            | "ssize_t"
            | "int8_t"
            | "int16_t"
            | "int32_t"
            | "int64_t"
            | "uint8_t"
            | "uint16_t"
            | "uint32_t"
            | "uint64_t"
            | "bool"
            | "FILE"
            | "NULL"
    ) {
        return true;
    }
    is_pascal_case(w)
}

// -----------------------------------------------------------------------------
// Tokenizador C++
// -----------------------------------------------------------------------------

fn tokenize_cpp(line: &str) -> Vec<(Range<usize>, TokenType)> {
    tokenize_c_family(
        line,
        is_cpp_keyword,
        is_cpp_type,
        false,
        true, // preprocesador '#'
        false,
    )
}

fn is_cpp_keyword(w: &str) -> bool {
    is_c_keyword(w)
        || matches!(
            w,
            "class"
                | "public"
                | "private"
                | "protected"
                | "virtual"
                | "override"
                | "final"
                | "template"
                | "typename"
                | "namespace"
                | "using"
                | "new"
                | "delete"
                | "this"
                | "friend"
                | "operator"
                | "throw"
                | "try"
                | "catch"
                | "const_cast"
                | "static_cast"
                | "dynamic_cast"
                | "reinterpret_cast"
                | "constexpr"
                | "decltype"
                | "noexcept"
                | "nullptr"
                | "true"
                | "false"
                | "concept"
                | "requires"
                | "co_await"
                | "co_yield"
                | "co_return"
                | "explicit"
                | "mutable"
        )
}

fn is_cpp_type(w: &str) -> bool {
    if is_c_type(w)
        || matches!(
            w,
            "std"
                | "string"
                | "string_view"
                | "vector"
                | "map"
                | "set"
                | "unordered_map"
                | "unordered_set"
                | "shared_ptr"
                | "unique_ptr"
                | "weak_ptr"
                | "pair"
                | "tuple"
                | "optional"
                | "variant"
                | "any"
                | "array"
                | "deque"
                | "list"
                | "cout"
                | "cin"
                | "endl"
        )
    {
        return true;
    }
    is_pascal_case(w)
}

// -----------------------------------------------------------------------------
// Tokenizador Java
// -----------------------------------------------------------------------------

fn tokenize_java(line: &str) -> Vec<(Range<usize>, TokenType)> {
    tokenize_c_family(
        line,
        is_java_keyword,
        is_java_type,
        false,
        false,
        true, // anotaciones '@' (ej: @Override)
    )
}

fn is_java_keyword(w: &str) -> bool {
    matches!(
        w,
        "abstract"
            | "assert"
            | "boolean"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "int"
            | "interface"
            | "long"
            | "native"
            | "new"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "short"
            | "static"
            | "strictfp"
            | "super"
            | "switch"
            | "synchronized"
            | "this"
            | "throw"
            | "throws"
            | "transient"
            | "try"
            | "void"
            | "volatile"
            | "while"
            | "record"
            | "sealed"
            | "permits"
            | "yield"
            | "true"
            | "false"
            | "null"
            | "var"
    )
}

fn is_java_type(w: &str) -> bool {
    if matches!(
        w,
        "String"
            | "Integer"
            | "Long"
            | "Double"
            | "Float"
            | "Boolean"
            | "Byte"
            | "Short"
            | "Character"
            | "Object"
            | "Class"
            | "System"
            | "Math"
            | "List"
            | "ArrayList"
            | "Map"
            | "HashMap"
            | "Set"
            | "HashSet"
            | "Collections"
            | "Arrays"
            | "Optional"
            | "Stream"
            | "Exception"
            | "Throwable"
            | "Thread"
            | "Runnable"
    ) {
        return true;
    }
    is_pascal_case(w)
}

// -----------------------------------------------------------------------------
// Tokenizador JavaScript / TypeScript
// -----------------------------------------------------------------------------

fn tokenize_javascript(line: &str) -> Vec<(Range<usize>, TokenType)> {
    tokenize_c_family(
        line,
        is_js_keyword,
        is_js_type,
        true, // backticks `` `...` ``
        false,
        false,
    )
}

fn is_js_keyword(w: &str) -> bool {
    matches!(
        w,
        "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "let"
            | "new"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            | "from"
            | "as"
            | "true"
            | "false"
            | "null"
            | "undefined"
            | "NaN"
            | "interface"
            | "type"
            | "declare"
            | "namespace"
            | "enum"
            | "implements"
            | "private"
            | "protected"
            | "public"
            | "readonly"
            | "override"
            | "keyof"
            | "is"
    )
}

fn is_js_type(w: &str) -> bool {
    if matches!(
        w,
        "Promise"
            | "Array"
            | "Object"
            | "Function"
            | "Boolean"
            | "Number"
            | "String"
            | "RegExp"
            | "Date"
            | "Error"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "JSON"
            | "Math"
            | "console"
            | "document"
            | "window"
            | "any"
            | "unknown"
            | "never"
            | "boolean"
            | "number"
            | "string"
            | "symbol"
            | "bigint"
    ) {
        return true;
    }
    is_pascal_case(w)
}

// -----------------------------------------------------------------------------
// Tokenizador genérico estilo C (C, C++, Java, JS/TS, Rust)
// -----------------------------------------------------------------------------

fn tokenize_c_family(
    line: &str,
    is_keyword: impl Fn(&str) -> bool,
    is_type: impl Fn(&str) -> bool,
    allow_backticks: bool,
    allow_preprocessor: bool,
    allow_annotations: bool,
) -> Vec<(Range<usize>, TokenType)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Directivas de preprocesador C/C++ (#include, #define, etc.)
        if allow_preprocessor && b == b'#' {
            let start = i;
            i += 1;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            while i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            tokens.push((start..i, TokenType::Keyword));
            continue;
        }

        // Anotaciones Java (@Override, @Test, etc.)
        if allow_annotations && b == b'@' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Type));
            continue;
        }

        // Comentarios de línea (//...)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            tokens.push((i..len, TokenType::Comment));
            break;
        }

        // Comentarios de bloque en una sola línea (/* ... */)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
            tokens.push((start..i, TokenType::Comment));
            continue;
        }

        // Cadenas con comillas dobles ("...")
        if b == b'"' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push((start..i, TokenType::StringLiteral));
            continue;
        }

        // Cadenas con comillas simples ('...')
        if b == b'\'' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'\'' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push((start..i, TokenType::StringLiteral));
            continue;
        }

        // Template strings / backticks en JS ( `...` )
        if allow_backticks && b == b'`' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'`' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push((start..i, TokenType::StringLiteral));
            continue;
        }

        // Números (enteros, hexadecimales, flotantes)
        if b.is_ascii_digit() {
            let start = i;
            i += 1;
            if b == b'0' && i < len && (bytes[i] == b'x' || bytes[i] == b'b' || bytes[i] == b'o') {
                i += 1;
                while i < len && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
                    i += 1;
                }
            } else {
                while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
                if i < len && bytes[i] == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                        i += 1;
                    }
                }
            }
            // Sufijos numéricos (f32, u, l, n, etc.)
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Number));
            continue;
        }

        // Identificadores y palabras clave
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            let word = &line[start..i];
            let token_type = if is_keyword(word) {
                TokenType::Keyword
            } else if is_type(word) {
                TokenType::Type
            } else {
                TokenType::Normal
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Espacios y otros símbolos
        let start = i;
        while i < len
            && !bytes[i].is_ascii_alphanumeric()
            && bytes[i] != b'_'
            && bytes[i] != b'$'
            && bytes[i] != b'"'
            && bytes[i] != b'\''
            && !(allow_backticks && bytes[i] == b'`')
            && !(allow_preprocessor && bytes[i] == b'#')
            && !(allow_annotations && bytes[i] == b'@')
            && !(bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*'))
        {
            i += 1;
        }
        tokens.push((start..i, TokenType::Normal));
    }

    tokens
}

// -----------------------------------------------------------------------------
// Tokenizador SQL
// -----------------------------------------------------------------------------

fn tokenize_sql(line: &str) -> Vec<(Range<usize>, TokenType)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Comentarios SQL (--...)
        if b == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            tokens.push((i..len, TokenType::Comment));
            break;
        }

        // Comentarios de bloque (/* ... */)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
            tokens.push((start..i, TokenType::Comment));
            continue;
        }

        // Cadenas ('...' o "...")
        if b == b'\'' || b == b'"' {
            let quote = b;
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push((start..i, TokenType::StringLiteral));
            continue;
        }

        // Números
        if b.is_ascii_digit() {
            let start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Number));
            continue;
        }

        // Palabras clave y tipos SQL (case-insensitive)
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let lower = word.to_lowercase();
            let token_type = if is_sql_keyword(&lower) {
                TokenType::Keyword
            } else if is_sql_type(&lower) {
                TokenType::Type
            } else {
                TokenType::Normal
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Espacios y símbolos
        let start = i;
        while i < len
            && !bytes[i].is_ascii_alphanumeric()
            && bytes[i] != b'_'
            && bytes[i] != b'\''
            && bytes[i] != b'"'
            && !(bytes[i] == b'-' && i + 1 < len && bytes[i + 1] == b'-')
            && !(bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*')
        {
            i += 1;
        }
        tokens.push((start..i, TokenType::Normal));
    }

    tokens
}

fn is_sql_keyword(w: &str) -> bool {
    matches!(
        w,
        "select"
            | "from"
            | "where"
            | "insert"
            | "into"
            | "update"
            | "delete"
            | "create"
            | "table"
            | "drop"
            | "alter"
            | "join"
            | "inner"
            | "left"
            | "right"
            | "full"
            | "outer"
            | "on"
            | "group"
            | "by"
            | "order"
            | "having"
            | "limit"
            | "offset"
            | "and"
            | "or"
            | "not"
            | "in"
            | "is"
            | "null"
            | "like"
            | "as"
            | "distinct"
            | "union"
            | "all"
            | "case"
            | "when"
            | "then"
            | "else"
            | "end"
            | "values"
            | "set"
            | "primary"
            | "key"
            | "foreign"
            | "references"
            | "index"
            | "view"
            | "trigger"
            | "database"
            | "schema"
            | "exists"
            | "between"
            | "count"
            | "sum"
            | "avg"
            | "min"
            | "max"
            | "coalesce"
            | "begin"
            | "commit"
            | "rollback"
            | "transaction"
            | "asc"
            | "desc"
            | "default"
            | "unique"
            | "check"
    )
}

fn is_sql_type(w: &str) -> bool {
    matches!(
        w,
        "int"
            | "integer"
            | "bigint"
            | "smallint"
            | "varchar"
            | "char"
            | "text"
            | "boolean"
            | "date"
            | "time"
            | "timestamp"
            | "decimal"
            | "numeric"
            | "float"
            | "double"
            | "real"
            | "blob"
            | "serial"
            | "bytea"
            | "json"
            | "jsonb"
    )
}

// -----------------------------------------------------------------------------
// Tokenizador HTML
// -----------------------------------------------------------------------------

fn tokenize_html(line: &str) -> Vec<(Range<usize>, TokenType)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Comentarios HTML (<!-- ... -->)
        if bytes[i..].starts_with(b"<!--") {
            let start = i;
            i += 4;
            while i + 2 < len && !bytes[i..].starts_with(b"-->") {
                i += 1;
            }
            if i + 2 < len {
                i += 3;
            } else {
                i = len;
            }
            tokens.push((start..i, TokenType::Comment));
            continue;
        }

        // Apertura o cierre de etiquetas (<tag, </tag>)
        if bytes[i] == b'<' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'/' {
                i += 1;
            }
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b':') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Keyword));
            continue;
        }

        // Cierre de etiquetas (> o />)
        if bytes[i] == b'>' {
            tokens.push((i..i + 1, TokenType::Keyword));
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'>' {
            tokens.push((i..i + 2, TokenType::Keyword));
            i += 2;
            continue;
        }

        // Strings en atributos ("..." o '...')
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < len && bytes[i] != quote {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            tokens.push((start..i, TokenType::StringLiteral));
            continue;
        }

        // Entidades HTML (&nbsp;, &lt;, etc.)
        if bytes[i] == b'&' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'#') {
                i += 1;
            }
            if i < len && bytes[i] == b';' {
                i += 1;
                tokens.push((start..i, TokenType::Type));
                continue;
            }
        }

        // Nombres de atributos (identificador seguido de '=' o espacio)
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'-' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            // Si tiene '=' cerca es casi seguro un atributo HTML
            let mut is_attr = false;
            let mut j = i;
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len && bytes[j] == b'=' {
                is_attr = true;
            }
            let token_type = if is_attr || matches!(word, "class" | "id" | "src" | "href" | "style" | "type" | "name") {
                TokenType::Type
            } else {
                TokenType::Normal
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Texto o espacios normales
        let start = i;
        while i < len
            && bytes[i] != b'<'
            && bytes[i] != b'>'
            && bytes[i] != b'"'
            && bytes[i] != b'\''
            && bytes[i] != b'&'
            && !bytes[i].is_ascii_alphabetic()
        {
            i += 1;
        }
        tokens.push((start..i, TokenType::Normal));
    }

    tokens
}

// -----------------------------------------------------------------------------
// Tokenizador CSS
// -----------------------------------------------------------------------------

fn tokenize_css(line: &str) -> Vec<(Range<usize>, TokenType)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Comentarios (/* ... */ o //...)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
            tokens.push((start..i, TokenType::Comment));
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            tokens.push((i..len, TokenType::Comment));
            break;
        }

        // Cadenas ("..." o '...')
        if b == b'"' || b == b'\'' {
            let quote = b;
            let start = i;
            i += 1;
            while i < len && bytes[i] != quote {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            tokens.push((start..i, TokenType::StringLiteral));
            continue;
        }

        // At-rules (@media, @keyframes, etc.)
        if b == b'@' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Keyword));
            continue;
        }

        // Colores hexadecimales (#fff, #1a2b3c) o selectores de ID
        if b == b'#' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Number));
            continue;
        }

        // Números y unidades (10px, 1.5rem, 100%, 0)
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // Unidad (px, em, rem, %, vh, vw, s, ms, deg)
            if i < len && bytes[i] == b'%' {
                i += 1;
            } else {
                while i < len && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
            }
            tokens.push((start..i, TokenType::Number));
            continue;
        }

        // Pseudoclases (:hover, ::before)
        if b == b':' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b':' {
                i += 1;
            }
            if i < len && bytes[i].is_ascii_alphabetic() {
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                    i += 1;
                }
                tokens.push((start..i, TokenType::Keyword));
                continue;
            } else {
                tokens.push((start..start + 1, TokenType::Normal));
                continue;
            }
        }

        // Identificadores (propiedades o valores CSS)
        if b.is_ascii_alphabetic() || b == b'-' || b == b'!' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];

            // Si lo que sigue es ':' (es una propiedad CSS)
            let mut is_property = false;
            let mut j = i;
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len && bytes[j] == b':' {
                is_property = true;
            }

            let token_type = if word == "!important" || is_css_keyword(word) {
                TokenType::Keyword
            } else if is_property || is_css_property(word) {
                TokenType::Type
            } else {
                TokenType::Normal
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Espacios y símbolos
        let start = i;
        while i < len
            && !bytes[i].is_ascii_alphanumeric()
            && bytes[i] != b'-'
            && bytes[i] != b'_'
            && bytes[i] != b'@'
            && bytes[i] != b'#'
            && bytes[i] != b':'
            && bytes[i] != b'"'
            && bytes[i] != b'\''
            && !(bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'*' || bytes[i + 1] == b'/'))
        {
            i += 1;
        }
        tokens.push((start..i, TokenType::Normal));
    }

    tokens
}

fn is_css_property(w: &str) -> bool {
    matches!(
        w,
        "color"
            | "background"
            | "background-color"
            | "font-size"
            | "font-family"
            | "font-weight"
            | "line-height"
            | "margin"
            | "margin-top"
            | "margin-bottom"
            | "margin-left"
            | "margin-right"
            | "padding"
            | "padding-top"
            | "padding-bottom"
            | "padding-left"
            | "padding-right"
            | "border"
            | "border-radius"
            | "display"
            | "flex"
            | "grid"
            | "width"
            | "height"
            | "min-width"
            | "max-width"
            | "position"
            | "top"
            | "bottom"
            | "left"
            | "right"
            | "z-index"
            | "opacity"
            | "overflow"
            | "cursor"
            | "box-shadow"
            | "transform"
            | "transition"
            | "align-items"
            | "justify-content"
    )
}

fn is_css_keyword(w: &str) -> bool {
    matches!(
        w,
        "auto"
            | "none"
            | "block"
            | "inline"
            | "flex"
            | "grid"
            | "absolute"
            | "relative"
            | "fixed"
            | "sticky"
            | "solid"
            | "dashed"
            | "dotted"
            | "transparent"
            | "center"
            | "hidden"
            | "visible"
            | "bold"
            | "pointer"
            | "inherit"
            | "initial"
            | "unset"
    )
}

// -----------------------------------------------------------------------------
// Tokenizador Markdown
// -----------------------------------------------------------------------------

fn tokenize_markdown(line: &str) -> Vec<(Range<usize>, TokenType)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    // Encabezados (# Titulo)
    if trimmed.starts_with('#') {
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if trimmed[hash_count..].starts_with(' ') || hash_count == trimmed.len() {
            return vec![
                (0..indent + hash_count, TokenType::Keyword),
                (indent + hash_count..line.len(), TokenType::Type),
            ];
        }
    }

    // Listas con viñetas o citas
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("> ") {
        let mut tokens = vec![
            (0..indent, TokenType::Normal),
            (indent..indent + 2, TokenType::Keyword),
        ];
        if indent + 2 < line.len() {
            tokens.push((indent + 2..line.len(), TokenType::Normal));
        }
        return tokens;
    }

    // Código en bloque o inline (`code`)
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len && bytes[i] == b'`' {
                i += 1;
            }
            tokens.push((start..i, TokenType::StringLiteral));
        } else {
            let start = i;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            tokens.push((start..i, TokenType::Normal));
        }
    }

    tokens
}

// -----------------------------------------------------------------------------
// Tokenizador JSON
// -----------------------------------------------------------------------------

fn tokenize_json(line: &str) -> Vec<(Range<usize>, TokenType)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Comentarios (JSONC //...)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            tokens.push((i..len, TokenType::Comment));
            break;
        }

        // Strings o Claves de propiedad ("...")
        if b == b'"' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            // Verificar si lo que sigue es ':' (es una clave de propiedad)
            let mut is_key = false;
            let mut j = i;
            while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < len && bytes[j] == b':' {
                is_key = true;
            }

            let token_type = if is_key {
                TokenType::Type
            } else {
                TokenType::StringLiteral
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Números
        if b.is_ascii_digit() || (b == b'-' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'e' || bytes[i] == b'E' || bytes[i] == b'+' || bytes[i] == b'-') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Number));
            continue;
        }

        // Palabras clave: true, false, null
        if b.is_ascii_alphabetic() {
            let start = i;
            while i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word = &line[start..i];
            let token_type = if matches!(word, "true" | "false" | "null") {
                TokenType::Keyword
            } else {
                TokenType::Normal
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Puntuación y espacios
        let start = i;
        while i < len
            && !bytes[i].is_ascii_alphanumeric()
            && bytes[i] != b'"'
            && bytes[i] != b'-'
            && !(bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/')
        {
            i += 1;
        }
        tokens.push((start..i, TokenType::Normal));
    }

    tokens
}

// -----------------------------------------------------------------------------
// Helper PascalCase
// -----------------------------------------------------------------------------

fn is_pascal_case(w: &str) -> bool {
    let mut chars = w.chars();
    if let Some(first) = chars.next() {
        first.is_ascii_uppercase() && chars.any(|c| c.is_ascii_lowercase())
    } else {
        false
    }
}

// -----------------------------------------------------------------------------
// LayoutJob Builder
// -----------------------------------------------------------------------------

/// Construye un `LayoutJob` con formato coloreado para un fragmento (`chunk`) de una línea,
/// a partir de los tokens de la línea completa.
pub fn build_layout_job(
    chunk: &str,
    chunk_byte_start: usize,
    line_tokens: &[(Range<usize>, TokenType)],
    theme: &crate::theme::Theme,
    font_id: egui::FontId,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let chunk_byte_end = chunk_byte_start + chunk.len();
    let mut cursor = chunk_byte_start;

    for (range, token_type) in line_tokens {
        if range.end <= cursor || range.start >= chunk_byte_end {
            continue;
        }

        let token_start = range.start.max(cursor);
        let token_end = range.end.min(chunk_byte_end);

        // Espacio o texto sin token específico antes del token actual
        if token_start > cursor {
            let slice = &chunk[(cursor - chunk_byte_start)..(token_start - chunk_byte_start)];
            job.append(
                slice,
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: theme.text,
                    ..Default::default()
                },
            );
        }

        let slice = &chunk[(token_start - chunk_byte_start)..(token_end - chunk_byte_start)];
        job.append(
            slice,
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: token_type.color(theme),
                ..Default::default()
            },
        );
        cursor = token_end;
    }

    // Resto del chunk posterior al último token
    if cursor < chunk_byte_end {
        let slice = &chunk[(cursor - chunk_byte_start)..];
        job.append(
            slice,
            0.0,
            egui::TextFormat {
                font_id,
                color: theme.text,
                ..Default::default()
            },
        );
    }

    job
}

// -----------------------------------------------------------------------------
// LineLayoutCache (Roadmap v0.2.9, S1)
// -----------------------------------------------------------------------------

/// Entrada única del cache de layouts: el galley ya armado + el tick
/// de su último uso (el cache es LRU y poda lo más viejo).
#[derive(Clone)]
struct CacheEntry<V> {
    value: V,
    last_used: u64,
}

/// Caché de líneas ya tipografiadas. El render del editor armaba el
/// `LayoutJob` y pedía a egui el layout de CADA fragmento visible en
/// CADA frame: a 144Hz el lexer y el shaper corrían cientos de veces
/// por segundo sin que el usuario tocara nada. Con este cache, el
/// costo se paga solo cuando la línea cambia.
///
/// Clave (S1): hash de los bytes del fragmento + lenguaje + tamaño de
/// fuente, junto al modo de tema (los colores de sintaxis dependen de
/// la paleta activa). Invalidación (S1): al cambiar de tema la clave
/// nueva apunta a otra entrada; al regenerar el AST (`sync_ast`) se
/// vacía el cache entero, porque las marcas semánticas de líneas SIN
/// modificar pueden cambiar (p.ej. desbalancear un bloque arriba
/// reclasifica lo de abajo).
///
/// `V` se deja genérico para poder probar el LRU/invalidación con
/// valores baratos; en producción `V = Arc<egui::Galley>` (el galley
/// ya viene tipografiado por egui: reusarlo salta TAMBIÉN el shaper).
pub struct LineLayoutCache<V> {
    entries: std::collections::HashMap<(u64, crate::theme::ThemeMode), CacheEntry<V>>,
    ast_generation: u64,
    tick: u64,
}

impl<V: Clone> LineLayoutCache<V> {
    /// Techo de entradas (≈ 2000 líneas visibles del S1). Al pasarlo
    /// se descarta el cuarto más viejo; las entradas reutilizadas
    /// recientemente sobreviven.
    pub const CAPACITY: usize = 2048;

    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            ast_generation: 0,
            tick: 0,
        }
    }

    /// Devuelve el valor cacheado para `(hash, tema)` o lo produce
    /// una vez y lo recuerda. Actualiza el tick LRU de la entrada.
    pub fn get_or_insert(&mut self, hash: u64, theme: crate::theme::ThemeMode, produce: impl FnOnce() -> V) -> V {
        self.tick = self.tick.wrapping_add(1);
        let current_tick = self.tick;
        let entry = self
            .entries
            .entry((hash, theme))
            .and_modify(|e| e.last_used = current_tick)
            .or_insert_with(|| CacheEntry { value: produce(), last_used: current_tick });
        let value = entry.value.clone();
        self.prune_if_full();
        value
    }

    /// Sincroniza la estampa de generación del AST: si cambió desde la
    /// última sincronización, vacía el cache (los galleys pueden tener
    /// colores semánticos desactualizados).
    pub fn sync_ast(&mut self, generation: u64) {
        if self.ast_generation != generation {
            self.ast_generation = generation;
            self.entries.clear();
        }
    }

    /// Cantidad de entradas vivas (para diagnóstico y tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Poda LRU: cuando se supera `CAPACITY` descarta el cuarto más
    /// viejo (amortizado: corre pocas veces y siempre carga pequeña).
    fn prune_if_full(&mut self) {
        if self.entries.len() <= Self::CAPACITY {
            return;
        }
        let mut by_age: Vec<(u64, (u64, crate::theme::ThemeMode))> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.last_used, *key))
            .collect();
        by_age.sort_by_key(|&(tick, _)| tick);
        let to_drop = by_age.len() / 4;
        for (_, key) in by_age.into_iter().take(to_drop) {
            self.entries.remove(&key);
        }
    }
}

impl<V: Clone> Clone for LineLayoutCache<V> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            ast_generation: self.ast_generation,
            tick: self.tick,
        }
    }
}

impl<V> std::fmt::Debug for LineLayoutCache<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineLayoutCache")
            .field("entries", &self.entries.len())
            .field("ast_generation", &self.ast_generation)
            .finish()
    }
}

/// Hash de la clave de cache de un fragmento: bytes del fragmento,
/// lenguaje y tamaño de fuente (el modo de tema viaja aparte, como
/// segunda mitad de la clave del HashMap).
pub fn line_layout_key(chunk: &str, lang: Language, font_size: f32) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&chunk.as_bytes(), &mut hasher);
    std::hash::Hash::hash(&(lang as u32), &mut hasher);
    std::hash::Hash::hash(&font_size.to_bits(), &mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    #[test]
    fn test_detection_by_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("md"), Language::Markdown);
        assert_eq!(Language::from_extension("markdown"), Language::Markdown);
        assert_eq!(Language::from_extension("json"), Language::Json);
        assert_eq!(Language::from_extension("c"), Language::C);
        assert_eq!(Language::from_extension("h"), Language::C);
        assert_eq!(Language::from_extension("cpp"), Language::Cpp);
        assert_eq!(Language::from_extension("hpp"), Language::Cpp);
        assert_eq!(Language::from_extension("java"), Language::Java);
        assert_eq!(Language::from_extension("js"), Language::JavaScript);
        assert_eq!(Language::from_extension("ts"), Language::JavaScript);
        assert_eq!(Language::from_extension("sql"), Language::Sql);
        assert_eq!(Language::from_extension("html"), Language::Html);
        assert_eq!(Language::from_extension("css"), Language::Css);
        assert_eq!(Language::from_extension("scss"), Language::Css);
        assert_eq!(Language::from_extension("txt"), Language::PlainText);
    }

    #[test]
    fn test_autodetect_language_without_extension() {
        use std::path::Path;

        // 1. Filenames without extension
        assert_eq!(Language::detect(Some(Path::new("README")), ""), Language::Markdown);
        assert_eq!(Language::detect(Some(Path::new("CHANGELOG")), ""), Language::Markdown);

        // 2. Shebangs without extension
        assert_eq!(Language::detect(Some(Path::new("my-script")), "#!/usr/bin/env node\nconsole.log(1);"), Language::JavaScript);
        assert_eq!(Language::detect(None, "#!/usr/bin/env rust-script\nfn main() {}"), Language::Rust);

        // 3. Rust syntax signatures without extension
        assert_eq!(Language::detect(Some(Path::new("unknown_file")), "pub fn compute(x: i32) -> i32 {\n    let mut y = x;\n    y\n}"), Language::Rust);
        assert_eq!(Language::detect(None, "#[derive(Debug)]\npub struct Config {\n}"), Language::Rust);

        // 4. HTML signatures without extension
        assert_eq!(Language::detect(None, "<!DOCTYPE html>\n<html>\n<body>\n</body>\n</html>"), Language::Html);

        // 5. JSON signatures without extension
        assert_eq!(Language::detect(None, "{\n  \"name\": \"karkinos\",\n  \"version\": 1\n}"), Language::Json);

        // 6. C/C++ signatures without extension
        assert_eq!(Language::detect(None, "#include <stdio.h>\nint main() { return 0; }"), Language::C);
        assert_eq!(Language::detect(None, "#include <iostream>\nint main() { std::cout << 42; return 0; }"), Language::Cpp);

        // 7. SQL signatures without extension
        assert_eq!(Language::detect(None, "SELECT user_id, email FROM users WHERE active = 1;"), Language::Sql);

        // 8. Plain text fallback
        assert_eq!(Language::detect(None, "Just some normal notes without any code patterns."), Language::PlainText);
    }

    #[test]
    fn test_tokenize_rust() {
        let code = "fn main() { let x = 42; // respuesta\n }";
        let tokens = tokenize_line(code, Language::Rust);
        assert!(tokens.iter().any(|(r, t)| &code[r.clone()] == "fn" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &code[r.clone()] == "let" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &code[r.clone()] == "42" && *t == TokenType::Number));
        assert!(tokens.iter().any(|(r, t)| code[r.clone()].starts_with("//") && *t == TokenType::Comment));
    }

    #[test]
    fn test_tokenize_c_y_cpp() {
        let c_code = "#include <stdio.h>\nint main() { printf(\"Hola\"); return 0; }";
        let c_tokens = tokenize_line(c_code, Language::C);
        assert!(c_tokens.iter().any(|(r, t)| &c_code[r.clone()] == "#include" && *t == TokenType::Keyword));
        assert!(c_tokens.iter().any(|(r, t)| &c_code[r.clone()] == "int" && *t == TokenType::Type));
        assert!(c_tokens.iter().any(|(r, t)| &c_code[r.clone()] == "return" && *t == TokenType::Keyword));
        assert!(c_tokens.iter().any(|(r, t)| &c_code[r.clone()] == "\"Hola\"" && *t == TokenType::StringLiteral));

        let cpp_code = "class MyClass { public: template<typename T> void run(); };";
        let cpp_tokens = tokenize_line(cpp_code, Language::Cpp);
        assert!(cpp_tokens.iter().any(|(r, t)| &cpp_code[r.clone()] == "class" && *t == TokenType::Keyword));
        assert!(cpp_tokens.iter().any(|(r, t)| &cpp_code[r.clone()] == "public" && *t == TokenType::Keyword));
        assert!(cpp_tokens.iter().any(|(r, t)| &cpp_code[r.clone()] == "template" && *t == TokenType::Keyword));
        assert!(cpp_tokens.iter().any(|(r, t)| &cpp_code[r.clone()] == "void" && *t == TokenType::Type));
    }

    #[test]
    fn test_tokenize_java() {
        let java_code = "@Override public void print(String text) { int val = 10; }";
        let tokens = tokenize_line(java_code, Language::Java);
        assert!(tokens.iter().any(|(r, t)| &java_code[r.clone()] == "@Override" && *t == TokenType::Type));
        assert!(tokens.iter().any(|(r, t)| &java_code[r.clone()] == "public" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &java_code[r.clone()] == "String" && *t == TokenType::Type));
        assert!(tokens.iter().any(|(r, t)| &java_code[r.clone()] == "10" && *t == TokenType::Number));
    }

    #[test]
    fn test_tokenize_javascript() {
        let js_code = "const greet = async (name) => `Hello ${name}`;";
        let tokens = tokenize_line(js_code, Language::JavaScript);
        assert!(tokens.iter().any(|(r, t)| &js_code[r.clone()] == "const" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &js_code[r.clone()] == "async" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &js_code[r.clone()] == "`Hello ${name}`" && *t == TokenType::StringLiteral));
    }

    #[test]
    fn test_tokenize_sql() {
        let sql = "SELECT id, name, count(*) FROM users WHERE age >= 18 ORDER BY id DESC;";
        let tokens = tokenize_line(sql, Language::Sql);
        assert!(tokens.iter().any(|(r, t)| &sql[r.clone()] == "SELECT" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &sql[r.clone()] == "FROM" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &sql[r.clone()] == "WHERE" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &sql[r.clone()] == "18" && *t == TokenType::Number));
    }

    #[test]
    fn test_tokenize_html() {
        let html = "<div class=\"container\"><p>Hola</p></div>";
        let tokens = tokenize_line(html, Language::Html);
        assert!(tokens.iter().any(|(r, t)| &html[r.clone()] == "<div" && *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(r, t)| &html[r.clone()] == "class" && *t == TokenType::Type));
        assert!(tokens.iter().any(|(r, t)| &html[r.clone()] == "\"container\"" && *t == TokenType::StringLiteral));
    }

    #[test]
    fn test_tokenize_css() {
        let css = ".box { color: #ff0000; font-size: 14px; display: flex; }";
        let tokens = tokenize_line(css, Language::Css);
        assert!(tokens.iter().any(|(r, t)| &css[r.clone()] == "color" && *t == TokenType::Type));
        assert!(tokens.iter().any(|(r, t)| &css[r.clone()] == "#ff0000" && *t == TokenType::Number));
        assert!(tokens.iter().any(|(r, t)| &css[r.clone()] == "14px" && *t == TokenType::Number));
        assert!(tokens.iter().any(|(r, t)| &css[r.clone()] == "flex" && *t == TokenType::Keyword));
    }

    #[test]
    fn test_build_layout_job() {
        let line = "let a = 10;";
        let tokens = tokenize_line(line, Language::Rust);
        let theme = crate::theme::ThemeMode::Dark.theme();
        let font_id = egui::FontId::monospace(14.0);

        let job = build_layout_job(line, 0, &tokens, &theme, font_id.clone());
        assert_eq!(job.text, line);
        assert!(job.sections.len() >= 3);

        let chunk = &line[4..];
        let sub_job = build_layout_job(chunk, 4, &tokens, &theme, font_id);
        assert_eq!(sub_job.text, chunk);
        assert!(!sub_job.sections.is_empty());
    }

    #[test]
    fn test_tokenize_line_semantic_with_ast() {
        use karkinos_core::syntax_ast::{AstEngine, AstLanguage};

        let mut engine = AstEngine::new(AstLanguage::Rust).expect("should init rust engine");
        let code = "fn compute() {\n    let val = 42;\n}";
        engine.update_from_text(code);

        // S3 (Roadmap v0.2.9): la función ahora recibe los spans planos
        // que produce el worker de fondo, no el motor completo.
        let spans = engine.highlight_spans();

        // Line 0: "fn compute() {"
        let tokens = tokenize_line_semantic("fn compute() {", 0, Language::Rust, Some(&spans));
        assert!(!tokens.is_empty());
        assert!(tokens.iter().any(|(_, t)| *t == TokenType::Keyword));
        assert!(tokens.iter().any(|(_, t)| *t == TokenType::Type)); // compute as FunctionDef mapped to Type

        // Line 1: "    let val = 42;"
        let line1_tokens = tokenize_line_semantic("    let val = 42;", 1, Language::Rust, Some(&spans));
        assert!(!line1_tokens.is_empty());
        assert!(line1_tokens.iter().any(|(_, t)| *t == TokenType::Keyword));
        assert!(line1_tokens.iter().any(|(_, t)| *t == TokenType::Number));

        // Fallback test without engine
        let fallback = tokenize_line_semantic("let a = 10;", 0, Language::Rust, None);
        assert!(!fallback.is_empty());
    }

    // ---------------------------------------------------------------------
    // LineLayoutCache (Roadmap v0.2.9, S1)
    // ---------------------------------------------------------------------

    #[test]
    fn line_layout_cache_reuses_value_for_same_key_and_theme() {
        let mut cache = LineLayoutCache::new();
        let mut produced = 0u64;
        let key = line_layout_key("let a = 10;", Language::Rust, 14.0);
        let first = cache.get_or_insert(key, ThemeMode::Dark, || {
            produced += 1;
            produced
        });
        let second = cache.get_or_insert(key, ThemeMode::Dark, || {
            produced += 1;
            produced
        });
        assert_eq!(first, 1);
        assert_eq!(second, 1, "el segundo frame debe reutilizar el valor cacheado");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn line_layout_cache_separates_themes() {
        let mut cache = LineLayoutCache::new();
        let key = line_layout_key("let a = 10;", Language::Rust, 14.0);
        assert_eq!(cache.get_or_insert(key, ThemeMode::Dark, || 1), 1);
        assert_eq!(cache.get_or_insert(key, ThemeMode::Monokai, || 2), 2);
        assert_eq!(cache.len(), 2, "cada tema tiene su propia entrada");
        // el valor del tema Monokai sigue vivo (no cross-contaminó al Dark)
        assert_eq!(cache.get_or_insert(key, ThemeMode::Monokai, || 3), 2);
    }

    #[test]
    fn line_layout_cache_clears_when_ast_generation_changes() {
        let mut cache = LineLayoutCache::new();
        let key = line_layout_key("x", Language::Rust, 14.0);
        // Generación inicial (0): produce y cachea.
        assert_eq!(cache.get_or_insert(key, ThemeMode::Dark, || 10), 10);
        // Sincronizar con una generación NUEVA vacía la caché.
        cache.sync_ast(7);
        assert!(cache.is_empty(), "un AST nuevo invalida los galleys viejos");
        assert_eq!(cache.get_or_insert(key, ThemeMode::Dark, || 11), 11);
        // Volver a sincronizar con la MISMA generación NO debe volver a vaciar.
        cache.sync_ast(7);
        assert_eq!(cache.get_or_insert(key, ThemeMode::Dark, || 12), 11);
        // Y una generación distinta vuelve a vaciar.
        cache.sync_ast(8);
        assert!(cache.is_empty());
    }

    #[test]
    fn line_layout_cache_keys_differ_by_input() {
        let k1 = line_layout_key("let x = 1;", Language::Rust, 14.0);
        let k1b = line_layout_key("let x = 1;", Language::Rust, 14.0);
        let k_lang = line_layout_key("let x = 1;", Language::Cpp, 14.0);
        let k_font = line_layout_key("let x = 1;", Language::Rust, 15.0);
        let k_text = line_layout_key("let x = 2;", Language::Rust, 14.0);
        assert_eq!(k1, k1b, "mismos inputs → misma clave");
        assert_ne!(k1, k_lang, "el lenguaje entra en la clave");
        assert_ne!(k1, k_font, "el tamaño de fuente entra en la clave");
        assert_ne!(k1, k_text, "el contenido entra en la clave");
    }

    #[test]
    fn line_layout_cache_evicts_lru_and_keeps_recent_entries() {
        let mut cache = LineLayoutCache::new();
        let first_key = line_layout_key("first", Language::Rust, 14.0);
        cache.get_or_insert(first_key, ThemeMode::Dark, || 0);

        // Tormenta de líneas nuevas. El editor re-toca sus líneas visibles
        // en cada frame: se emula re-tocando `first_key` periódicamente,
        // tal como el render haría, para que el LRU no lo canjee.
        for i in 0..(LineLayoutCache::<u64>::CAPACITY + 256) {
            let key = line_layout_key(&format!("line-{i}"), Language::Rust, 14.0);
            cache.get_or_insert(key, ThemeMode::Dark, || i as u64);
            if i % 64 == 0 {
                cache.get_or_insert(first_key, ThemeMode::Dark, || 0);
            }
        }
        assert!(
            cache.len() <= LineLayoutCache::<u64>::CAPACITY,
            "la caché debe podar entradas viejas en vez de crecer sin techo"
        );
        assert_eq!(
            cache.get_or_insert(first_key, ThemeMode::Dark, || 999),
            0,
            "la entrada re-usada periódicamente sobrevive a la tormenta"
        );
    }
}
