//! The ListRelationship attribute family (ETSI GS CIM 009 clause 5.2.37).

use crate::attribute::{common_fields::CommonAttributeFields, inline_entity::InlineEntity, object_list::RelationshipObjectList, one_or_many::OneOrMany};
use compact_str::CompactString;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD ListRelationship representation (ETSI GS CIM 009 clause 5.2.37).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:ListRelationship",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct ListRelationship {
    /// Ordered list of target object URIs.
    #[jsonld(property = "ngsi:hasObjectList", container = "list", coerce = "@id")]
    pub object_list: RelationshipObjectList,
    /// Optional target node type(s).
    #[jsonld(property = "ngsi:hasObjectType", passthrough)]
    pub object_type: Option<OneOrMany<CompactString>>,
    /// Inline linked entities returned for `join=inline`.
    #[jsonld(property = "ngsi:entityList", passthrough)]
    pub entity_list: Option<InlineEntity>,
    /// Previous object list emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousObjectList", container = "list", coerce = "@id")]
    pub previous_object_list: Option<RelationshipObjectList>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use jsonld::Expandable;

    #[test]
    fn list_relationship_expanded_has_list_of_id_refs() {
        let json = serde_json::json!({
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"],
            "previousObjectList": ["urn:ngsi-ld:Entity:0"]
        });
        let attr: Attribute = serde_json::from_value(json).expect("list_relationship");
        let Attribute::ListRelationship(lr) = &attr else {
            unreachable!("expected ListRelationship");
        };
        let expanded = lr.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("expanded is object");

        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/ListRelationship"]));
        let has_object_list = &obj["https://uri.etsi.org/ngsi-ld/hasObjectList"];
        pretty_assertions::assert_eq!(
            has_object_list,
            &serde_json::json!([{"@list": [{"@id": "urn:ngsi-ld:Entity:1"}, {"@id": "urn:ngsi-ld:Entity:2"}]}])
        );
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/hasPreviousObjectList"));
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
