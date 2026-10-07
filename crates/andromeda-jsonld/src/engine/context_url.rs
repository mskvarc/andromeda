//! Telling an absolute IRI from a term.

/// Whether a term is already an absolute IRI and needs no expansion.
///
/// A conservative short-circuit for the common schemes; a term this rejects is
/// not necessarily a plain term (see [`is_absolute_iri`]), so callers that must
/// distinguish an absolute IRI from a compact one only after ruling out a
/// defined prefix use that function instead.
///
/// It is also the rule deciding which names a name compaction compacts at
/// all: a name outside these schemes is answered verbatim.
#[must_use]
pub fn is_full_iri(term: &str) -> bool {
    term.starts_with("http://") || term.starts_with("https://") || term.starts_with("urn:")
}

/// Whether a term has the form of an absolute IRI: a scheme (RFC 3987
/// `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`) followed by `:`.
///
/// Unlike [`is_full_iri`] this admits every scheme (`did:`, `mailto:`, `tel:`),
/// so a context reference written with any scheme is kept as the absolute
/// IRI it is rather than resolved against a base (JSON-LD 1.1 clause 4.6.2).
/// `did:example:1` is an absolute IRI although the three-scheme allowlist
/// rejects it.
pub(crate) fn is_absolute_iri(term: &str) -> bool {
    let mut chars = term.char_indices();
    match chars.next() {
        Some((_, first)) if first.is_ascii_alphabetic() => {}
        Some(_) | None => return false,
    }
    for (index, ch) in chars {
        if ch == ':' {
            return index > 0;
        }
        if !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.') {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::engine::context_url::{is_absolute_iri, is_full_iri};

    #[test]
    fn a_term_counts_as_a_full_iri_only_under_http_https_or_urn() {
        assert!(is_full_iri("https://example.com/Type"));
        assert!(is_full_iri("http://example.com/Type"));
        assert!(is_full_iri("urn:ngsi-ld:Type"));
        assert!(!is_full_iri("Vehicle"));
        assert!(!is_full_iri("ex:Vehicle"));
        // A term that merely starts with `http` but carries no scheme is not a full IRI: compaction gates on this predicate.
        assert!(!is_full_iri("httpEndpoint"));
        assert!(!is_full_iri("httpfoo:x"));
    }

    #[test]
    fn a_string_with_any_scheme_has_the_form_of_an_absolute_iri() {
        assert!(is_absolute_iri("https://example.com/Type"));
        assert!(is_absolute_iri("urn:ngsi-ld:Type"));
        assert!(is_absolute_iri("did:example:123"));
        assert!(is_absolute_iri("mailto:x@example.com"));
        // `ex:Vehicle` has IRI form; whether it expands as a compact IRI is
        // decided by the prefix table before this is consulted.
        assert!(is_absolute_iri("ex:Vehicle"));
        assert!(!is_absolute_iri("Vehicle"));
        assert!(!is_absolute_iri("1did:x"));
    }
}
