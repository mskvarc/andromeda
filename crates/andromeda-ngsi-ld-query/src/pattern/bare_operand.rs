//! Where a pattern written bare after `~=` or `!~=` ends, and which patterns read back as themselves written that way.
//!
//! Clause 4.9 writes `QueryTerm =/ Attribute patternOp RegExp` with no quotes around the `RegExp` and does not say where it ends, while `;`, `|` and `)` are both query syntax and pattern syntax. Inside a group or a bracket expression of the pattern they belong to the pattern. At its top level they end it, because a client can always group an alternation or bracket a literal, but has no other way to end a pattern term and join the next one. The JSON whitespace around a bare pattern belongs to the expression, as it does around every other operand, and the whitespace inside it belongs to the pattern.

use crate::{
    pattern::{bracket_expression::query_term_scan, pattern_error::PatternError},
    query_text::whitespace::Whitespace,
};

/// The byte length of the bare pattern that opens `text`, which starts after the whitespace before the pattern.
///
/// A trailing run of JSON whitespace is left to the expression, unless an escape holds it.
///
/// # Errors
///
/// Returns [`PatternError::NonPosixConstruct`] for a bracket expression of the pattern that names no single character or opens no character class it can name.
pub(crate) fn bare_pattern_length(text: &str) -> Result<usize, PatternError> {
    let scan = query_term_scan(text)?;
    let unescaped = &text[scan.escaped_through..scan.length];
    let kept = unescaped.trim_end_matches(|character| Whitespace::Json.contains(character));
    Ok(scan.escaped_through + kept.len())
}

/// Whether `pattern`, written bare after `~=` or `!~=`, reads back as itself.
///
/// An empty pattern, one that opens with a quotation mark or JSON whitespace, and one a `q` filter would end early do not.
pub(crate) fn reads_back_bare(pattern: &str) -> bool {
    match pattern.chars().next() {
        None | Some('"') => false,
        Some(first) if Whitespace::Json.contains(first) => false,
        Some(_) => bare_pattern_length(pattern).is_ok_and(|length| length == pattern.len()),
    }
}

#[cfg(test)]
mod tests {
    use crate::pattern::bare_operand::{bare_pattern_length, reads_back_bare};
    use pretty_assertions::assert_eq;

    /// The bare pattern that opens `text`, or `None` when its scan refuses it.
    fn bare(text: &str) -> Option<&str> {
        bare_pattern_length(text).ok().map(|length| &text[..length])
    }

    #[test]
    fn a_bare_pattern_ends_at_a_top_level_delimiter_and_runs_through_its_groups_and_bracket_expressions() {
        for (text, pattern) in [
            ("^Merc;speed>50", "^Merc"),
            ("^Merc|speed>50", "^Merc"),
            ("^Merc)|speed>50", "^Merc"),
            ("^Mer(c|z)edes$|x", "^Mer(c|z)edes$"),
            ("(^Merc|^BMW);x", "(^Merc|^BMW)"),
            ("[;|)]x;y", "[;|)]x"),
            ("[]|]x|y", "[]|]x"),
            ("(?i)^merc;x", "(?i)^merc"),
            ("(?i:a|b)c|d", "(?i:a|b)c"),
            ("a{2,3},b;c", "a{2,3},b"),
            ("a\"b;c", "a\"b"),
            ("Mercedes Benz", "Mercedes Benz"),
            ("^Merc", "^Merc"),
        ] {
            assert_eq!(bare(text), Some(pattern), "{text}");
        }
    }

    #[test]
    fn an_escaped_delimiter_belongs_to_the_pattern() {
        for (text, pattern) in [(r"a\;b;c", r"a\;b"), (r"a\|b|c", r"a\|b"), (r"a\)b)c", r"a\)b")] {
            assert_eq!(bare(text), Some(pattern), "{text}");
        }
    }

    /// A comment of `(?x)` runs to the end of its line, which in a `q` filter is the end of the query, so it shelters no delimiter.
    #[test]
    fn a_comment_shelters_no_delimiter_at_the_top_level() {
        assert_eq!(bare("(?x)a #;b"), Some("(?x)a #"));
        assert_eq!(bare("(?x)a #|b"), Some("(?x)a #"));
    }

    #[test]
    fn whitespace_after_a_bare_pattern_belongs_to_the_expression_and_inside_it_to_the_pattern() {
        for (text, pattern) in [
            ("^M ;speed>50", "^M"),
            ("^M \t\n\r)", "^M"),
            ("Mercedes Benz ;x", "Mercedes Benz"),
            (r"a\ ;x", r"a\ "),
            (r"a\  ;x", r"a\ "),
            (r"a\. b ;x", r"a\. b"),
        ] {
            assert_eq!(bare(text), Some(pattern), "{text}");
        }
    }

    #[test]
    fn a_pattern_that_holds_no_character_before_a_delimiter_is_empty() {
        for text in ["", ";x", "|x", ")x"] {
            assert_eq!(bare(text), Some(""), "{text}");
        }
    }

    #[test]
    fn an_unclosed_group_or_bracket_expression_runs_to_the_end_of_the_text() {
        for text in ["(a;b", "[a;b", "(a|b"] {
            assert_eq!(bare(text), Some(text), "{text}");
        }
    }

    #[test]
    fn a_pattern_reads_back_bare_unless_it_is_empty_opens_with_a_quote_or_whitespace_or_ends_early() {
        for pattern in ["^Merc", "^Mer(c|z)edes$", "[;|)]x", r"a\;b", r"a\ ", "Mercedes Benz", "a\"b", r"^\w+$"] {
            assert!(reads_back_bare(pattern), "{pattern} reads back bare");
        }
        for pattern in ["", "\"x", " a", "a ", "a\t", "a|b", "a;b", "a)b", "(?x)a #;b"] {
            assert!(!reads_back_bare(pattern), "{pattern} is written quoted");
        }
    }
}
