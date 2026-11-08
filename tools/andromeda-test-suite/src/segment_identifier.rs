//! Identifiers holding a character a path segment cannot carry verbatim (IETF RFC 3986 section 3.3), each beside the
//! segment it is written as once percent-encoded: a `/` and a `?` would end the segment, a `#` would end the path, a
//! `%` would start an escape, and a non-ASCII character is no URI character at all.

use crate::namespace::Namespace;

/// An identifier of a test and the one path segment that names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentIdentifier {
    /// The identifier as the test writes it.
    pub id: String,
    /// The identifier percent-encoded as one path segment: every octet outside `pchar`, `%` included, written as an
    /// escape.
    pub segment: String,
}

impl SegmentIdentifier {
    /// The identifier `id`, written in a path segment as `segment`.
    const fn new(id: String, segment: String) -> SegmentIdentifier {
        SegmentIdentifier { id, segment }
    }
}

/// The identifiers of `namespace` for a resource of `kind`, one for each character outside `pchar` a path segment
/// cannot carry: a URN holding `/`, a URL with a query, a URL with a fragment, a URN holding the escape `%41`, and a
/// URN holding a non-ASCII character.
#[must_use]
pub fn segment_identifiers(namespace: &Namespace, kind: &str) -> [SegmentIdentifier; 5] {
    let token = namespace.token();
    let url = format!("https://example.org/{token}/{kind}/1");
    let url_segment = format!("https:%2F%2Fexample.org%2F{token}%2F{kind}%2F1");
    [
        SegmentIdentifier::new(namespace.urn_text(kind, "x/y"), namespace.urn_text(kind, "x%2Fy")),
        SegmentIdentifier::new(format!("{url}?x=2"), format!("{url_segment}%3Fx=2")),
        SegmentIdentifier::new(format!("{url}#part"), format!("{url_segment}%23part")),
        SegmentIdentifier::new(namespace.urn_text(kind, "pct%41b"), namespace.urn_text(kind, "pct%2541b")),
        // U+010D is the two UTF-8 octets C4 8D.
        SegmentIdentifier::new(namespace.urn_text(kind, "\u{10d}aj"), namespace.urn_text(kind, "%C4%8Daj")),
    ]
}

#[cfg(test)]
mod tests {
    use crate::{namespace::Namespace, segment_identifier::segment_identifiers};
    use googletest::prelude::*;
    use percent_encoding::percent_decode_str;

    #[gtest]
    fn each_segment_decodes_to_its_identifier() -> Result<()> {
        for identifier in segment_identifiers(&Namespace::fresh(), "Building") {
            verify_that!(percent_decode_str(&identifier.segment).decode_utf8()?, eq(identifier.id.as_str()))?;
        }
        Ok(())
    }

    #[gtest]
    fn no_segment_holds_a_delimiter_or_a_non_ascii_character() -> Result<()> {
        for identifier in segment_identifiers(&Namespace::fresh(), "Building") {
            verify_that!(identifier.segment.contains(['/', '?', '#']), eq(false))?;
            verify_that!(identifier.segment.is_ascii(), eq(true))?;
        }
        Ok(())
    }
}
