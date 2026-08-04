use crate::domain::attribute::{
    AttributeValue,
    common::{CommonAttributeFields, InlineEntity, OneOrMany, RelationshipObjectList},
};
use compact_str::CompactString;
use indexmap::IndexMap;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD ListRelationship representation (ETSI GS CIM 009 clause 5.2.37).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:ListRelationship",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct ListRelationship {
    #[jsonld(property = "ngsi:hasObjectList", container = "list", coerce = "@id")]
    pub object_list: RelationshipObjectList,
    #[jsonld(property = "ngsi:hasObjectType", passthrough)]
    pub object_type: Option<OneOrMany<CompactString>>,
    #[jsonld(property = "ngsi:entityList", passthrough)]
    pub entity_list: Option<InlineEntity>,
    #[jsonld(property = "ngsi:hasPreviousObjectList", container = "list", coerce = "@id")]
    pub previous_object_list: Option<RelationshipObjectList>,
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

impl Default for ListRelationship {
    fn default() -> ListRelationship {
        ListRelationship {
            object_list: RelationshipObjectList(Vec::new()),
            object_type: None,
            entity_list: None,
            previous_object_list: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use jsonld::Expandable;

    #[test]
    fn list_relationship_expanded_has_list_of_id_refs() {
        let json = serde_json::json!({
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"],
            "previousObjectList": ["urn:ngsi-ld:Entity:0"]
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("list_relationship");
        let crate::domain::attribute::Attribute::ListRelationship(lr) = &attr else {
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
}
