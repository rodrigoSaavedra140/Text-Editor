//! Text manipulation algorithms: word boundaries, indentation, bracket matching, and comment toggling.

/// Converts a character index to a byte offset within a UTF-8 string slice.
pub fn char_to_byte(s: &str, char_idx: usize) -> usize {
    if s.is_ascii() {
        return char_idx.min(s.len());
    }
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

/// Returns the number of characters in a string slice (O(1) fast path for ASCII).
pub fn line_char_count(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().count()
    }
}

/// Checks whether a character is an alphanumeric word character.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Computes cursor position for next word jump (Ctrl+Right).
pub fn next_word_pos(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    let line = line.min(lines.len() - 1);
    let chars: Vec<char> = lines[line].chars().collect();
    let len = chars.len();
    if col >= len {
        if line + 1 < lines.len() {
            (line + 1, 0)
        } else {
            (line, len)
        }
    } else {
        let mut c = col;
        if is_word_char(chars[c]) {
            while c < len && is_word_char(chars[c]) {
                c += 1;
            }
        } else {
            while c < len && !is_word_char(chars[c]) {
                c += 1;
            }
        }
        (line, c)
    }
}

/// Computes cursor position for previous word jump (Ctrl+Left).
pub fn prev_word_pos(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    if col == 0 {
        if line > 0 {
            (line - 1, line_char_count(&lines[line - 1]))
        } else {
            (0, 0)
        }
    } else {
        let line = line.min(lines.len() - 1);
        let chars: Vec<char> = lines[line].chars().collect();
        let mut c = col.min(chars.len());
        // 1. Skip whitespace backwards
        while c > 0 && chars[c - 1].is_whitespace() {
            c -= 1;
        }
        if c == 0 {
            return (line, 0);
        }
        // 2. Skip either word characters or punctuation characters
        if is_word_char(chars[c - 1]) {
            while c > 0 && is_word_char(chars[c - 1]) {
                c -= 1;
            }
        } else {
            while c > 0 && !is_word_char(chars[c - 1]) && !chars[c - 1].is_whitespace() {
                c -= 1;
            }
        }
        (line, c)
    }
}

/// Default visual tab size (4 spaces).
pub const DEFAULT_TAB_SIZE: usize = 4;

/// Computes the 0-based visual column for a given 0-based character index within `line`,
/// expanding `\t` characters to tab stops of `tab_size`.
pub fn visual_col_at(line: &str, char_idx: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let mut vcol = 0;
    for (i, ch) in line.chars().enumerate() {
        if i >= char_idx {
            break;
        }
        if ch == '\t' {
            vcol += tab_size - (vcol % tab_size);
        } else {
            vcol += 1;
        }
    }
    vcol
}

/// Converts a 0-based visual column back to the corresponding character index within `line`.
/// Rounds to the nearest character boundary.
pub fn col_from_visual_col(line: &str, target_vcol: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let mut current_vcol = 0;
    for (i, ch) in line.chars().enumerate() {
        if current_vcol >= target_vcol {
            return i;
        }
        let char_width = if ch == '\t' {
            tab_size - (current_vcol % tab_size)
        } else {
            1
        };
        if current_vcol + char_width > target_vcol {
            if target_vcol - current_vcol >= char_width.div_ceil(2) {
                return i + 1;
            } else {
                return i;
            }
        }
        current_vcol += char_width;
    }
    line_char_count(line)
}

/// Computes the total visual character length of a line, expanding `\t`.
pub fn line_visual_char_count(line: &str, tab_size: usize) -> usize {
    visual_col_at(line, usize::MAX, tab_size)
}

/// Returns the `[start, end)` character bounds of the word at `col`.
pub fn word_range_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let c = if col >= chars.len() {
        chars.len() - 1
    } else {
        col
    };
    if is_word_char(chars[c]) {
        let mut start = c;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = c + 1;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }
        (start, end)
    } else {
        (c, c + 1)
    }
}

