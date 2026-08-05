//! The two operations RFC 8288's quoting rules force on a parser.
//!
//! A `Link` header separates link-values with `,` and parameters with `;`, but
//! either character may appear inside a quoted parameter value, so neither
//! `str::split` nor `str::trim_matches` can be used directly.

use crate::link::error::LinkParseError;

/// Splits a string on the given delimiter characters, ignoring any that fall
/// inside a double-quoted run.
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

        for byte in self.source.as_bytes()[start..].iter() {
            in_quotes ^= *byte == b'"';
            if !in_quotes && self.delimiters.as_bytes().contains(byte) {
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
        Ok(raw.trim_matches(|character: char| character == open as char || character == close as char || character.is_whitespace()))
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

    #[test]
    fn a_matched_delimiter_pair_is_trimmed_with_its_whitespace() {
        pretty_assertions::assert_eq!(verify_and_trim(">  some string   <", b'>', b'<').ok(), Some("some string"));
    }

    #[test]
    fn a_leading_character_before_the_opening_delimiter_is_rejected() {
        assert!(verify_and_trim(" >  some string   <", b'>', b'<').is_err());
    }
}
