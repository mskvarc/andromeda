//! The NGSI-LD compacted-name grammar and the reference type built on it.

use crate::{name::grammar::NameBuf, uri::Uri};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, From, Into};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Error returned when a compacted NGSI-LD reference string is invalid.
#[derive(Debug, Error)]
pub enum NgsiLdReferenceError {
    /// The value was empty.
    #[error("NGSI-LD reference must not be empty")]
    Empty,
    /// The reference did not match any supported NGSI-LD compacted or expanded form.
    #[error("invalid NGSI-LD reference: {0}")]
    Invalid(String),
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
/// Returns [`NgsiLdReferenceError::Empty`] for an empty value and
/// [`NgsiLdReferenceError::Invalid`] when no accepted form matches.
pub fn validate_ngsi_ld_name(value: &str) -> Result<(), NgsiLdReferenceError> {
    if value.is_empty() {
        return Err(NgsiLdReferenceError::Empty);
    }
    if Uri::from_str(value).is_ok() || IriBuf::from_str(value).is_ok() || NameBuf::new(value.to_string()).is_ok() {
        return Ok(());
    }
    if let Some((prefix, suffix)) = value.split_once(':')
        && NameBuf::new(prefix.to_string()).is_ok()
        && !suffix.is_empty()
        && !suffix.chars().any(char::is_whitespace)
    {
        return Ok(());
    }
    Err(NgsiLdReferenceError::Invalid(value.to_string()))
}

validated_ngsi_name! {
    /// NGSI-LD reference value used by `vocab`, `valueType`, and `objectType`
    /// members.
    ///
    /// The compact wire form can be a short name, a compact IRI, or an expanded
    /// IRI/URI.
    NgsiLdReference,
    derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, From, Into),
    as_ref(str, CompactString)
}

#[cfg(test)]
mod tests {
    use crate::reference::NgsiLdReference;

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
}
