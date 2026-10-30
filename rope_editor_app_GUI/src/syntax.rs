//! S6 (Etapa 2): Resaltado de sintaxis ligero con soporte para Rust,
//! Markdown y JSON, renderizado mediante `egui::text::LayoutJob`.

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

    /// Detecta el lenguaje a partir de una extensión textual (ej: "rs", "json").
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "md" | "markdown" => Language::Markdown,
            "json" | "jsonc" => Language::Json,
            _ => Language::PlainText,
        }
    }

    /// Nombre legible del lenguaje para la interfaz de usuario.
    #[allow(dead_code)]
    pub fn name(&self) -> &'static str {
        match self {
            Language::PlainText => "Texto plano",
            Language::Rust => "Rust",
            Language::Markdown => "Markdown",
            Language::Json => "JSON",
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
    }
}

/// Tokenizador ligero para Rust.
fn tokenize_rust(line: &str) -> Vec<(Range<usize>, TokenType)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Comentarios de línea (//...)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            tokens.push((i..len, TokenType::Comment));
            break;
        }

        // Cadenas de texto ("...")
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

        // Literales de caracteres ('c' o '\n')
        if b == b'\'' && i + 1 < len {
            let start = i;
            let mut j = i + 1;
            if j < len && bytes[j] == b'\\' {
                j += 2;
            } else if j < len {
                j += 1;
            }
            if j < len && bytes[j] == b'\'' {
                i = j + 1;
                tokens.push((start..i, TokenType::StringLiteral));
                continue;
            }
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
            // Sufijo de tipo (ej: u32, f64, usize)
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((start..i, TokenType::Number));
            continue;
        }

        // Identificadores y palabras clave
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let token_type = if is_rust_keyword(word) {
                TokenType::Keyword
            } else if is_rust_type(word) {
                TokenType::Type
            } else {
                TokenType::Normal
            };
            tokens.push((start..i, token_type));
            continue;
        }

        // Espacios y otros caracteres
        let start = i;
        while i < len
            && !bytes[i].is_ascii_alphanumeric()
            && bytes[i] != b'_'
            && bytes[i] != b'"'
            && bytes[i] != b'\''
            && !(bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/')
        {
            i += 1;
        }
        tokens.push((start..i, TokenType::Normal));
    }

    tokens
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
    ) {
        return true;
    }
    // Tipos convencionales en PascalCase
    let mut chars = w.chars();
    if let Some(first) = chars.next() {
        first.is_ascii_uppercase() && chars.any(|c| c.is_ascii_lowercase())
    } else {
        false
    }
}

/// Tokenizador ligero para Markdown.
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

/// Tokenizador ligero para JSON.
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
        assert_eq!(Language::from_extension("txt"), Language::PlainText);
        assert_eq!(Language::from_extension("unknown"), Language::PlainText);
    }

    #[test]
    fn test_tokenize_rust() {
        let code = "fn main() { let x = 42; // respuesta\n }";
        let tokens = tokenize_line(code, Language::Rust);

        // Debe haber tokens de fn (Keyword), let (Keyword), 42 (Number), // respuesta (Comment)
        let has_keyword_fn = tokens.iter().any(|(r, t)| &code[r.clone()] == "fn" && *t == TokenType::Keyword);
        let has_keyword_let = tokens.iter().any(|(r, t)| &code[r.clone()] == "let" && *t == TokenType::Keyword);
        let has_number = tokens.iter().any(|(r, t)| &code[r.clone()] == "42" && *t == TokenType::Number);
        let has_comment = tokens.iter().any(|(r, t)| code[r.clone()].starts_with("//") && *t == TokenType::Comment);

        assert!(has_keyword_fn);
        assert!(has_keyword_let);
        assert!(has_number);
        assert!(has_comment);
    }

    #[test]
    fn test_tokenize_json() {
        let json = r#"{"name": "test", "count": 100, "active": true}"#;
        let tokens = tokenize_line(json, Language::Json);

        let has_key = tokens.iter().any(|(r, t)| &json[r.clone()] == "\"name\"" && *t == TokenType::Type);
        let has_str_val = tokens.iter().any(|(r, t)| &json[r.clone()] == "\"test\"" && *t == TokenType::StringLiteral);
        let has_num = tokens.iter().any(|(r, t)| &json[r.clone()] == "100" && *t == TokenType::Number);
        let has_kw = tokens.iter().any(|(r, t)| &json[r.clone()] == "true" && *t == TokenType::Keyword);

        assert!(has_key);
        assert!(has_str_val);
        assert!(has_num);
        assert!(has_kw);
    }

    #[test]
    fn test_tokenize_markdown() {
        let md = "# Titulo Principal";
        let tokens = tokenize_line(md, Language::Markdown);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].1, TokenType::Keyword);
        assert_eq!(tokens[1].1, TokenType::Type);

        let inline_code = "Usa `cargo test` para verificar";
        let tokens2 = tokenize_line(inline_code, Language::Markdown);
        let has_code = tokens2.iter().any(|(r, t)| &inline_code[r.clone()] == "`cargo test`" && *t == TokenType::StringLiteral);
        assert!(has_code);
    }

    #[test]
    fn test_build_layout_job() {
        let line = "let a = 10;";
        let tokens = tokenize_line(line, Language::Rust);
        let theme = crate::theme::ThemeMode::Dark.theme();
        let font_id = egui::FontId::monospace(14.0);

        let job = build_layout_job(line, 0, &tokens, &theme, font_id.clone());
        assert_eq!(job.text, line);
        // Debe tener varias secciones formateadas
        assert!(job.sections.len() >= 3);

        // Sub-fila envuelta: chunk que empieza en byte offset 4 ("a = 10;")
        let chunk = &line[4..];
        let sub_job = build_layout_job(chunk, 4, &tokens, &theme, font_id);
        assert_eq!(sub_job.text, chunk);
        assert!(!sub_job.sections.is_empty());
    }
}
