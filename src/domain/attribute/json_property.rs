use crate::{
    domain::attribute::{AttributeValue, common::CommonAttributeFields},
    jsonld::JsonLdExpandableMacro,
};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD JsonProperty representation (ETSI GS CIM 009 clause 5.2.38).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:JsonProperty")]
pub struct JsonProperty {
    /// Raw non-expandable JSON payload.
    #[jsonld(property = "ngsi:hasJSON", json_value)]
    pub json: Value,
    /// Optional JSON-LD datatype for `json`.
    #[jsonld(property = "ngsi:hasValueType", vocab)]
    pub value_type: Option<CompactString>,
    /// Previous JSON payload emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousJson", json_value)]
    pub previous_json: Option<Value>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

impl Default for JsonProperty {
    fn default() -> JsonProperty {
        JsonProperty {
            json: Value::Null,
            value_type: None,
            previous_json: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        domain::attribute::Attribute,
        jsonld::{JsonLdExpandable, vocab},
    };

    #[test]
    fn json_property_round_trip_supports_previous_json() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": { "key": "value" },
            "previousJson": { "key": "old" }
        });
        let attr: Attribute = serde_json::from_value(json).expect("json property");
        let Attribute::JsonProperty(property) = attr else {
            unreachable!("expected JsonProperty");
        };
        assert!(property.previous_json.is_some());
    }

    #[test]
    fn json_property_expanded_includes_previous_json_member() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": { "key": "value" },
            "previousJson": { "key": "old" }
        });
        let attr: Attribute = serde_json::from_value(json).expect("json property");
        let Attribute::JsonProperty(property) = attr else {
            unreachable!("expected JsonProperty");
        };
        let expanded = property.to_expanded();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::PREVIOUS_JSON.as_str()));
    }
}
