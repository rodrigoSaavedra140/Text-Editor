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
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        Self::from_extension(ext)
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
            Language::PlainText => "Texto plano",
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
