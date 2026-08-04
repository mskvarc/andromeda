//! The type an entity declares (ETSI GS CIM 009 clause 5.2.4, `type` member).

use crate::attribute::reference::{NgsiLdReferenceError, validate_ngsi_ld_name};
use compact_str::CompactString;
use derive_more::{Deref, Display, Into};
use jsonld::ExpandableTypeValue;
use jsonld_expandable_core::JsonValue;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    iter::once,
    slice::{Iter, from_ref},
    str::FromStr,
};

/// Name of an entity type (clause 5.2.4).
///
/// Compact payloads name types by term or compact IRI; expanded payloads name
/// them by full IRI. Both are accepted, because both reach this type depending
/// on which side of JSON-LD expansion the payload sits on. The grammar is the
/// one every named NGSI-LD member shares — see [`validate_ngsi_ld_name`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Display, Deref, Into)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
#[into(String)]
pub struct EntityTypeName(CompactString);

impl EntityTypeName {
    /// Borrow the type name as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl FromStr for EntityTypeName {
    type Err = NgsiLdReferenceError;

    fn from_str(value: &str) -> Result<EntityTypeName, NgsiLdReferenceError> {
        validate_ngsi_ld_name(value)?;
        Ok(EntityTypeName(CompactString::from(value)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for EntityTypeName {
    type Error = NgsiLdReferenceError;

    fn try_from(value: String) -> Result<EntityTypeName, NgsiLdReferenceError> {
        value.parse()
    }
}

impl AsRef<str> for EntityTypeName {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

// No derive produces `Borrow` for a target other than the field's own type,
// and this one lets sets and maps keyed by type name be probed with a `&str`
// without allocating. Sound because the derived `Hash` forwards to
// `CompactString`, which hashes exactly as its `str` does.
impl Borrow<str> for EntityTypeName {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

/// Single or multiple entity type values (clause 5.2.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EntityTypeValue {
    /// Exactly one type.
    Single(EntityTypeName),
    /// Several types declared together.
    Multiple(Vec<EntityTypeName>),
}

impl EntityTypeValue {
    /// Iterate the contained type names uniformly regardless of variant.
    pub fn iter(&self) -> Iter<'_, EntityTypeName> {
        match self {
            EntityTypeValue::Single(name) => from_ref(name).iter(),
            EntityTypeValue::Multiple(names) => names.iter(),
        }
    }
}

impl ExpandableTypeValue for EntityTypeValue {
    fn to_type_array<V: JsonValue>(&self) -> V {
        match self {
            EntityTypeValue::Single(name) => V::array(once(V::string(name.as_str()))),
            EntityTypeValue::Multiple(names) => V::array(names.iter().map(|name| V::string(name.as_str()))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::entity_type::{EntityTypeName, EntityTypeValue};

    #[test]
    fn an_entity_type_name_accepts_terms_prefixes_and_expanded_iris() {
        for raw in ["Building", "schema:Place", "https://example.com/Vehicle"] {
            pretty_assertions::assert_eq!(raw.parse::<EntityTypeName>().expect(raw).as_str(), raw);
        }
    }

    #[test]
    fn an_entity_type_name_rejects_empty_and_whitespace_bearing_input() {
        assert!("".parse::<EntityTypeName>().is_err());
        assert!("two words".parse::<EntityTypeName>().is_err());
    }

    #[test]
    fn an_entity_type_value_keeps_the_single_and_array_wire_forms_apart() {
        let single: EntityTypeValue = serde_json::from_value(serde_json::json!("Building")).expect("single");
        pretty_assertions::assert_eq!(serde_json::to_value(&single).expect("serialises"), serde_json::json!("Building"));
        pretty_assertions::assert_eq!(single.iter().count(), 1);

        let multiple: EntityTypeValue = serde_json::from_value(serde_json::json!(["Building", "Venue"])).expect("multiple");
        pretty_assertions::assert_eq!(serde_json::to_value(&multiple).expect("serialises"), serde_json::json!(["Building", "Venue"]));
        pretty_assertions::assert_eq!(multiple.iter().count(), 2);
    }
}
