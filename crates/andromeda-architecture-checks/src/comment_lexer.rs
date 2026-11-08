//! Splits Rust source into its code and its comments, for the comment-text gate.

/// The comment syntax a piece of comment text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommentKind {
    /// An outer doc line, `///`.
    Outer,
    /// An inner doc line, `//!`.
    Inner,
    /// A plain line comment, `//`.
    Plain,
    /// A block comment, `/* */`, nesting included.
    Block,
}

/// One comment found in a source file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Comment {
    /// The 1-based line the comment opens on.
    pub(crate) line: usize,
    /// The syntax the comment was written in.
    pub(crate) kind: CommentKind,
    /// The text after the opening marker, without the closing marker of a block comment.
    pub(crate) text: String,
}

/// A source file with its comments blanked out and the comments listed in order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Lexed {
    /// The source with every comment replaced by spaces, newlines kept, so line numbers match.
    pub(crate) code: String,
    /// Every comment, in source order.
    pub(crate) comments: Vec<Comment>,
}

/// Split one Rust source into its code and its comments.
///
/// String, raw string, byte string and character literals are copied to the code
/// unchanged, so a `//` inside a literal is not a comment. A lifetime such as
/// `'a` is code.
pub(crate) fn lex(source: &str) -> Lexed {
    let chars: Vec<char> = source.chars().collect();
    let mut code = String::with_capacity(source.len());
    let mut comments = Vec::new();
    let mut line = 1;
    let mut index = 0;
    while let Some(&current) = chars.get(index) {
        if current == '\n' {
            line += 1;
            code.push('\n');
            index += 1;
        } else if let Some(end) = raw_string_end(&chars, index) {
            copy_span(&chars, index, end, &mut code);
            line += newlines(&chars, index, end);
            index = end;
        } else if current == '"' {
            let end = string_end(&chars, index);
            copy_span(&chars, index, end, &mut code);
            line += newlines(&chars, index, end);
            index = end;
        } else if current == '\'' {
            if let Some(end) = char_literal_end(&chars, index) {
                copy_span(&chars, index, end, &mut code);
                index = end;
            } else {
                code.push('\'');
                index += 1;
            }
        } else if starts_with(&chars, index, "//") {
            let end = line_end(&chars, index);
            let text: String = chars[index..end].iter().collect();
            comments.push(line_comment(line, &text));
            blank_span(&chars, index, end, &mut code);
            index = end;
        } else if starts_with(&chars, index, "/*") {
            let end = block_end(&chars, index);
            let text: String = chars[index..end].iter().collect();
            let body = text.strip_prefix("/*").unwrap_or_default();
            let body = body.strip_suffix("*/").unwrap_or(body);
            comments.push(Comment {
                line,
                kind: CommentKind::Block,
                text: body.to_owned(),
            });
            line += newlines(&chars, index, end);
            blank_span(&chars, index, end, &mut code);
            index = end;
        } else {
            code.push(current);
            index += 1;
        }
    }
    Lexed { code, comments }
}

/// The comment a `//` line opens, classified by its marker.
fn line_comment(line: usize, text: &str) -> Comment {
    let (kind, body) = if let Some(body) = text.strip_prefix("///").filter(|_| !text.starts_with("////")) {
        (CommentKind::Outer, body)
    } else if let Some(body) = text.strip_prefix("//!") {
        (CommentKind::Inner, body)
    } else {
        (CommentKind::Plain, text.strip_prefix("//").unwrap_or_default())
    };
    Comment {
        line,
        kind,
        text: body.to_owned(),
    }
}

/// Whether `pattern` starts at `index`.
fn starts_with(chars: &[char], index: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(index + offset) == Some(&expected))
}

