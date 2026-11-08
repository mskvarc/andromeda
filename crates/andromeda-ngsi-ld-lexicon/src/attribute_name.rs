//! The name under which an attribute is keyed inside an entity.

use crate::reference::{NgsiLdReference, NgsiLdReferenceError, validate_ngsi_ld_name};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, Into};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

validated_ngsi_name! {
    /// Name under which an attribute is keyed in an entity or in another
    /// attribute's sub-attribute map (ETSI GS CIM 009 clause 5.2.4).
    ///
    /// Compact payloads key attributes by term or compact IRI; expanded payloads
    /// key them by full IRI. Both are accepted, because both reach this type
    /// depending on which side of JSON-LD expansion the payload sits on. The
    /// grammar is exactly the one [`NgsiLdReference`] uses, checked by
    /// [`validate_ngsi_ld_name`].
    ///
    /// [`NgsiLdReference`]: crate::reference::NgsiLdReference
    AttributeName,
    derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, AsRef, Display, Deref, Into),
    as_ref(str),
    borrow_str
}

// Hand-written: no derive converts between two newtypes. Total, because an
// attribute name and a reference are checked against one grammar
// (`validate_ngsi_ld_name`), so a reference naming a `GeoProperty` names the
// attribute that keys it.
impl From<&NgsiLdReference> for AttributeName {
    fn from(reference: &NgsiLdReference) -> AttributeName {
        AttributeName(CompactString::from(reference.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_name::AttributeName, identifier_length::IDENTIFIER_BYTE_LIMIT, reference::NgsiLdReferenceError};
    use indexmap::IndexMap;
    use iri_rs::IriBuf;

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

    name_length_tests!(AttributeName);
}
