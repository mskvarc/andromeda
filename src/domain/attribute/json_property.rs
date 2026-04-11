use crate::{domain::attribute::AttributeValue, jsonld::JsonLdExpandableMacro};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use urn::Urn;

/// NGSI-LD JsonProperty representation (ETSI GS CIM 009 clause 5.2.38).
///
/// A JsonProperty represents an attribute whose value is an arbitrary JSON
/// document. Contains the JSON value, optional metadata (observedAt, datasetId),
/// and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:JsonProperty")]
pub struct JsonProperty {
    #[jsonld(property = "ngsi:hasJSON", json_value)]
    pub json: Value,
    #[serde(default)]
    #[jsonld(property = "ngsi:observedAt", typed_value, datatype = "ngsi:DateTime")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(property = "ngsi:expiresAt", typed_value, datatype = "ngsi:DateTime")]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "ngsi:datasetId", id_ref)]
    pub dataset_id: Option<Urn>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonLdExpandable;

    #[test]
    fn json_property_expanded_has_json_typed_value() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": {"key": "value", "num": 42}
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("json_property");
        let crate::domain::attribute::Attribute::JsonProperty(j) = &attr else {
            panic!("expected JsonProperty");
        };
        let expanded = j.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/JsonProperty"]));
        let has_json = &obj["https://uri.etsi.org/ngsi-ld/hasJSON"];
        assert_eq!(has_json[0]["@type"], "@json");
        assert_eq!(has_json[0]["@value"], serde_json::json!({"key": "value", "num": 42}));
    }
}
