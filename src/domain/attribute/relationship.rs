use crate::{
    domain::{
        attribute::{
            AttributeValue,
            common::{CommonAttributeFields, InlineEntity, OneOrMany},
        },
        uri::Uri,
    },
    jsonld::Expandable,
};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD Relationship representation (ETSI GS CIM 009 clause 5.2.6).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Relationship",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct Relationship {
    /// One or more target object URIs.
    #[jsonld(property = "ngsi:hasObject", custom)]
    pub object: OneOrMany<Uri>,
    /// Optional target node type(s).
    #[jsonld(property = "ngsi:hasObjectType", custom)]
    pub object_type: Option<OneOrMany<CompactString>>,
    /// Previous target object emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousObject", custom)]
    pub previous_object: Option<OneOrMany<Uri>>,
    /// Inline linked entity or entities returned for `join=inline`.
    #[jsonld(property = "ngsi:entity", custom)]
    pub entity: Option<InlineEntity>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

impl Default for Relationship {
    fn default() -> Relationship {
        Relationship {
            object: OneOrMany::Many(Vec::new()),
            object_type: None,
            previous_object: None,
            entity: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        domain::attribute::{Attribute, common::OneOrMany},
        jsonld::{Expandable, vocab},
    };

    #[test]
    fn relationship_round_trip_supports_object_arrays_and_object_type() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"],
            "objectType": ["Building", "schema:Place"],
            "previousObject": "urn:ngsi-ld:Entity:0"
        });
        let attr: Attribute = serde_json::from_value(json).expect("relationship");
        let Attribute::Relationship(relationship) = attr else {
            unreachable!("expected Relationship");
        };
        assert!(matches!(relationship.object, OneOrMany::Many(_)));
        assert!(relationship.object_type.is_some());
        assert!(relationship.previous_object.is_some());
    }

    #[test]
    fn relationship_expanded_includes_object_type() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1",
            "objectType": "Building"
        });
        let attr: Attribute = serde_json::from_value(json).expect("relationship");
        let Attribute::Relationship(relationship) = attr else {
            unreachable!("expected Relationship");
        };
        let expanded = relationship.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::properties::OBJECT_TYPE.as_str()));
    }
}
