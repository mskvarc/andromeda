// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! The two operations RFC 8288's quoting rules force on a parser.
//!
//! This file supports the adapted `Link` implementation derived from
//! <https://github.com/hyperium/headers> and is distributed under the MIT License.
//!
//! A `Link` header separates link-values with `,` and parameters with `;`, but
//! either character may appear inside a quoted parameter value, so neither
//! `str::split` nor `str::trim_matches` can be used directly.

use crate::link::error::LinkParseError;

/// Splits a string on the given delimiter characters, ignoring any that fall
/// inside a double-quoted run or inside a `<...>` target.
///
/// RFC 8288 section 3 writes a target as `"<" URI-Reference ">"`, and a URI
/// reference may carry `,` and `;` (RFC 3986 section 2.2), so neither ends a
/// link-value or a parameter while the target is open.
pub struct SplitAsciiUnquoted<'a> {
    /// The text being split.
    source: &'a str,
    /// How far into `source` the next segment starts.
    position: usize,
    /// The delimiter characters, as a byte set.
    delimiters: &'a str,
}

impl<'a> SplitAsciiUnquoted<'a> {
    /// Split `source` on any character of `delimiters` that is not quoted.
    #[must_use]
    pub const fn new(source: &'a str, delimiters: &'a str) -> SplitAsciiUnquoted<'a> {
        SplitAsciiUnquoted {
            source,
            position: 0,
            delimiters,
        }
    }
}

impl<'a> Iterator for SplitAsciiUnquoted<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.position >= self.source.len() {
            return None;
        }
        let start = self.position;
        let mut end = self.position;
        let mut in_quotes = false;
        let mut in_target = false;
        let mut escaped = false;

        for byte in &self.source.as_bytes()[start..] {
            if escaped {
                // The target of an RFC 8288 quoted-pair (`\`-escaped): data,
                // never a quote toggle or a delimiter.
                escaped = false;
            } else if in_quotes && *byte == b'\\' {
                // A backslash inside a quoted string opens a quoted-pair, so the
                // next byte, including `\"`, does not end the quote.
                escaped = true;
            } else if in_target {
                in_target = *byte != b'>';
            } else if *byte == b'"' {
                in_quotes = !in_quotes;
            } else if !in_quotes && *byte == b'<' {
                in_target = true;
            } else if !in_quotes && self.delimiters.as_bytes().contains(byte) {
                break;
            }
            end += 1;
        }

        self.position = end + 1;
        Some(&self.source[start..end])
    }
}

/// Verify that `raw` opens with `open` and closes with `close`, returning the
/// inner text with those delimiters and surrounding whitespace trimmed.
///
/// # Errors
///
/// Returns [`LinkParseError`] when the delimiters are absent or unbalanced.
pub fn verify_and_trim(raw: &str, open: u8, close: u8) -> Result<&str, LinkParseError> {
    let bytes = raw.as_bytes();
    if bytes.len() > 1 && bytes[0] == open && bytes[bytes.len() - 1] == close {
        // Only the one outer pair is stripped, so `<<url>>` keeps its inner
        // brackets. `open`/`close` are single ASCII bytes, so slicing off byte 0
        // and the last byte lands on char boundaries.
        Ok(raw[1..raw.len() - 1].trim())
    } else {
        Err(LinkParseError)
    }
}

#[cfg(test)]
mod tests {
    use crate::link::quoting::{SplitAsciiUnquoted, verify_and_trim};

    #[test]
    fn a_delimiter_inside_quotes_does_not_split() {
        let segments: Vec<&str> = SplitAsciiUnquoted::new("some, text; \"and, more; in quotes\", or not", ";,").collect();
        pretty_assertions::assert_eq!(segments, vec!["some", " text", " \"and, more; in quotes\"", " or not"]);
    }

    /// RFC 3986 section 2.2 lets a target carry `,` and `;`, so neither splits
    /// while the `<...>` target is open.
    #[test]
    fn a_delimiter_inside_a_target_does_not_split() {
        let raw = r#"</entities?type=A,B;C>; rel="next", <https://example.org/a.jsonld>"#;
        let segments: Vec<&str> = SplitAsciiUnquoted::new(raw, ",").collect();
        pretty_assertions::assert_eq!(segments, vec![r#"</entities?type=A,B;C>; rel="next""#, " <https://example.org/a.jsonld>"]);
    }

    #[test]
    fn a_matched_delimiter_pair_is_trimmed_with_its_whitespace() {
        pretty_assertions::assert_eq!(verify_and_trim(">  some string   <", b'>', b'<').ok(), Some("some string"));
    }

    #[test]
    fn a_leading_character_before_the_opening_delimiter_is_rejected() {
        assert!(verify_and_trim(" >  some string   <", b'>', b'<').is_err());
    }

    #[test]
    fn an_escaped_quote_inside_a_quoted_value_does_not_end_the_quote() {
        // RFC 8288 quoted-pair: `\"` is an escaped quote (data inside the
        // value), so it must not toggle quote state and let a following `;`
        // split the segment.
        let raw = r#"rel="next"; title="a\"b; c""#;
        let segments: Vec<&str> = SplitAsciiUnquoted::new(raw, ";,").collect();
        pretty_assertions::assert_eq!(segments, vec![r#"rel="next""#, r#" title="a\"b; c""#]);
    }

    #[test]
    fn only_the_single_outer_delimiter_pair_is_stripped() {
        // A value that legitimately begins or ends with the delimiter character
        // keeps it: only the one outer pair is removed.
        pretty_assertions::assert_eq!(verify_and_trim("<<url>>", b'<', b'>').ok(), Some("<url>"));
    }
}