/// Whether a character can continue an identifier.
fn is_identifier(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// The index just past a raw string or byte raw string that opens at `index`.
fn raw_string_end(chars: &[char], index: usize) -> Option<usize> {
    let prefix = if chars.get(index) == Some(&'b') { index + 1 } else { index };
    if chars.get(prefix) != Some(&'r') || (index > 0 && chars.get(index - 1).is_some_and(|&c| is_identifier(c))) {
        return None;
    }
    let mut cursor = prefix + 1;
    let mut hashes = 0;
    while chars.get(cursor) == Some(&'#') {
        hashes += 1;
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'"') {
        return None;
    }
    cursor += 1;
    while cursor < chars.len() {
        if chars[cursor] == '"' && (1..=hashes).all(|offset| chars.get(cursor + offset) == Some(&'#')) {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    Some(chars.len())
}

/// The index just past an ordinary string literal that opens at `index`.
fn string_end(chars: &[char], index: usize) -> usize {
    let mut cursor = index + 1;
    while let Some(&current) = chars.get(cursor) {
        if current == '\\' {
            cursor += 2;
        } else if current == '"' {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    chars.len()
}

/// The index just past a character literal that opens at `index`, or `None` when
/// the quote opens a lifetime or a label.
fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index + 1) == Some(&'\\') {
        let close = (index + 3..chars.len()).find(|&cursor| chars[cursor] == '\'');
        return Some(close.map_or(index + 1, |cursor| cursor + 1));
    }
    (chars.get(index + 2) == Some(&'\'')).then_some(index + 3)
}

/// The index of the newline ending the line that contains `index`, or the end of input.
fn line_end(chars: &[char], index: usize) -> usize {
    (index..chars.len()).find(|&cursor| chars[cursor] == '\n').unwrap_or(chars.len())
}

/// The index just past the block comment that opens at `index`, nesting included.
fn block_end(chars: &[char], index: usize) -> usize {
    let mut depth = 0;
    let mut cursor = index;
    while cursor < chars.len() {
        if starts_with(chars, cursor, "/*") {
            depth += 1;
            cursor += 2;
        } else if starts_with(chars, cursor, "*/") {
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return cursor;
            }
        } else {
            cursor += 1;
        }
    }
    chars.len()
}

/// Copy `chars[start..end]` into the code.
fn copy_span(chars: &[char], start: usize, end: usize, code: &mut String) {
    code.extend(&chars[start..end]);
}

/// Replace `chars[start..end]` in the code with spaces, keeping newlines.
fn blank_span(chars: &[char], start: usize, end: usize, code: &mut String) {
    code.extend(chars[start..end].iter().map(|&c| if c == '\n' { '\n' } else { ' ' }));
}

/// The number of newlines in `chars[start..end]`.
fn newlines(chars: &[char], start: usize, end: usize) -> usize {
    chars[start..end].iter().filter(|&&c| c == '\n').count()
}

#[cfg(test)]
mod tests {
    use crate::comment_lexer::{Comment, CommentKind, Lexed, lex};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_line_comment_is_blanked_and_recorded() {
        let lexed = lex("let a = 1; // note\nlet b = 2;");
        assert_eq!(lexed.code, format!("let a = 1;{}\nlet b = 2;", " ".repeat(8)));
        assert_eq!(
            lexed.comments,
            [Comment {
                line: 1,
                kind: CommentKind::Plain,
                text: " note".to_owned(),
            }]
        );
    }

    #[test]
    fn doc_lines_are_classified_by_their_marker() {
        let lexed = lex("/// outer\n//! inner\n//// plain\n");
        let kinds: Vec<CommentKind> = lexed.comments.iter().map(|comment| comment.kind).collect();
        assert_eq!(kinds, [CommentKind::Outer, CommentKind::Inner, CommentKind::Plain]);
    }

    #[test]
    fn a_double_slash_inside_a_string_is_code() {
        let lexed = lex(r#"let url = "https://example.test"; // real"#);
        assert_eq!(lexed.comments.len(), 1);
        assert_eq!(lexed.comments[0].text, " real");
    }

    #[test]
    fn a_raw_string_with_hashes_is_code() {
        let lexed = lex(r##"let s = r#"a "// not" b"#; // real"##);
        assert_eq!(lexed.comments.len(), 1);
        assert_eq!(lexed.comments[0].text, " real");
    }

    #[test]
    fn a_lifetime_is_code_and_a_char_literal_is_not_a_comment_opener() {
        let lexed = lex("fn f<'a>(x: &'a char) -> char { '/' } // tail");
        assert_eq!(lexed.comments.len(), 1);
        assert_eq!(lexed.comments[0].text, " tail");
    }

    #[test]
    fn a_block_comment_nests_and_keeps_its_lines() {
        let lexed = lex("a /* one /* two */ still\n */ b");
        assert_eq!(lexed.code, format!("a {}\n    b", " ".repeat(22)));
        assert_eq!(lexed.comments.len(), 1);
        assert_eq!(lexed.comments[0].kind, CommentKind::Block);
        assert_eq!(lexed.comments[0].line, 1);
    }

    #[test]
    fn code_keeps_its_line_numbers_after_a_multiline_comment() {
        let lexed = lex("/* a\nb */\n// c");
        assert_eq!(lexed.comments[1].line, 3);
        assert_eq!(lexed.code.lines().count(), 3);
    }

    #[test]
    fn an_empty_source_has_no_code_and_no_comments() {
        assert_eq!(
            lex(""),
            Lexed {
                code: String::new(),
                comments: Vec::new(),
            }
        );
    }
}
