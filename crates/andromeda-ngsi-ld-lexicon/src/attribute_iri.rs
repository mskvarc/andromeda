//! The absolute IRI an attribute name expands to.

use crate::{identifier_length::IdentifierTooLong, reference::NgsiLdReferenceError};
use derive_more::{AsRef, Display, Into};
use iri_rs::{Iri, IriBuf};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

/// The expanded form of an attribute name: an absolute IRI within the
/// identifier byte bound (ETSI GS CIM 009 clause 4.6.2).
///
/// A compact attribute name is a term or a compact IRI, and JSON-LD expansion
/// turns it into an absolute IRI. Every expanded attribute name is a key in the
/// store, so it carries the same [`IDENTIFIER_BYTE_LIMIT`] as an
/// [`AttributeName`]. A value `ex:speed` is an absolute IRI syntactically (its
/// scheme is `ex`) and is accepted; whether `ex` is a declared prefix is the
/// expander's question, not this type's.
///
/// [`AttributeName`]: crate::attribute_name::AttributeName
/// [`IDENTIFIER_BYTE_LIMIT`]: crate::identifier_length::IDENTIFIER_BYTE_LIMIT
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, AsRef, Into, Serialize, Deserialize)]
#[serde(try_from = "IriBuf", into = "IriBuf")]
#[as_ref(str)]
pub struct AttributeIri(IriBuf);

impl AttributeIri {
    /// Borrow the IRI as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Borrow the parsed IRI, whose absoluteness is known from construction.
    #[must_use]
    pub const fn as_iri(&self) -> &IriBuf {
        &self.0
    }

    /// The attribute IRI a vocabulary names itself. The grammar is checked at
    /// compile time by `iri!`, and the caller passing a literal it wrote is the
    /// guarantor that it is within the identifier bound.
    #[must_use]
    pub fn from_static(iri: Iri<&'static str>) -> AttributeIri {
        AttributeIri(IriBuf::from(iri))
    }
}

// Hand-written: `Borrow<str>` has no derive for a target other than the
// field's own type. Sound because `IriBuf` hashes and compares exactly as its
// `str` does.
impl Borrow<str> for AttributeIri {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

// Hand-written: the bound check is what makes the newtype worth having, and no
// derive expresses a fallible conversion with a check.
impl TryFrom<IriBuf> for AttributeIri {
    type Error = IdentifierTooLong;

    fn try_from(iri: IriBuf) -> Result<AttributeIri, IdentifierTooLong> {
        IdentifierTooLong::check(iri.as_str())?;
        Ok(AttributeIri(iri))
    }
}

// Hand-written: parsing lives in `FromStr` so borrowed call sites do not
// allocate, and no derive expresses a fallible parse. The length is checked
// before the IRI grammar, which has no bound of its own.
impl FromStr for AttributeIri {
    type Err = NgsiLdReferenceError;

    fn from_str(value: &str) -> Result<AttributeIri, NgsiLdReferenceError> {
        if value.is_empty() {
            return Err(NgsiLdReferenceError::Empty);
        }
        IdentifierTooLong::check(value)?;
        let iri = IriBuf::from_str(value).map_err(|_| NgsiLdReferenceError::NotAbsolute)?;
        Ok(AttributeIri(iri))
    }
}

// Hand-written: no derive provides a fallible `TryFrom<String>` that parses.
impl TryFrom<String> for AttributeIri {
    type Error = NgsiLdReferenceError;

    fn try_from(value: String) -> Result<AttributeIri, NgsiLdReferenceError> {
        value.parse()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_iri::AttributeIri,
        identifier_length::{IDENTIFIER_BYTE_LIMIT, IdentifierTooLong},
        reference::NgsiLdReferenceError,
    };
    use indexmap::IndexMap;
    use iri_rs::{IriBuf, iri};
    use std::collections::HashSet;

