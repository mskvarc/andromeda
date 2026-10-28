//! How a migration's text ends: in exactly one newline, or not at all when it
//! is empty.
//!
//! `pg_format` follows every statement with a blank line, the last one
//! included, and no option disables it, so the blank lines after the last
//! statement are dropped once the text is formatted.

use std::borrow::Cow;

/// `sql` ending in a newline, which `pg_format` is given its input with.
#[must_use]
pub fn with_final_newline(sql: &str) -> Cow<'_, str> {
    if sql.is_empty() || sql.ends_with('\n') {
        Cow::Borrowed(sql)
    } else {
        Cow::Owned(format!("{sql}\n"))
    }
}

/// `sql` ending in exactly one newline, or empty when it holds only newlines.
#[must_use]
pub fn without_trailing_blank_lines(sql: &str) -> String {
    match sql.trim_end_matches('\n') {
        "" => String::new(),
        text => format!("{text}\n"),
    }
}

#[cfg(test)]
mod tests {
    use crate::file_ending::{with_final_newline, without_trailing_blank_lines};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_text_with_trailing_blank_lines_ends_in_exactly_one_newline() {
        assert_eq!(without_trailing_blank_lines("select 1;\n\n\n"), "select 1;\n");
        assert_eq!(without_trailing_blank_lines("select 1;"), "select 1;\n");
        assert_eq!(without_trailing_blank_lines("select 1;\n"), "select 1;\n");
    }

    #[test]
    fn blank_lines_inside_the_text_are_kept() {
        assert_eq!(without_trailing_blank_lines("select 1;\n\nselect 2;\n\n"), "select 1;\n\nselect 2;\n");
    }

    #[test]
    fn a_text_of_newlines_alone_becomes_empty() {
        assert_eq!(without_trailing_blank_lines("\n\n"), "");
        assert_eq!(without_trailing_blank_lines(""), "");
    }

    #[test]
    fn a_final_newline_is_added_only_where_missing() {
        assert_eq!(with_final_newline("select 1;"), "select 1;\n");
        assert_eq!(with_final_newline("select 1;\n\n"), "select 1;\n\n");
        assert_eq!(with_final_newline(""), "");
    }
}
