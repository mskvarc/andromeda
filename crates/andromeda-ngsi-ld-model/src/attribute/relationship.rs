//! The Relationship attribute family (ETSI GS CIM 009 clause 5.2.6).

use crate::attribute::{common_fields::CommonAttributeFields, inline_entity::InlineEntity, one_or_many::OneOrMany};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use compact_str::CompactString;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD Relationship representation (ETSI GS CIM 009 clause 5.2.6).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Relationship",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct Relationship {
    /// One or more target object URIs.
    #[jsonld(property = "ngsi:hasObject", passthrough)]
    pub object: OneOrMany<Uri>,
    /// Optional target node type(s).
    #[jsonld(property = "ngsi:hasObjectType", passthrough)]
    pub object_type: Option<OneOrMany<CompactString>>,
    /// Previous target object emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousObject", passthrough)]
    pub previous_object: Option<OneOrMany<Uri>>,
    /// Inline linked entity or entities returned for `join=inline`.
    #[jsonld(property = "ngsi:entity", passthrough)]
    pub entity: Option<InlineEntity>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, one_or_many::OneOrMany};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use jsonld::Expandable;

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
        assert!(obj.contains_key(expanded_properties::OBJECT_TYPE.as_str()));
    }

    #[test]
    fn the_relationship_default_matches_the_field_by_field_literal() {
        use crate::attribute::relationship::Relationship;
        let actual = Relationship::default();
        pretty_assertions::assert_eq!(actual.object, OneOrMany::Many(Vec::new()));
        assert!(actual.object_type.is_none() && actual.previous_object.is_none() && actual.entity.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