    #[test]
    fn an_attribute_iri_accepts_an_absolute_iri() {
        let iri: AttributeIri = "https://uri.etsi.org/ngsi-ld/default-context/speed".parse().expect("absolute IRI");
        pretty_assertions::assert_eq!(iri.as_str(), "https://uri.etsi.org/ngsi-ld/default-context/speed");
        pretty_assertions::assert_eq!(iri.as_iri().as_str(), iri.as_str());
    }

    #[test]
    fn an_attribute_iri_is_built_from_a_compile_time_iri() {
        let iri = AttributeIri::from_static(iri!("https://example.com/a"));
        pretty_assertions::assert_eq!(iri.as_str(), "https://example.com/a");
    }

    #[test]
    fn an_attribute_iri_refuses_a_bare_term() {
        let refused = "temperature".parse::<AttributeIri>().expect_err("a bare term");
        pretty_assertions::assert_eq!(refused, NgsiLdReferenceError::NotAbsolute);
    }

    #[test]
    fn an_attribute_iri_refuses_an_empty_value() {
        let refused = "".parse::<AttributeIri>().expect_err("empty");
        pretty_assertions::assert_eq!(refused, NgsiLdReferenceError::Empty);
    }

    #[test]
    fn an_attribute_iri_accepts_a_compact_iri_whose_prefix_is_a_scheme_shaped_term() {
        let iri: AttributeIri = "ex:speed".parse().expect("`ex` is a scheme to the IRI grammar");
        pretty_assertions::assert_eq!(iri.as_str(), "ex:speed");
    }

    #[test]
    fn an_attribute_iri_is_built_from_an_iri_within_the_bound() {
        let iri: IriBuf = "https://example.com/a".parse().expect("absolute IRI");
        let attribute = AttributeIri::try_from(iri.clone()).expect("within the bound");
        pretty_assertions::assert_eq!(IriBuf::from(attribute), iri);
    }

    #[test]
    fn an_attribute_iri_refuses_a_parsed_iri_past_the_bound() {
        let prefix = "https://example.com/";
        let raw = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()));
        let iri: IriBuf = raw.parse().expect("the IRI grammar has no length bound");
        pretty_assertions::assert_eq!(
            AttributeIri::try_from(iri),
            Err(IdentifierTooLong {
                length: IDENTIFIER_BYTE_LIMIT + 1
            })
        );
    }

    #[test]
    fn an_attribute_iri_probes_a_set_by_str() {
        let set: HashSet<AttributeIri> = HashSet::from(["https://example.com/a".parse().expect("absolute IRI")]);
        assert!(set.contains("https://example.com/a"));
        assert!(!set.contains("https://example.com/b"));
    }

    #[test]
    fn an_attribute_iri_round_trips_as_a_json_string() {
        let iri: AttributeIri = "https://example.com/a".parse().expect("absolute IRI");
        let json = serde_json::to_value(&iri).expect("serialises");
        pretty_assertions::assert_eq!(json, serde_json::json!("https://example.com/a"));
        pretty_assertions::assert_eq!(serde_json::from_value::<AttributeIri>(json).expect("deserialises"), iri);
    }

    #[test]
    fn an_attribute_iri_refuses_a_bare_term_when_deserialising() {
        assert!(serde_json::from_value::<AttributeIri>(serde_json::json!("temperature")).is_err());
    }

    #[test]
    fn an_attribute_iri_round_trips_as_a_json_object_key() {
        let mut map: IndexMap<AttributeIri, u8> = IndexMap::new();
        map.insert("https://example.com/a".parse().expect("absolute IRI"), 25);
        let json = serde_json::to_value(&map).expect("serialises");
        pretty_assertions::assert_eq!(json, serde_json::json!({"https://example.com/a": 25}));
        let decoded: IndexMap<AttributeIri, u8> = serde_json::from_value(json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded.get("https://example.com/a"), Some(&25));
    }

    #[test]
    fn a_bare_term_key_fails_to_deserialize_as_an_attribute_iri_map() {
        assert!(serde_json::from_value::<IndexMap<AttributeIri, u8>>(serde_json::json!({"temperature": 1})).is_err());
    }

    name_length_tests!(AttributeIri);
}
