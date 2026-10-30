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
        while c > 0 {
            c -= 1;
            if !is_word_char(chars[c]) {
                break;
            }
        }
        (line, c)
    }
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
    lines: &mut Vec<String>,
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

    for l in start_l..=end_l {
        let line = &lines[l];
        if line.trim().is_empty() {
            continue;
        }
        if all_commented {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(line, indent_len);
            let rest = &line[indent_bytes..];
            if rest.starts_with(prefix) {
                let strip_len = if rest[prefix.len()..].starts_with(' ') {
                    prefix.len() + 1
                } else {
                    prefix.len()
                };
                lines[l].replace_range(indent_bytes..indent_bytes + strip_len, "");
            }
        } else {
            let indent_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent_bytes = char_to_byte(line, indent_len);
            let insert = format!("{} ", prefix);
            lines[l].insert_str(indent_bytes, &insert);
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
        for l in cursor_line..end_line {
            for (c_idx, c) in lines[l].chars().enumerate().skip(start_c) {
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

        assert_eq!(prev_word_pos(&lines, 0, 7), (0, 2));
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
