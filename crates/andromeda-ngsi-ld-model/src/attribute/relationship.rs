//! The Relationship attribute family (ETSI GS CIM 009 clause 5.2.6).

use crate::attribute::{common_fields::CommonAttributeFields, inline_entity::InlineEntity, one_or_many::OneOrMany};
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, uri::Uri};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD Relationship representation (ETSI GS CIM 009 clause 5.2.6).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    /// One or more target object URIs.
    pub object: OneOrMany<Uri>,
    /// Optional target node type(s).
    pub object_type: Option<OneOrMany<NgsiLdReference>>,
    /// Previous target object emitted in notifications when `showChanges` is active.
    pub previous_object: Option<OneOrMany<Uri>>,
    /// Inline linked entity or entities returned for `join=inline`.
    pub entity: Option<InlineEntity>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, one_or_many::OneOrMany};

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
            panic!("expected Relationship");
        };
        assert!(matches!(relationship.object, OneOrMany::Many(_)));
        assert!(relationship.object_type.is_some());
        assert!(relationship.previous_object.is_some());
    }

    /// A Relationship target of `length` octets.
    fn long_target(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Stop:";
        format!("{prefix}{}", "s".repeat(length - prefix.len()))
    }

    /// Clause 4.6.2 sets no length on a URI, and a Relationship's `object`
    /// (Table 5.2.6-1) is "Valid URI or an Array of Valid URIs": a target far
    /// longer than any identifier the broker stores is a valid object, in the
    /// single form and inside the array form.
    #[test]
    fn a_relationship_object_of_six_thousand_octets_parses_in_both_forms() {
        let target = long_target(6000);
        for object in [serde_json::json!(target), serde_json::json!(["urn:ngsi-ld:Stop:1", target])] {
            let attr: Attribute = serde_json::from_value(serde_json::json!({"type": "Relationship", "object": object})).expect("a long object is a URI");
            let Attribute::Relationship(relationship) = attr else {
                panic!("expected Relationship");
            };
            assert!(
                relationship.object.iter().any(|uri| uri.as_str() == target),
                "the long target survives byte for byte"
            );
        }
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
