//! The NGSI-LD compacted-name grammar and the reference type built on it.

use crate::{entity_type_name::EntityTypeName, identifier_length::IdentifierTooLong, name::grammar::Name, uri::Uri};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, Into};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Error returned when a compacted NGSI-LD reference string is invalid.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum NgsiLdReferenceError {
    /// The value was empty.
    #[error("a name must not be empty")]
    Empty,
    /// The value is longer than a name may be.
    #[error(transparent)]
    TooLong(#[from] IdentifierTooLong),
    /// The reference did not match any supported NGSI-LD compacted or expanded form.
    #[error("a name must be a term, a compact IRI or an absolute IRI")]
    Invalid,
}

/// Validate a value against the NGSI-LD compacted-name grammar (clause 4.6.2).
///
/// An accepted value is a URI, an expanded IRI, a JSON-LD term (short name), or
/// a compact IRI `prefix:suffix` whose prefix is itself a term. This is the one
/// grammar every named NGSI-LD member shares, so both [`NgsiLdReference`] and
/// the attribute-name type validate through it rather than restating it.
///
/// # Errors
///
/// Returns [`NgsiLdReferenceError::Empty`] for an empty value,
/// [`NgsiLdReferenceError::TooLong`] for one longer than
/// [`IDENTIFIER_BYTE_LIMIT`], and [`NgsiLdReferenceError::Invalid`] when no
/// accepted form matches. The length is checked before the grammar, because the
/// IRI grammar admits a name of any length that the store would then refuse.
///
/// [`IDENTIFIER_BYTE_LIMIT`]: crate::identifier_length::IDENTIFIER_BYTE_LIMIT
pub fn validate_ngsi_ld_name(value: &str) -> Result<(), NgsiLdReferenceError> {
    if value.is_empty() {
        return Err(NgsiLdReferenceError::Empty);
    }
    IdentifierTooLong::check(value)?;
    // The four forms are alternatives, so they are tried cheapest first: the
    // compact-IRI and short-name forms are a grammar match on the borrowed text,
    // and they already accept every absolute IRI whose scheme is a term (the
    // expanded attribute names a stored payload is keyed by) before the URI
    // and IRI parsers, which build a parsed value, are consulted.
    if is_compact_iri(value) || Name::new(value).is_ok() || Uri::from_str(value).is_ok() || IriBuf::from_str(value).is_ok() {
        return Ok(());
    }
    Err(NgsiLdReferenceError::Invalid)
}

/// Whether a value is a compact IRI `prefix:suffix` whose prefix is a term and
/// whose suffix is present and holds no whitespace.
fn is_compact_iri(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(prefix, suffix)| Name::new(prefix).is_ok() && !suffix.is_empty() && !suffix.chars().any(char::is_whitespace))
}

validated_ngsi_name! {
    /// NGSI-LD reference value used by `vocab`, `valueType`, and `objectType`
    /// members.
    ///
    /// The compact wire form can be a short name, a compact IRI, or an expanded
    /// IRI/URI.
    NgsiLdReference,
    derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, Into),
    as_ref(str, CompactString)
}

// Hand-written: no derive converts between two newtypes. Total, because an
// entity type name and a reference are checked against one grammar
// (`validate_ngsi_ld_name`), so an IRI a store interns as a type name reads
// back as the reference it was interned from.
impl From<&EntityTypeName> for NgsiLdReference {
    fn from(name: &EntityTypeName) -> NgsiLdReference {
        NgsiLdReference(CompactString::from(name.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_type_name::EntityTypeName,
        identifier_length::IDENTIFIER_BYTE_LIMIT,
        reference::{NgsiLdReference, NgsiLdReferenceError},
    };
    use iri_rs::IriBuf;

    #[test]
    fn ngsi_ld_reference_accepts_short_name() {
        let value = NgsiLdReference::try_from("Temperature".to_string()).expect("short name");
        pretty_assertions::assert_eq!(value.to_string(), "Temperature");
    }

    #[test]
    fn ngsi_ld_reference_accepts_compact_iri() {
        let value = NgsiLdReference::try_from("schema:Vehicle".to_string()).expect("compact IRI");
        pretty_assertions::assert_eq!(value.to_string(), "schema:Vehicle");
    }

    #[test]
    fn ngsi_ld_reference_rejects_whitespace() {
        let result = NgsiLdReference::try_from("bad ref".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn an_ngsi_ld_reference_parses_from_a_borrowed_str() {
        let parsed: NgsiLdReference = "https://example.com/Vehicle".parse().expect("valid reference");
        pretty_assertions::assert_eq!(String::from(parsed), "https://example.com/Vehicle");
        assert!("".parse::<NgsiLdReference>().is_err());
    }

    #[test]
    fn a_type_name_is_the_reference_of_the_same_text() {
        for raw in ["xsd:decimal", "https://example.com/Vehicle", "Building"] {
            let name: EntityTypeName = raw.parse().expect(raw);
            pretty_assertions::assert_eq!(NgsiLdReference::from(&name), raw.parse::<NgsiLdReference>().expect(raw));
        }
    }

    name_length_tests!(NgsiLdReference);
}
