//! The RFC 8785 canonical form of a stored `@context` document.
//!
//! An `ImplicitlyCreated` entry (clause 5.13.1) takes an identifier derived
//! from its content, so equal documents share one entry. "Equal" has to mean
//! equal as JSON rather than equal as text: a client that sends the same
//! `@context` array with other whitespace, members in another order or a number
//! spelled `1.0` instead of `1` has sent the same document. The JSON
//! Canonicalization Scheme (RFC 8785) is the standard answer to that, and it is
//! what both the derivation and the check that a stored entry holds the same
//! document read.

use serde_json::Value;

/// A stored `@context` document written in the JSON Canonicalization Scheme.
///
/// Member order, whitespace, string escaping and number spelling are fixed by
/// RFC 8785, so two documents equal as JSON have byte-identical canonical forms.
/// Array order is kept: JSON-LD processes an `@context` array in order, so two
/// arrays holding the same contexts in another order are different documents.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CanonicalContextDocument(Vec<u8>);

impl CanonicalContextDocument {
    /// The canonical form of `document`.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error for a number no IEEE 754 double can hold
    /// (`1e400`, say): RFC 8785 section 3.2.2.3 writes every number as the
    /// shortest double that round-trips, so such a document has no canonical
    /// form at all.
    pub(crate) fn of(document: &Value) -> Result<CanonicalContextDocument, serde_json::Error> {
        serde_json_canonicalizer::to_vec(document).map(CanonicalContextDocument)
    }

    /// The canonical bytes.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::canonical_document::CanonicalContextDocument;
    use serde_json::{Value, json};

    /// Parse `text` exactly as a request body is parsed.
    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).expect("valid JSON")
    }

    fn canonical(document: &Value) -> Vec<u8> {
        CanonicalContextDocument::of(document).expect("a canonical form").as_bytes().to_vec()
    }

    #[test]
    fn member_order_and_whitespace_do_not_change_the_canonical_form() {
        let first = parsed(r#"{"@context": {"a": "https://e.org/a", "b": "https://e.org/b"}}"#);
        let second = parsed("{\n  \"@context\" : {\"b\":\"https://e.org/b\",\n \"a\":\"https://e.org/a\"}\n}");

        pretty_assertions::assert_eq!(canonical(&first), canonical(&second));
        pretty_assertions::assert_eq!(
            String::from_utf8(canonical(&first)).expect("UTF-8"),
            r#"{"@context":{"a":"https://e.org/a","b":"https://e.org/b"}}"#
        );
    }

    /// The workspace builds `serde_json` with `arbitrary_precision`, which keeps
    /// a number's spelling as written. The canonical form must not: RFC 8785
    /// section 3.2.2.3 writes every number as the shortest round-tripping double,
    /// so `12e1`, `120.0` and `120` are one number.
    #[test]
    fn number_spelling_does_not_change_the_canonical_form() {
        let spellings = [
            parsed(r#"{"@context": {"@version": 120}}"#),
            parsed(r#"{"@context": {"@version": 12e1}}"#),
            parsed(r#"{"@context": {"@version": 120.0}}"#),
        ];

        for spelling in &spellings {
            pretty_assertions::assert_eq!(String::from_utf8(canonical(spelling)).expect("UTF-8"), r#"{"@context":{"@version":120}}"#);
        }
    }

    #[test]
    fn array_order_is_significant() {
        let forward = json!({ "@context": ["https://e.org/a.jsonld", "https://e.org/b.jsonld"] });
        let backward = json!({ "@context": ["https://e.org/b.jsonld", "https://e.org/a.jsonld"] });

        assert_ne!(canonical(&forward), canonical(&backward));
    }

    #[test]
    fn a_number_no_double_can_hold_has_no_canonical_form() {
        assert!(CanonicalContextDocument::of(&parsed(r#"{"@context": {"@version": 1e400}}"#)).is_err());
    }
}
