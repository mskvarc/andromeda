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

use andromeda_json_literal::{
    JsonLiteral,
    piece::{JsonKind, JsonPiece},
};
use serde::{
    Serialize,
    Serializer,
    ser::{Error as SerializeError, SerializeMap, SerializeSeq},
};
use thiserror::Error;

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
    /// Returns the serializer's error for a number no IEEE 754 double can hold:
    /// RFC 8785 section 3.2.2.3 writes every number as the shortest double that
    /// round-trips, so such a document has no canonical form at all.
    pub(crate) fn of(document: &JsonLiteral) -> Result<CanonicalContextDocument, serde_json::Error> {
        serde_json_canonicalizer::to_vec(&ValueOf(document.piece())).map(CanonicalContextDocument)
    }

    /// The canonical bytes.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One value of an admitted document, handed to the canonicalizer value by
/// value: strings and member names as text, numbers as the doubles RFC 8785
/// writes them as.
struct ValueOf<'a>(JsonPiece<'a>);

// Hand-written: a piece is written by what it holds, level by level, which no
// derive over a borrowed slice expresses.
impl Serialize for ValueOf<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let piece = self.0;
        match piece.kind() {
            JsonKind::Null => serializer.serialize_unit(),
            JsonKind::Boolean => serializer.serialize_bool(piece.as_bool().ok_or_else(|| S::Error::custom(NoCanonicalForm::Unreadable))?),
            JsonKind::Number => {
                let number: f64 = piece.text().trim().parse().map_err(S::Error::custom)?;
                if !number.is_finite() {
                    return Err(S::Error::custom(NoCanonicalForm::BeyondEveryDouble));
                }
                serializer.serialize_f64(number)
            }
            JsonKind::String => serializer.serialize_str(&piece.as_str().ok_or_else(|| S::Error::custom(NoCanonicalForm::Unreadable))?),
            JsonKind::Array => {
                let items = piece.elements().ok_or_else(|| S::Error::custom(NoCanonicalForm::Unreadable))?;
                let mut sequence = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    sequence.serialize_element(&ValueOf(item))?;
                }
                sequence.end()
            }
            JsonKind::Object => {
                let members = piece.members().ok_or_else(|| S::Error::custom(NoCanonicalForm::Unreadable))?;
                let mut map = serializer.serialize_map(Some(members.len()))?;
                for (key, value) in members {
                    map.serialize_entry(key.as_ref(), &ValueOf(value))?;
                }
                map.end()
            }
        }
    }
}

/// Why an admitted value has no canonical form.
#[derive(Debug, Error)]
enum NoCanonicalForm {
    /// The piece is not the kind its first character says, which no admitted
    /// text is.
    #[error("an admitted value does not read as its kind")]
    Unreadable,
    /// The number lies beyond every IEEE 754 double, which RFC 8785 section
    /// 3.2.2.3 writes every number as.
    #[error("a number lies beyond every IEEE 754 double")]
    BeyondEveryDouble,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::canonical_document::CanonicalContextDocument;
    use andromeda_json_literal::JsonLiteral;
    use serde_json::json;

    /// Admit `text` exactly as a request body is admitted.
    fn parsed(text: &str) -> JsonLiteral {
        JsonLiteral::admit(text).expect("valid JSON")
    }

    fn canonical(document: &JsonLiteral) -> Vec<u8> {
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
        let forward = parsed(&json!({ "@context": ["https://e.org/a.jsonld", "https://e.org/b.jsonld"] }).to_string());
        let backward = parsed(&json!({ "@context": ["https://e.org/b.jsonld", "https://e.org/a.jsonld"] }).to_string());

        assert_ne!(canonical(&forward), canonical(&backward));
    }

    /// A number no double can hold is admitted, as the broker keeps it
    /// exactly, but has no RFC 8785 canonical form.
    #[test]
    fn a_number_no_double_can_hold_has_no_canonical_form() {
        assert!(CanonicalContextDocument::of(&parsed(r#"{"@context": {"@version": 1e400}}"#)).is_err());
    }
}
