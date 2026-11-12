//! The NGSI-LD Null (ETSI GS CIM 009 v1.9.1 clauses 3.1 and 4.5.0).
//!
//! JSON-LD expansion drops a JSON `null`, so NGSI-LD encodes a deletion as the
//! URI `urn:ngsi-ld:null` wherever a URI string is a valid value, as an array
//! holding it alone where an array is required, and as `{"@none":
//! "urn:ngsi-ld:null"}` for a `languageMap`. Clause 3.1 defines the Null as
//! exactly those encodings, so no other text names it: the string is an
//! absolute IRI that expansion leaves as written, and an IRI that merely
//! resembles it is ordinary data.
//!
//! Every reading of the Null, whatever holds the text (a compact node, an
//! expanded item, a stored document, a JSON literal), decides through
//! [`is_ngsi_ld_null`], so no two readers can disagree on what the Null is.

use urn_rs::{UrnSlice, urn};

/// The NGSI-LD Null, the URN `urn:ngsi-ld:null`.
///
/// It appears as data rather than as an `@context` term, so the core context
/// declares no term for it. A `static`, so the text it lends lives as long as
/// the program and a writer can hand it on.
pub static NULL_URI: UrnSlice<'static> = urn!("urn:ngsi-ld:null");

/// Whether `text` is the NGSI-LD Null.
#[must_use]
pub fn is_ngsi_ld_null(text: &str) -> bool {
    text == NULL_URI.as_str()
}

#[cfg(test)]
mod tests {
    use crate::ngsi_ld_null::{NULL_URI, is_ngsi_ld_null};

    #[test]
    fn the_null_is_the_urn_clause_3_1_defines() {
        pretty_assertions::assert_eq!(NULL_URI.as_str(), "urn:ngsi-ld:null");
        assert!(is_ngsi_ld_null("urn:ngsi-ld:null"));
    }

    /// Clause 3.1 names one spelling. The core context's `ngsi-ld` prefix
    /// expands the compact IRI `ngsi-ld:null` to the IRI below, which is a
    /// value a client may legitimately write and so is never a deletion.
    #[test]
    fn no_other_text_is_the_null() {
        for text in ["https://uri.etsi.org/ngsi-ld/null", "ngsi-ld:null", " urn:ngsi-ld:null", "urn:ngsi-ld:nil", ""] {
            assert!(!is_ngsi_ld_null(text), "{text}");
        }
    }
}
