//! The absolute IRI an entity type name expands to.

use crate::reference::NgsiLdReferenceError;
use derive_more::{AsRef, Display, From, Into};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

/// The expanded form of an entity type name: an absolute IRI (ETSI GS CIM 009
/// clause 4.6.2).
///
/// It carries no length bound: a type selection that expands its types holds
/// them as plain IRIs, and the bound is applied where an expanded type is
/// stored as a name. A value `ex:Vehicle` is an absolute IRI syntactically (its
/// scheme is `ex`) and is accepted; whether `ex` is a declared prefix is the
/// expander's question, not this type's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, AsRef, From, Into, Serialize, Deserialize)]
#[serde(from = "IriBuf", into = "IriBuf")]
#[as_ref(str)]
pub struct EntityTypeIri(IriBuf);

impl EntityTypeIri {
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
}

// Hand-written: `Borrow<str>` has no derive for a target other than the
// field's own type. Sound because `IriBuf` hashes and compares exactly as its
// `str` does.
impl Borrow<str> for EntityTypeIri {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

// Hand-written: parsing lives in `FromStr` so borrowed call sites do not
// allocate, and no derive expresses a fallible parse.
impl FromStr for EntityTypeIri {
    type Err = NgsiLdReferenceError;

    fn from_str(value: &str) -> Result<EntityTypeIri, NgsiLdReferenceError> {
        if value.is_empty() {
            return Err(NgsiLdReferenceError::Empty);
        }
        let iri = IriBuf::from_str(value).map_err(|_| NgsiLdReferenceError::NotAbsolute)?;
        Ok(EntityTypeIri(iri))
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_type_iri::EntityTypeIri, identifier_length::IDENTIFIER_BYTE_LIMIT, reference::NgsiLdReferenceError};
    use indexmap::IndexMap;
    use iri_rs::IriBuf;
    use std::collections::HashSet;

    #[test]
    fn an_entity_type_iri_accepts_an_absolute_iri() {
        let iri: EntityTypeIri = "https://example.com/Vehicle".parse().expect("absolute IRI");
        pretty_assertions::assert_eq!(iri.as_str(), "https://example.com/Vehicle");
        pretty_assertions::assert_eq!(iri.as_iri().as_str(), iri.as_str());
    }

    #[test]
    fn an_entity_type_iri_refuses_a_bare_term() {
        let refused = "Vehicle".parse::<EntityTypeIri>().expect_err("a bare term");
        pretty_assertions::assert_eq!(refused, NgsiLdReferenceError::NotAbsolute);
    }

    #[test]
    fn an_entity_type_iri_refuses_an_empty_value() {
        let refused = "".parse::<EntityTypeIri>().expect_err("empty");
        pretty_assertions::assert_eq!(refused, NgsiLdReferenceError::Empty);
    }

    #[test]
    fn an_entity_type_iri_accepts_a_compact_iri_whose_prefix_is_a_scheme_shaped_term() {
        let iri: EntityTypeIri = "ex:Vehicle".parse().expect("`ex` is a scheme to the IRI grammar");
        pretty_assertions::assert_eq!(iri.as_str(), "ex:Vehicle");
    }

    #[test]
    fn an_entity_type_iri_accepts_an_iri_past_the_identifier_bound() {
        let prefix = "https://example.com/";
        let raw = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()));
        let iri: EntityTypeIri = raw.parse().expect("an expanded type carries no length bound");
        pretty_assertions::assert_eq!(iri.as_str(), raw);
    }

    #[test]
    fn an_entity_type_iri_is_built_from_a_parsed_iri() {
        let iri: IriBuf = "https://example.com/Vehicle".parse().expect("absolute IRI");
        let type_iri = EntityTypeIri::from(iri.clone());
        pretty_assertions::assert_eq!(IriBuf::from(type_iri), iri);
    }

    #[test]
    fn an_entity_type_iri_probes_a_set_by_str() {
        let set: HashSet<EntityTypeIri> = HashSet::from(["https://example.com/Vehicle".parse().expect("absolute IRI")]);
        assert!(set.contains("https://example.com/Vehicle"));
        assert!(!set.contains("https://example.com/Building"));
    }

    #[test]
    fn an_entity_type_iri_round_trips_as_a_json_string() {
        let iri: EntityTypeIri = "https://example.com/Vehicle".parse().expect("absolute IRI");
        let json = serde_json::to_value(&iri).expect("serialises");
        pretty_assertions::assert_eq!(json, serde_json::json!("https://example.com/Vehicle"));
        pretty_assertions::assert_eq!(serde_json::from_value::<EntityTypeIri>(json).expect("deserialises"), iri);
    }

    #[test]
    fn an_entity_type_iri_refuses_a_bare_term_when_deserialising() {
        assert!(serde_json::from_value::<EntityTypeIri>(serde_json::json!("Vehicle")).is_err());
    }

    #[test]
    fn an_entity_type_iri_round_trips_as_a_json_object_key() {
        let mut map: IndexMap<EntityTypeIri, u8> = IndexMap::new();
        map.insert("https://example.com/Vehicle".parse().expect("absolute IRI"), 3);
        let json = serde_json::to_value(&map).expect("serialises");
        pretty_assertions::assert_eq!(json, serde_json::json!({"https://example.com/Vehicle": 3}));
        let decoded: IndexMap<EntityTypeIri, u8> = serde_json::from_value(json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded.get("https://example.com/Vehicle"), Some(&3));
    }
}
