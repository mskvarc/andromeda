use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandableMacro,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iref::IriBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD Property representation (ETSI GS CIM 009 clause 5.2.5).
///
/// A Property represents an attribute whose value is any JSON value.
/// Contains the value, optional metadata (observedAt, unitCode, datasetId),
/// and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:Property")]
pub struct Property {
    #[jsonld(property = "ngsi:hasValue")]
    pub value: Value,
    #[jsonld(property = "ngsi:unitCode")]
    pub unit_code: Option<String>,
    #[serde(default)]
    #[jsonld(property = "ngsi:observedAt", typed_value, datatype = "ngsi:DateTime")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(property = "ngsi:expiresAt", typed_value, datatype = "ngsi:DateTime")]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "ngsi:datasetId", id_ref)]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<IriBuf, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonLdExpandable;

    #[test]
    fn property_expanded_has_correct_type_and_value() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 25
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("property");
        let crate::domain::attribute::Attribute::Property(p) = &attr else {
            panic!("expected Property");
        };
        let expanded = p.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Property"]));
        assert_eq!(obj["https://uri.etsi.org/ngsi-ld/hasValue"], serde_json::json!([{"@value": 25}]));
    }

    #[test]
    fn property_expanded_includes_optional_metadata() {
        let json = serde_json::json!({
            "type": "Property",
            "value": "hello",
            "observedAt": "2024-01-01T00:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:1",
            "unitCode": "CEL"
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("property");
        let crate::domain::attribute::Attribute::Property(p) = &attr else {
            panic!("expected Property");
        };
        let expanded = p.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/observedAt"));
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/datasetId"));
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/unitCode"));
        // observedAt should be typed value
        let observed = &obj["https://uri.etsi.org/ngsi-ld/observedAt"];
        assert!(observed[0]["@type"].as_str().expect("type") == "https://uri.etsi.org/ngsi-ld/DateTime");
        // datasetId should be id_ref
        let dataset = &obj["https://uri.etsi.org/ngsi-ld/datasetId"];
        assert!(dataset[0]["@id"].is_string());
    }

    #[test]
    fn property_expanded_omits_none_metadata() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 42
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("property");
        let crate::domain::attribute::Attribute::Property(p) = &attr else {
            panic!("expected Property");
        };
        let expanded = p.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert!(!obj.contains_key("https://uri.etsi.org/ngsi-ld/observedAt"));
        assert!(!obj.contains_key("https://uri.etsi.org/ngsi-ld/expiresAt"));
        assert!(!obj.contains_key("https://uri.etsi.org/ngsi-ld/datasetId"));
        assert!(!obj.contains_key("https://uri.etsi.org/ngsi-ld/unitCode"));
    }
}
