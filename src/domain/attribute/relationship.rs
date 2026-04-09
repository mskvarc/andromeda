use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandableMacro,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD Relationship representation (ETSI GS CIM 009 clause 5.2.6).
///
/// A Relationship represents an attribute whose value is a reference (URI)
/// to another entity. Contains the object URI, optional metadata
/// (observedAt, datasetId), and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/Relationship")]
pub struct Relationship {
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/hasObject", id_ref)]
    pub object: Uri,
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
    fn relationship_expanded_has_correct_type_and_object() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1"
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("relationship");
        let crate::domain::attribute::Attribute::Relationship(r) = &attr else {
            panic!("expected Relationship");
        };
        let expanded = r.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Relationship"]));
        assert_eq!(
            obj["https://uri.etsi.org/ngsi-ld/hasObject"],
            serde_json::json!([{"@id": "urn:ngsi-ld:Entity:1"}])
        );
    }

    #[test]
    fn relationship_expanded_includes_metadata() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1",
            "observedAt": "2024-01-01T00:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:1"
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("relationship");
        let crate::domain::attribute::Attribute::Relationship(r) = &attr else {
            panic!("expected Relationship");
        };
        let expanded = r.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/observedAt"));
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/datasetId"));
    }
}
