//! The comment-text rules: no spaced double hyphen and no em or en dash in any
//! comment, and no clone note that refers to another line or stands on nothing.

use crate::{
    comment_lexer::{CommentKind, lex},
    source_scan::source_tree_paths,
};
use std::{fs, path::Path};

/// Assert that no comment in the workspace breaks the comment-text rules.
///
/// The scan covers every Rust file below `src/`, `crates/` and `tools/`, test code
/// included: the dash rule holds for every comment, and the clone-note rule holds
/// for test code as well.
///
/// # Panics
///
/// Panics when a source file cannot be read, or lists every violation with its
/// `path:line` otherwise.
pub fn assert_comment_hygiene(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    let mut unreadable = Vec::new();
    for path in source_tree_paths(root) {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        match fs::read_to_string(&path) {
            Ok(source) => violations.extend(violations_in(&relative, &source)),
            Err(error) => unreadable.push(format!("{relative} could not be read: {error}")),
        }
    }
    assert!(unreadable.is_empty(), "comment sources could not be scanned:\n{}", unreadable.join("\n"));
    assert!(violations.is_empty(), "comment-text violations:\n{}", violations.join("\n"));
}

/// Every comment rule violation in one source file, as `path:line: message` lines.
fn violations_in(relative: &str, source: &str) -> Vec<String> {
    let lexed = lex(source);
    let code_lines: Vec<&str> = lexed.code.lines().collect();
    let regions = test_regions(&code_lines);
    let test_path = is_test_path(relative);
    let mut found = Vec::new();
    for comment in &lexed.comments {
        if has_forbidden_dash(&comment.text) {
            found.push(format!("{relative}:{}: spaced double hyphen or em or en dash in a comment", comment.line));
        }
        if comment.kind == CommentKind::Plain
            && let Some(note) = comment.text.trim().strip_prefix("clone:")
        {
            let note = note.trim();
            if test_path || in_regions(comment.line, &regions) {
                found.push(format!("{relative}:{}: clone note in test code", comment.line));
            } else if is_stub_clone_note(note) {
                found.push(format!(
                    "{relative}:{}: clone note refers to another line or to a handle the type shows",
                    comment.line
                ));
            }
        }
    }
    found
}

/// Whether a comment holds an em dash, an en dash, or a `--` set off by whitespace.
fn has_forbidden_dash(text: &str) -> bool {
    text.contains(['\u{2014}', '\u{2013}']) || spaced_double_hyphen(text)
}

/// Whether `text` holds a `--` that is preceded and followed by whitespace or an edge.
fn spaced_double_hyphen(text: &str) -> bool {
    text.match_indices("--").any(|(position, _)| {
        let before = text[..position].chars().next_back();
        let after = text[position + 2..].chars().next();
        before.is_none_or(char::is_whitespace) && after.is_none_or(char::is_whitespace)
    })
}

/// Whether a clone note refers to another line, or names a handle that the type
/// already shows, instead of stating the reason the copy is needed.
fn is_stub_clone_note(note: &str) -> bool {
    let words: Vec<&str> = note
        .split(|character: char| !character.is_alphanumeric() && character != '-')
        .filter(|word| !word.is_empty())
        .collect();
    words.first() == Some(&"same")
        || words
            .iter()
            .any(|word| matches!(*word, "above" | "below" | "Arc" | "Rc" | "ref-count" | "reference-counted"))
}

/// Whether a source path belongs to integration test code.
fn is_test_path(relative: &str) -> bool {
    relative.contains("/tests/") || relative.starts_with("tests/") || relative.ends_with("tests.rs")
}

/// Inclusive 1-based line ranges of the items compiled only under `cfg(test)`.
fn test_regions(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if !lines[index].trim_start().starts_with("#[cfg(test)]") {
            index += 1;
            continue;
        }
        let mut start = index + 1;
        while start < lines.len() && (lines[start].trim().is_empty() || lines[start].trim_start().starts_with("#[")) {
            start += 1;
        }
        let end = item_end(lines, start);
        regions.push((index + 1, end + 1));
        index = end + 1;
    }
    regions
}

/// Whether a 1-based line number falls inside one of the ranges.
fn in_regions(line: usize, regions: &[(usize, usize)]) -> bool {
    regions.iter().any(|&(first, last)| first <= line && line <= last)
}