/// Returns the matching closing delimiter for an opening delimiter, or `None`
/// if `open` does not open a pair. Quotes and backticks are self-pairing.
pub fn closing_delimiter(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

/// Returns `true` when `c` is a closing delimiter (`)`, `]`, `}`, `"`, `'`, `` ` ``).
pub fn is_closing_delimiter(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'' | '`')
}

/// Returns `true` when the cursor at `col` sits between an empty pair, i.e.
/// the character before and after form `()`, `[]`, `{}`, `""`, `''` or `` `` ``.
pub fn is_empty_pair_around(line: &str, col: usize) -> bool {
    if col == 0 {
        return false;
    }
    let chars: Vec<char> = line.chars().collect();
    if col > chars.len() {
        return false;
    }
    if let (Some(&before), Some(&after)) = (chars.get(col - 1), chars.get(col)) {
        closing_delimiter(before) == Some(after)
    } else {
        false
    }
}

/// Returns leading whitespace from `prev_line` for auto-indentation.
pub fn inherited_indent(prev_line: &str) -> String {
    prev_line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// Dedents a line by removing a leading `\t` or up to 4 spaces. Returns removed character count.
pub fn dedent_line(line: &mut String) -> usize {
    if line.starts_with('\t') {
        line.remove(0);
        1
    } else if line.starts_with(' ') {
        let spaces = line.chars().take(4).take_while(|&c| c == ' ').count();
        line.drain(0..spaces);
        spaces
    } else {
        0
    }
}

/// Indents a line by inserting a tab at the beginning.
pub fn indent_line(line: &mut String) {
    line.insert(0, '\t');
}

/// Toggles line or block comments using the specified prefix.
pub fn toggle_comment_range(
    lines: &mut [String],
    start_line: usize,
    end_line: usize,
    prefix: &str,
) {
    if lines.is_empty() {
        return;
    }
    let end_l = end_line.min(lines.len() - 1);
    let start_l = start_line.min(end_l);

    let all_commented = (start_l..=end_l).all(|l| {
        let trimmed = lines[l].trim_start();
        trimmed.is_empty() || trimmed.starts_with(prefix)
    });

    for line in lines.iter_mut().take(end_l + 1).skip(start_l) {
        if line.trim().is_empty() {
            continue;
        }
        if all_commented {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(line, indent_len);
            let rest = &line[indent_bytes..];
            if let Some(stripped) = rest.strip_prefix(prefix) {
                let strip_len = if stripped.starts_with(' ') {
                    prefix.len() + 1
                } else {
                    prefix.len()
                };
                line.replace_range(indent_bytes..indent_bytes + strip_len, "");
            }
        } else {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(line, indent_len);
            let insert = format!("{} ", prefix);
            line.insert_str(indent_bytes, &insert);
        }
    }
}

/// Match information for paired delimiter brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BracketMatch {
    pub origin: (usize, usize),
    pub partner: Option<(usize, usize)>,
}

fn matching_partner(c: char) -> Option<(char, bool)> {
    match c {
        '(' => Some((')', true)),
        '[' => Some((']', true)),
        '{' => Some(('}', true)),
        ')' => Some(('(', false)),
        ']' => Some(('[', false)),
        '}' => Some(('{', false)),
        _ => None,
    }
}

