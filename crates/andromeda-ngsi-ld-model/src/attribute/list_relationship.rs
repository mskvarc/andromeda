//! The `ListRelationship` attribute family (ETSI GS CIM 009 clause 5.2.37).

use crate::attribute::{common_fields::CommonAttributeFields, inline_entity::InlineEntity, object_list::RelationshipObjectList, one_or_many::OneOrMany};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD `ListRelationship` representation (ETSI GS CIM 009 clause 5.2.37).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRelationship {
    /// Ordered list of target object URIs.
    pub object_list: RelationshipObjectList,
    /// Optional target node type(s).
    pub object_type: Option<OneOrMany<NgsiLdReference>>,
    /// Inline linked entities returned for `join=inline`.
    pub entity_list: Option<InlineEntity>,
    /// Previous object list emitted in notifications when `showChanges` is active.
    pub previous_object_list: Option<RelationshipObjectList>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;

    /// Clause 4.6.2 sets no length on a URI, and every `objectList` member
    /// (Table 5.2.37-1) is a URI: a member far longer than any identifier the
    /// broker stores parses, byte for byte.
    #[test]
    fn a_list_relationship_member_of_six_thousand_octets_parses_verbatim() {
        let prefix = "urn:ngsi-ld:GtfsStopTimeUpdate:";
        let member = format!("{prefix}{}", "u".repeat(6000 - prefix.len()));
        let json =
            serde_json::json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Entity:1", member], "previousObjectList": ["urn:ngsi-ld:Entity:0"]});
        let attr: Attribute = serde_json::from_value(json).expect("a long member is a URI");
        let Attribute::ListRelationship(lr) = &attr else {
            panic!("expected ListRelationship");
        };
        let members: Vec<&str> = lr.object_list.0.iter().map(AsRef::as_ref).collect();
        pretty_assertions::assert_eq!(members, vec!["urn:ngsi-ld:Entity:1", member.as_str()]);
        assert!(lr.previous_object_list.is_some());
    }

    #[test]
    fn the_list_relationship_default_matches_the_field_by_field_literal() {
        use crate::attribute::{list_relationship::ListRelationship, object_list::RelationshipObjectList};
        let actual = ListRelationship::default();
        pretty_assertions::assert_eq!(actual.object_list, RelationshipObjectList(Vec::new()));
        assert!(actual.object_type.is_none() && actual.entity_list.is_none() && actual.previous_object_list.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