/// The index of the last line of the item that begins at line index `start`.
///
/// A struct field has no body, so it runs to the first line ending in a comma. Every
/// other item runs to its `;` or to the `}` that closes its first `{`.
fn item_end(lines: &[&str], start: usize) -> usize {
    if lines.get(start).is_some_and(|line| is_field_line(line)) {
        return (start..lines.len())
            .find(|&index| lines[index].trim_end().ends_with(','))
            .unwrap_or(lines.len() - 1);
    }
    braced_or_semicolon_end(lines, start)
}

/// Whether a line declares a named struct field, optionally with a visibility.
fn is_field_line(line: &str) -> bool {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if rest.starts_with('(') {
            let Some(close) = rest.find(')') else {
                return false;
            };
            rest = rest[close + 1..].trim_start();
        }
    }
    let identifier = rest
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .next()
        .unwrap_or("");
    if identifier.is_empty() {
        return false;
    }
    let after = rest[identifier.len()..].trim_start();
    after.starts_with(':') && !after.starts_with("::")
}

/// The index of the line that ends the braced item, or of the `;` that ends a
/// bodiless one, scanning from line index `start`.
fn braced_or_semicolon_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0;
    let mut opened = false;
    for (index, line) in lines.iter().enumerate().skip(start) {
        for character in line.chars() {
            if !opened {
                if character == ';' {
                    return index;
                }
                if character == '{' {
                    opened = true;
                    depth = 1;
                }
            } else if character == '{' {
                depth += 1;
            } else if character == '}' {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
        }
    }
    lines.len() - 1
}

#[cfg(test)]
mod tests {
    use crate::comment_hygiene::{has_forbidden_dash, is_stub_clone_note, test_regions, violations_in};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_spaced_double_hyphen_is_a_forbidden_dash() {
        assert!(has_forbidden_dash(" the lock -- held across the await"));
    }

    #[test]
    fn a_double_hyphen_inside_a_word_is_not_a_dash() {
        assert!(!has_forbidden_dash(" the `--flag` option and a-- b"));
    }

    #[test]
    fn an_em_or_en_dash_is_a_forbidden_dash() {
        assert!(has_forbidden_dash(" the report \u{2014} the error"));
        assert!(has_forbidden_dash(" the 503 \u{2013} the error"));
    }

    #[test]
    fn a_clone_note_that_points_at_another_line_is_a_stub() {
        assert!(is_stub_clone_note("as above"));
        assert!(is_stub_clone_note("same reason"));
        assert!(is_stub_clone_note("compared below"));
    }

    #[test]
    fn a_clone_note_naming_a_shared_handle_is_a_stub() {
        assert!(is_stub_clone_note("Arc ref-count bump"));
        assert!(is_stub_clone_note("SQLx pools are shared reference-counted handles"));
    }

    #[test]
    fn a_clone_note_stating_a_lifetime_reason_is_kept() {
        assert!(!is_stub_clone_note("the error outlives this borrow"));
    }

    #[test]
    fn a_clone_note_in_an_integration_test_file_is_a_violation() {
        let found = violations_in(
            "tools/andromeda-test-suite/tests/a.rs",
            "fn f() {\n    // clone: the error outlives this borrow\n    let a = b.clone();\n}\n",
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_clone_note_after_a_test_module_is_a_violation_only_inside_it() {
        let source = "#[cfg(test)]\nmod tests {\n    fn t() {\n        // clone: the error outlives this borrow\n    }\n}\n// clone: the error outlives this borrow\nfn production() {}\n";
        let found = violations_in("crates/andromeda-x/src/lib.rs", source);
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("crates/andromeda-x/src/lib.rs:4:"));
    }

    #[test]
    fn a_single_item_test_attribute_does_not_open_a_region() {
        let lines = ["#[cfg(test)]", "mod tests;", "fn production() {", "}"];
        assert_eq!(test_regions(&lines), vec![(1, 2)]);
    }

    #[test]
    fn a_test_only_field_ends_at_its_comma() {
        let lines = ["struct S {", "    #[cfg(test)]", "    count: Arc<u64>,", "    other: u8,", "}"];
        assert_eq!(test_regions(&lines), vec![(2, 3)]);
    }

    #[test]
    fn a_braced_test_module_ends_at_its_closing_brace() {
        let lines = ["#[cfg(test)]", "mod tests {", "    fn t() {", "    }", "}", "fn production() {}"];
        assert_eq!(test_regions(&lines), vec![(1, 5)]);
    }
}
