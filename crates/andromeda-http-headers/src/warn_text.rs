//! The warn-text of one `NGSILD-Warning` value (IETF RFC 7234 section 5.5,
//! carried by ETSI GS CIM 009 v1.9.1 clause 6.3.17).
//!
//! A warn-text is an RFC 7230 quoted-string. Its human text is held as written
//! and quoted only on the wire: a double quote and a backslash become a
//! quoted-pair, and every octet of a character outside visible ASCII is
//! percent-encoded, so the value stays a valid header value whatever the text
//! holds. A text whose quoted form would exceed [`WARN_TEXT_BYTES`] is cut at a
//! character boundary and ends in `...`, so no single warning can outgrow a
//! response header buffer.

use std::fmt::{self, Display, Write};

/// The longest quoted form a warn-text takes on the wire, quotes excluded.
pub const WARN_TEXT_BYTES: usize = 256;

/// What a cut text ends in.
const ELLIPSIS: &str = "...";

/// The human text of one warning, bounded so its quoted form fits
/// [`WARN_TEXT_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display, derive_more::Into)]
pub struct WarnText(String);

impl WarnText {
    /// The warn-text of `text`, cut when its quoted form is too long.
    #[must_use]
    pub fn new(text: impl Display) -> WarnText {
        let mut bounded = BoundedText::default();
        // A `BoundedText` never refuses a write, so formatting fails only when
        // `text` itself does, and the part written before that still stands.
        if write!(bounded, "{text}").is_err() {
            tracing::debug!("a warning text stopped formatting part way");
        }
        bounded.finish()
    }

    /// The warn-text a received quoted-string holds, its quoted-pairs undone;
    /// `quoted` excludes the surrounding quotes.
    #[must_use]
    pub fn from_quoted(quoted: &str) -> WarnText {
        WarnText::new(Unquoted(quoted))
    }

    /// The text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text as the inside of a quoted-string.
    #[must_use]
    pub const fn quoted(&self) -> Quoted<'_> {
        Quoted(self)
    }
}

/// The wire form of a [`WarnText`], without the surrounding quotes.
#[derive(Debug, Clone, Copy)]
pub struct Quoted<'a>(&'a WarnText);

impl Display for Quoted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.0.chars().try_for_each(|character| write_quoted(f, character))
    }
}

/// Write `character` as it stands inside a quoted-string.
fn write_quoted(out: &mut impl Write, character: char) -> fmt::Result {
    match character {
        '"' | '\\' => {
            out.write_char('\\')?;
            out.write_char(character)
        }
        ' '..='~' => out.write_char(character),
        other => other.encode_utf8(&mut [0; 4]).bytes().try_for_each(|octet| write!(out, "%{octet:02X}")),
    }
}

/// How many octets `character` takes inside a quoted-string.
const fn quoted_len(character: char) -> usize {
    match character {
        '"' | '\\' => 2,
        ' '..='~' => 1,
        other => other.len_utf8() * 3,
    }
}

/// A received quoted-string read back to its text: `\x` stands for `x`.
struct Unquoted<'a>(&'a str);

impl Display for Unquoted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut characters = self.0.chars();
        while let Some(character) = characters.next() {
            match character {
                '\\' => characters.next().map_or(Ok(()), |escaped| f.write_char(escaped))?,
                other => f.write_char(other)?,
            }
        }
        Ok(())
    }
}

/// A text being written, which keeps what fits [`WARN_TEXT_BYTES`] once quoted
/// and drops the rest.
#[derive(Default)]
struct BoundedText {
    /// The characters kept.
    text: String,
    /// The quoted length of `text`.
    quoted: usize,
    /// Whether a character was dropped.
    cut: bool,
}

impl BoundedText {
    /// The text kept, ending in [`ELLIPSIS`] when a character was dropped.
    fn finish(mut self) -> WarnText {
        if self.cut {
            while self.quoted + ELLIPSIS.len() > WARN_TEXT_BYTES {
                let Some(dropped) = self.text.pop() else {
                    break;
                };
                self.quoted -= quoted_len(dropped);
            }
            self.text.push_str(ELLIPSIS);
        }
        WarnText(self.text)
    }
}

impl Write for BoundedText {
    fn write_str(&mut self, written: &str) -> fmt::Result {
        for character in written.chars() {
            let cost = quoted_len(character);
            if self.cut || self.quoted + cost > WARN_TEXT_BYTES {
                self.cut = true;
                break;
            }
            self.quoted += cost;
            self.text.push(character);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::warn_text::{WARN_TEXT_BYTES, WarnText};
    use pretty_assertions::assert_eq;

    fn quoted(text: &WarnText) -> String {
        text.quoted().to_string()
    }

    #[test]
    fn plain_ascii_is_written_as_it_is() {
        let text = WarnText::new("context source urn:ngsi-ld:ContextSourceRegistration:1 returned HTTP 503");
        assert_eq!(quoted(&text), "context source urn:ngsi-ld:ContextSourceRegistration:1 returned HTTP 503");
    }

    /// IETF RFC 7230 section 3.2.6: a quote and a backslash inside a
    /// quoted-string are quoted-pairs.
    #[test]
    fn a_quote_and_a_backslash_become_quoted_pairs() {
        let text = WarnText::new(r#"said "no" \ twice"#);
        assert_eq!(quoted(&text), r#"said \"no\" \\ twice"#);
        assert_eq!(text.as_str(), r#"said "no" \ twice"#);
    }

    #[test]
    fn a_character_outside_visible_ascii_is_percent_encoded() {
        assert_eq!(quoted(&WarnText::new("Zürich\r\n")), "Z%C3%BCrich%0D%0A");
    }

    #[test]
    fn a_text_that_fits_is_kept_whole() {
        let text = "a".repeat(WARN_TEXT_BYTES);
        assert_eq!(WarnText::new(&text).as_str(), text);
    }

    #[test]
    fn a_long_text_is_cut_to_the_bound_and_ends_in_an_ellipsis() {
        let cut = quoted(&WarnText::new("a".repeat(147_000)));
        assert_eq!(cut.len(), WARN_TEXT_BYTES);
        assert!(cut.ends_with("aaa..."), "{cut}");
    }

    /// The bound counts the quoted form, so a text of escapes and encoded
    /// characters is cut sooner and never inside one.
    #[test]
    fn the_bound_counts_the_quoted_form_and_cuts_between_characters() {
        let cut = quoted(&WarnText::new("ü\"".repeat(200)));
        assert!(cut.len() <= WARN_TEXT_BYTES, "{}", cut.len());
        assert!(cut.ends_with("%C3%BC\\\"...") || cut.ends_with("\\\"%C3%BC..."), "{cut}");
    }

    #[test]
    fn a_received_quoted_string_reads_back_to_its_text() {
        let text = WarnText::from_quoted(r#"said \"no\" \\ twice"#);
        assert_eq!(text.as_str(), r#"said "no" \ twice"#);
        assert_eq!(quoted(&text), r#"said \"no\" \\ twice"#);
    }

    #[test]
    fn a_received_text_past_the_bound_is_cut() {
        let cut = quoted(&WarnText::from_quoted(&"x".repeat(10_000)));
        assert_eq!(cut.len(), WARN_TEXT_BYTES);
    }

    /// A text relayed through several brokers keeps its spelling: reading a
    /// quoted form back and quoting it again changes nothing.
    #[test]
    fn quoting_a_received_text_again_is_stable() {
        let first = quoted(&WarnText::new("é \"x\" ".repeat(60)));
        let again = quoted(&WarnText::from_quoted(&first));
        assert_eq!(again, first);
    }
}
