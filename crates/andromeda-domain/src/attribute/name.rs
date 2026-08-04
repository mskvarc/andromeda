//! The name under which an attribute is keyed inside an entity.

use crate::attribute::reference::{NgsiLdReferenceError, validate_ngsi_ld_name};
use compact_str::CompactString;
use derive_more::{Deref, Display, Into};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

/// Name under which an attribute is keyed in an entity or in another
/// attribute's sub-attribute map (ETSI GS CIM 009 clause 5.2.4).
///
/// Compact payloads key attributes by term or compact IRI; expanded payloads
/// key them by full IRI. Both are accepted, because both reach this type
/// depending on which side of JSON-LD expansion the payload sits on. The
/// grammar is exactly the one [`NgsiLdReference`] uses — see
/// [`validate_ngsi_ld_name`].
///
/// [`NgsiLdReference`]: crate::attribute::reference::NgsiLdReference
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Display, Deref, Into)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
#[into(String)]
pub struct AttributeName(CompactString);

impl AttributeName {
    /// Borrow the name as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl FromStr for AttributeName {
    type Err = NgsiLdReferenceError;

    fn from_str(value: &str) -> Result<AttributeName, NgsiLdReferenceError> {
        validate_ngsi_ld_name(value)?;
        Ok(AttributeName(CompactString::from(value)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for AttributeName {
    type Error = NgsiLdReferenceError;

    fn try_from(value: String) -> Result<AttributeName, NgsiLdReferenceError> {
        value.parse()
    }
}

impl AsRef<str> for AttributeName {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

// No derive produces `Borrow` for a target other than the field's own type,
// and this one is what lets `IndexMap<AttributeName, _>` be looked up by `&str`
// without allocating a name per lookup. Sound because the derived `Hash`
// forwards to `CompactString`, which hashes exactly as its `str` does.
impl Borrow<str> for AttributeName {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::attribute::name::AttributeName;
    use indexmap::IndexMap;

    #[test]
    fn an_attribute_name_accepts_a_compact_short_name() {
        let name: AttributeName = "temperature".parse().expect("short name");
        pretty_assertions::assert_eq!(name.as_str(), "temperature");
    }

    #[test]
    fn an_attribute_name_accepts_a_prefixed_name() {
        let name: AttributeName = "ngsild:speed".parse().expect("prefixed name");
        pretty_assertions::assert_eq!(name.as_str(), "ngsild:speed");
    }

    #[test]
    fn an_attribute_name_accepts_an_expanded_iri() {
        let name: AttributeName = "https://uri.etsi.org/ngsi-ld/default-context/speed".parse().expect("expanded IRI");
        pretty_assertions::assert_eq!(name.as_str(), "https://uri.etsi.org/ngsi-ld/default-context/speed");
    }

    #[test]
    fn an_attribute_name_rejects_empty_and_whitespace_bearing_input() {
        assert!("".parse::<AttributeName>().is_err());
        assert!("bad name".parse::<AttributeName>().is_err());
    }

    #[test]
    fn an_attribute_name_round_trips_as_a_json_object_key() {
        let mut map: IndexMap<AttributeName, u8> = IndexMap::new();
        map.insert("temperature".parse().expect("name"), 25);
        let json = serde_json::to_value(&map).expect("serialises");
        pretty_assertions::assert_eq!(json, serde_json::json!({"temperature": 25}));
        let decoded: IndexMap<AttributeName, u8> = serde_json::from_value(json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded.get("temperature"), Some(&25));
    }

    #[test]
    fn a_malformed_attribute_key_fails_to_deserialize() {
        let result = serde_json::from_value::<IndexMap<AttributeName, u8>>(serde_json::json!({"bad name": 1}));
        assert!(result.is_err());
    }
}
