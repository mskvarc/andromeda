//! The delimiters of a parameter's own syntax that stand outside the quoted
//! names it carries.
//!
//! `orderBy` and `aggrGroupBy` separate their members with `,` and a member's
//! direction or bucket with `;`. A member names attributes along a clause 4.9
//! value path, where a name no `AttrName` spells is a `quotedStr` whose value,
//! an absolute IRI, may hold either mark. A mark inside such a string belongs
//! to the name, so a split of those parameters skips every JSON string of the
//! text: a string runs from a `"` to the next `"` that no backslash escapes,
//! and one left open runs to the end of the text.

use std::iter;

/// Where a scan of a text stands relative to its JSON strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quoting {
    /// Between strings, where a delimiter separates.
    Outside,
    /// Inside a string.
    Inside,
    /// Inside a string, right after the backslash that escapes the next
    /// character.
    Escaped,
}

impl Quoting {
    /// Where the scan stands after `character`.
    const fn after(self, character: char) -> Quoting {
        match (self, character) {
            (Quoting::Outside, '"') | (Quoting::Escaped, _) => Quoting::Inside,
            (Quoting::Inside, '\\') => Quoting::Escaped,
            (Quoting::Inside, '"') => Quoting::Outside,
            (unchanged, _) => unchanged,
        }
    }
}

/// The byte offset of the first `delimiter` in `text` outside a JSON string.
fn first_unquoted(text: &str, delimiter: char) -> Option<usize> {
    text.char_indices()
        .scan(Quoting::Outside, |quoting, (offset, character)| {
            let separates = *quoting == Quoting::Outside && character == delimiter;
            *quoting = quoting.after(character);
            Some(separates.then_some(offset))
        })
        .flatten()
        .next()
}

/// `text` before and after its first `delimiter` outside a JSON string, or
/// `None` when every `delimiter` it holds stands inside one.
pub(crate) fn split_once_unquoted(text: &str, delimiter: char) -> Option<(&str, &str)> {
    let (before, from_delimiter) = text.split_at_checked(first_unquoted(text, delimiter)?)?;
    Some((before, from_delimiter.strip_prefix(delimiter)?))
}

/// The parts of `text` between its `delimiter`s outside JSON strings, in
/// order; a text with no such delimiter is one part.
pub(crate) fn split_unquoted(text: &str, delimiter: char) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    iter::from_fn(move || {
        let current = rest?;
        // A part ends outside every string, so the rest is scanned from
        // outside one.
        let (part, after) = split_once_unquoted(current, delimiter).map_or((current, None), |(part, after)| (part, Some(after)));
        rest = after;
        Some(part)
    })
}

#[cfg(test)]
mod tests {
    use crate::query_text::unquoted_delimiter::{split_once_unquoted, split_unquoted};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_delimiter_outside_every_string_separates() {
        assert_eq!(split_unquoted("a,b , c", ',').collect::<Vec<_>>(), ["a", "b ", " c"]);
        assert_eq!(split_once_unquoted("name;desc", ';'), Some(("name", "desc")));
    }

    #[test]
    fn a_delimiter_inside_a_quoted_name_belongs_to_the_name() {
        assert_eq!(
            split_unquoted(r#""urn:x:(a,b)";desc,"urn:x:a;b""#, ',').collect::<Vec<_>>(),
            [r#""urn:x:(a,b)";desc"#, r#""urn:x:a;b""#]
        );
        assert_eq!(
            split_once_unquoted(r#""urn:x:a;b".observedAt;month"#, ';'),
            Some((r#""urn:x:a;b".observedAt"#, "month"))
        );
        assert_eq!(split_once_unquoted(r#""urn:x:a;b""#, ';'), None);
    }

    #[test]
    fn an_escaped_quotation_mark_leaves_the_string_open() {
        assert_eq!(split_unquoted(r#""a\",b",c"#, ',').collect::<Vec<_>>(), [r#""a\",b""#, "c"]);
        assert_eq!(split_unquoted(r#""a\\",b"#, ',').collect::<Vec<_>>(), [r#""a\\""#, "b"]);
    }

    #[test]
    fn a_string_left_open_runs_to_the_end_of_the_text() {
        assert_eq!(split_unquoted(r#"a,"b,c"#, ',').collect::<Vec<_>>(), ["a", r#""b,c"#]);
    }

    #[test]
    fn empty_parts_are_kept_where_the_delimiters_stand() {
        assert_eq!(split_unquoted("", ',').collect::<Vec<_>>(), [""]);
        assert_eq!(split_unquoted(",a,,", ',').collect::<Vec<_>>(), ["", "a", "", ""]);
    }
}