/// Finds the matching delimiter bracket starting from the given cursor position.
pub fn find_matching_bracket(
    lines: &[String],
    cursor_line: usize,
    cursor_col: usize,
) -> Option<BracketMatch> {
    if lines.is_empty() || cursor_line >= lines.len() {
        return None;
    }
    let cur_line = &lines[cursor_line];
    let cur_chars: Vec<char> = cur_line.chars().collect();

    let (origin_col, ch) = if cursor_col < cur_chars.len()
        && matching_partner(cur_chars[cursor_col]).is_some()
    {
        (cursor_col, cur_chars[cursor_col])
    } else if cursor_col > 0
        && cursor_col - 1 < cur_chars.len()
        && matching_partner(cur_chars[cursor_col - 1]).is_some()
    {
        (cursor_col - 1, cur_chars[cursor_col - 1])
    } else {
        return None;
    };

    let (partner_ch, forward) = matching_partner(ch)?;
    let origin = (cursor_line, origin_col);

    let max_lines = 2000;
    if forward {
        let mut depth = 1usize;
        let mut start_c = origin_col + 1;
        let end_line = (cursor_line + max_lines).min(lines.len());
        for (l, line) in lines.iter().enumerate().take(end_line).skip(cursor_line) {
            for (c_idx, c) in line.chars().enumerate().skip(start_c) {
                if c == ch {
                    depth += 1;
                } else if c == partner_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some(BracketMatch {
                            origin,
                            partner: Some((l, c_idx)),
                        });
                    }
                }
            }
            start_c = 0;
        }
    } else {
        let mut depth = 1usize;
        let start_c = origin_col;
        let min_line = cursor_line.saturating_sub(max_lines);
        for l in (min_line..=cursor_line).rev() {
            let line_chars: Vec<char> = lines[l].chars().collect();
            let from = if l == cursor_line { start_c } else { line_chars.len() };
            for c_idx in (0..from).rev() {
                let c = line_chars[c_idx];
                if c == ch {
                    depth += 1;
                } else if c == partner_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some(BracketMatch {
                            origin,
                            partner: Some((l, c_idx)),
                        });
                    }
                }
            }
        }
    }

    Some(BracketMatch {
        origin,
        partner: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_to_byte_and_char_count() {
        let ascii = "hello world";
        assert_eq!(char_to_byte(ascii, 5), 5);
        assert_eq!(line_char_count(ascii), 11);

        let unicode = "héllo wörld";
        assert_eq!(line_char_count(unicode), 11);
        assert_eq!(char_to_byte(unicode, 1), 1);
        assert_eq!(char_to_byte(unicode, 2), 3); // 'é' is 2 bytes
    }

    #[test]
    fn test_word_bounds_and_navigation() {
        let lines = vec!["fn main() {".to_string(), "    let x = 42;".to_string()];
        assert_eq!(next_word_pos(&lines, 0, 0), (0, 2)); // after "fn"
        assert_eq!(next_word_pos(&lines, 0, 2), (0, 3)); // skip " "
        assert_eq!(next_word_pos(&lines, 0, 3), (0, 7)); // after "main"

        assert_eq!(prev_word_pos(&lines, 0, 7), (0, 3)); // start of "main"
        assert_eq!(prev_word_pos(&lines, 0, 3), (0, 0)); // start of "fn"
        assert_eq!(prev_word_pos(&lines, 1, 14), (1, 12)); // start of "42" (from before ';')
        assert_eq!(prev_word_pos(&lines, 1, 12), (1, 10)); // start of "="
    }

    #[test]
    fn test_visual_col_and_tab_expansion() {
        // Line with tabs: "\tfoo\tbar"
        let line = "\tfoo\tbar";
        // Col 0: before first tab -> vcol 0
        assert_eq!(visual_col_at(line, 0, 4), 0);
        // Col 1: after first tab -> vcol 4 (tab takes 4 columns)
        assert_eq!(visual_col_at(line, 1, 4), 4);
        // Col 2: 'f' -> vcol 5
        assert_eq!(visual_col_at(line, 2, 4), 5);
        // Col 4: after "foo" -> vcol 7
        assert_eq!(visual_col_at(line, 4, 4), 7);
        // Col 5: after second tab -> vcol 8 (next tab stop multiple of 4)
        assert_eq!(visual_col_at(line, 5, 4), 8);
        // Total visual length
        assert_eq!(line_visual_char_count(line, 4), 11);

        // Roundtrip col_from_visual_col
        assert_eq!(col_from_visual_col(line, 0, 4), 0);
        assert_eq!(col_from_visual_col(line, 4, 4), 1);
        assert_eq!(col_from_visual_col(line, 5, 4), 2);
        assert_eq!(col_from_visual_col(line, 7, 4), 4);
        assert_eq!(col_from_visual_col(line, 8, 4), 5);
    }

    #[test]
    fn test_indent_and_dedent() {
        let mut line1 = "    code".to_string();
        assert_eq!(dedent_line(&mut line1), 4);
        assert_eq!(line1, "code");

        let mut line2 = "\tcode".to_string();
        assert_eq!(dedent_line(&mut line2), 1);
        assert_eq!(line2, "code");

        indent_line(&mut line2);
        assert_eq!(line2, "\tcode");
    }

    #[test]
    fn test_toggle_comment() {
        let mut lines = vec!["    let a = 1;".to_string(), "    let b = 2;".to_string()];
        toggle_comment_range(&mut lines, 0, 1, "//");
        assert_eq!(lines[0], "    // let a = 1;");
        assert_eq!(lines[1], "    // let b = 2;");

        toggle_comment_range(&mut lines, 0, 1, "//");
        assert_eq!(lines[0], "    let a = 1;");
        assert_eq!(lines[1], "    let b = 2;");
    }

    #[test]
    fn closing_delimiter_maps_parens_brackets_braces_and_quotes() {
        assert_eq!(closing_delimiter('('), Some(')'));
        assert_eq!(closing_delimiter('['), Some(']'));
        assert_eq!(closing_delimiter('{'), Some('}'));
        assert_eq!(closing_delimiter('"'), Some('"'));
        assert_eq!(closing_delimiter('\''), Some('\''));
        assert_eq!(closing_delimiter('`'), Some('`'));
        // Caracteres que no abren un par.
        assert_eq!(closing_delimiter(')'), None);
        assert_eq!(closing_delimiter('a'), None);
        assert_eq!(closing_delimiter(' '), None);
    }

    #[test]
    fn is_closing_delimiter_detects_only_closing_chars() {
        assert!(is_closing_delimiter(')'));
        assert!(is_closing_delimiter(']'));
        assert!(is_closing_delimiter('}'));
        assert!(is_closing_delimiter('"'));
        assert!(is_closing_delimiter('\''));
        assert!(is_closing_delimiter('`'));
        assert!(!is_closing_delimiter('('));
        assert!(!is_closing_delimiter('['));
        assert!(!is_closing_delimiter('{'));
        assert!(!is_closing_delimiter('a'));
    }

    #[test]
    fn is_empty_pair_around_detects_adjacent_empty_pairs() {
        assert!(is_empty_pair_around("()", 1));
        assert!(is_empty_pair_around("[]", 1));
        assert!(is_empty_pair_around("{}", 1));
        assert!(is_empty_pair_around("\"\"", 1));
        assert!(is_empty_pair_around("''", 1));
        assert!(is_empty_pair_around("``", 1));
        // No son pares adyacentes vacíos.
        assert!(!is_empty_pair_around("(x)", 1));
        assert!(!is_empty_pair_around("()", 0)); // col 0: no hay char previo
        assert!(!is_empty_pair_around("", 0));
        assert!(!is_empty_pair_around("()", 2)); // fin de línea: no hay char posterior
        assert!(!is_empty_pair_around("x(", 1));
    }

    #[test]
    fn test_matching_brackets() {
        let lines = vec!["fn foo(x: [i32]) -> (i32) {".to_string(), "    return x[0];".to_string(), "}".to_string()];
        // '(' at (0, 6) matches ')' at (0, 15)
        let m = find_matching_bracket(&lines, 0, 6).expect("should match");
        assert_eq!(m.partner, Some((0, 15)));

        // '{' at (0, 26) matches '}' at (2, 0)
        let m = find_matching_bracket(&lines, 0, 26).expect("should match");
        assert_eq!(m.partner, Some((2, 0)));
    }
}
