use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandableMacro,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD ListRelationship representation (ETSI GS CIM 009 clause 5.2.37).
///
/// A ListRelationship represents an attribute whose value is an ordered list
/// of entity references (URIs). Contains the object list, optional metadata
/// (observedAt, datasetId), and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/ListRelationship")]
pub struct ListRelationship {
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/hasObjectList", list, id_ref)]
    pub object_list: Vec<Uri>,
    #[serde(default)]
    #[jsonld(
        property = "https://uri.etsi.org/ngsi-ld/observedAt",
        typed_value,
        datatype = "https://uri.etsi.org/ngsi-ld/DateTime"
    )]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(
        property = "https://uri.etsi.org/ngsi-ld/expiresAt",
        typed_value,
        datatype = "https://uri.etsi.org/ngsi-ld/DateTime"
    )]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/datasetId", id_ref)]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonLdExpandable;

    #[test]
    fn list_relationship_expanded_has_list_of_id_refs() {
        let json = serde_json::json!({
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"]
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("list_relationship");
        let crate::domain::attribute::Attribute::ListRelationship(lr) = &attr else {
            panic!("expected ListRelationship");
        };
        let expanded = lr.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/ListRelationship"]));
        let has_object_list = &obj["https://uri.etsi.org/ngsi-ld/hasObjectList"];
        assert_eq!(
            has_object_list,
            &serde_json::json!([{"@list": [{"@id": "urn:ngsi-ld:Entity:1"}, {"@id": "urn:ngsi-ld:Entity:2"}]}])
        );
    }
}
